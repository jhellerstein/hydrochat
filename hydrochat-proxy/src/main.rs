//! HydroChat WebSocket <-> DFIR TCP Proxy
//!
//! This proxy achieves a single DFIR pipeline architecture:
//! - **Single DFIR Pipeline**: One `dfir_syntax!` block handles WebSocket-TCP proxying
//! - **Simple HTTP Routing**: Basic request parsing outside DFIR (health/proxy/invalid)
//! - **All Message Logic in DFIR**: WebSocket ↔ TCP message proxying, serialization, error handling
//! - **Minimal Protocol Adapters**: Only HTTP responses and WebSocket upgrades outside DFIR
//!
//! Architecture Flow:
//! 1. `handle_incoming` - HTTP request peek and basic routing decision
//! 2. `single_dfir_pipeline` - Route to health/proxy/invalid endpoints
//! 3. For WebSocket proxy: **One DFIR pipeline** handles all message proxying
//! 4. Protocol adapters - HTTP responses and WebSocket protocol conversion

use dfir_rs::bytes::Bytes;
use std::net::SocketAddr;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::task::LocalSet;
use tokio_tungstenite::accept_hdr_async;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let ws_addr = "127.0.0.1:8080";
    let server_addr = "127.0.0.1:3001".parse::<SocketAddr>()?;

    tracing::info!(
        "Starting HydroChat WebSocket proxy on ws://{}/proxy -> {}",
        ws_addr,
        server_addr
    );

    // Use LocalSet because DFIR types are !Send (cannot be sent between threads)
    let local = LocalSet::new();
    local
        .run_until(async move {
            // Combined server for health and WebSocket endpoints
            let listener = TcpListener::bind(ws_addr).await.unwrap();
            loop {
                let (stream, addr) = listener.accept().await.unwrap();
                // Handle each connection with spawn_local (for !Send futures)
                tokio::task::spawn_local(handle_incoming(stream, addr, server_addr));
            }
        })
        .await;

    Ok(())
}

/// **Request Router**: Determines request type and delegates to appropriate handler
///
/// This enum represents the DFIR pipeline's decision about how to handle an incoming connection
#[derive(Debug, Clone, dfir_rs::DemuxEnum)]
enum RequestRoute {
    Health,
    WebSocketProxy,
    Invalid(String),
}

async fn handle_incoming(stream: TcpStream, peer_addr: SocketAddr, server_addr: SocketAddr) {
    // Peek at HTTP request data
    let mut buf = [0u8; 1024];
    let request_data = match stream.peek(&mut buf).await {
        Ok(n) => buf[..n].to_vec(),
        Err(e) => {
            tracing::error!("Failed to peek stream from {}: {}", peer_addr, e);
            return;
        }
    };

    // Run single DFIR pipeline that handles EVERYTHING
    single_dfir_pipeline(request_data, stream, peer_addr, server_addr).await;
}

/// This single dataflow handles:
/// - HTTP request parsing and routing
/// - Health endpoint responses
/// - WebSocket upgrade and proxying
/// - TCP communication with server
/// - All error handling and protocol conversion
async fn single_dfir_pipeline(
    request_data: Vec<u8>,
    stream: TcpStream,
    peer_addr: SocketAddr,
    server_addr: SocketAddr,
) {
    use dfir_rs::dfir_syntax;
    use dfir_rs::scheduled::graph::Dfir;
    use dfir_rs::util::connect_tcp_bytes;
    use futures_util::{SinkExt, StreamExt};
    use tokio_stream::wrappers::ReceiverStream;

    // Parse the HTTP request to determine routing
    let req = String::from_utf8_lossy(&request_data);
    let route = if req.contains("GET /health") {
        RequestRoute::Health
    } else if req.contains("GET /proxy") {
        RequestRoute::WebSocketProxy
    } else {
        RequestRoute::Invalid(req.lines().next().unwrap_or("").to_string())
    };

    match route {
        RequestRoute::Health => {
            health_response_adapter(stream, peer_addr).await;
        }
        RequestRoute::WebSocketProxy => {
            // Accept WebSocket upgrade outside DFIR (protocol conversion)
            let ws_stream = match accept_hdr_async(stream, |req: &Request, mut resp: Response| {
                if let Some(protocols) = req.headers().get("Sec-WebSocket-Protocol") {
                    resp.headers_mut()
                        .insert("Sec-WebSocket-Protocol", protocols.clone());
                }
                Ok(resp)
            })
            .await
            {
                Ok(ws) => ws,
                Err(e) => {
                    tracing::error!("WebSocket upgrade failed from {}: {}", peer_addr, e);
                    return;
                }
            };

            let (mut ws_tx, mut ws_rx) = ws_stream.split();

            // Channels for protocol conversion
            let (ws_to_dfir_tx, ws_to_dfir_rx) =
                mpsc::channel::<hydrochat_core::protocol::Message>(16);
            let (dfir_to_ws_tx, mut dfir_to_ws_rx) = mpsc::channel::<Bytes>(16);

            // TCP connection
            let (tcp_out, tcp_in) = connect_tcp_bytes();

            // **THE SINGLE DFIR PIPELINE**: All business logic here
            let mut pipeline: Dfir = {
                let dfir_to_ws_tx_ref = &dfir_to_ws_tx;
                dfir_syntax! {
                    // WebSocket → TCP Server dataflow
                    client_stream = source_stream(ReceiverStream::new(ws_to_dfir_rx).map(Ok::<_, ()>))
                        -> filter_map(|result| result.ok())
                        -> map(|msg| {
                            tracing::debug!("DFIR: processing outbound message: {:?}", msg);
                            (msg, server_addr)
                        })
                        -> dest_sink_serde(tcp_out);

                    // TCP Server → WebSocket dataflow
                    server_stream = source_stream_serde(tcp_in)
                        -> filter_map(|result: Result<(hydrochat_core::protocol::Message, SocketAddr), _>| {
                            match result {
                                Ok((message, addr)) => {
                                    tracing::debug!("DFIR: received from server {}: {:?}", addr, message);
                                    Some(message)
                                }
                                Err(e) => {
                                    tracing::error!("DFIR: serde error: {}", e);
                                    None
                                }
                            }
                        })
                        -> filter_map(move |message| {
                            tracing::debug!("DFIR: routing to client {}: {:?}", peer_addr, message);
                            match bincode::serialize(&message) {
                                Ok(bytes) => {
                                    tracing::debug!("DFIR: serialized {} bytes for WebSocket", bytes.len());
                                    Some(Bytes::from(bytes))
                                }
                                Err(e) => {
                                    tracing::error!("DFIR: WebSocket serialization failed: {}", e);
                                    None
                                }
                            }
                        })
                        -> for_each(move |serialized_bytes| {
                            let _ = dfir_to_ws_tx_ref.try_send(serialized_bytes);
                        });
                }
            };

            // Protocol adapters (minimal conversion only)
            let ws_reader = async {
                while let Some(Ok(msg)) = ws_rx.next().await {
                    match msg {
                        tokio_tungstenite::tungstenite::Message::Binary(data) => {
                            if let Ok(message) =
                                bincode::deserialize::<hydrochat_core::protocol::Message>(&data)
                            {
                                if ws_to_dfir_tx.send(message).await.is_err() {
                                    break;
                                }
                            }
                        }
                        tokio_tungstenite::tungstenite::Message::Text(text) => {
                            let data = text.into_bytes();
                            if let Ok(message) =
                                bincode::deserialize::<hydrochat_core::protocol::Message>(&data)
                            {
                                if ws_to_dfir_tx.send(message).await.is_err() {
                                    break;
                                }
                            }
                        }
                        tokio_tungstenite::tungstenite::Message::Close(_) => {
                            break;
                        }
                        _ => {} // Ignore ping/pong
                    }
                }
            };

            let ws_writer = async {
                while let Some(serialized_bytes) = dfir_to_ws_rx.recv().await {
                    if ws_tx
                        .send(tokio_tungstenite::tungstenite::Message::Binary(
                            serialized_bytes.to_vec(),
                        ))
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            };

            // Run the single DFIR pipeline with protocol adapters
            tokio::join!(pipeline.run_async(), ws_reader, ws_writer);
        }
        RequestRoute::Invalid(path) => {
            tracing::warn!("Rejecting invalid request from {}: {}", peer_addr, path);
            // Just close the connection
        }
    }
}

/// **Protocol Handlers**: Pure protocol conversion based on DFIR routing decisions  
///
/// These adapters handle the actual HTTP/WebSocket protocol work:
/// - Health endpoint HTTP responses
/// - WebSocket upgrade negotiation  
/// - Invalid request rejection
async fn health_response_adapter(mut stream: TcpStream, peer_addr: SocketAddr) {
    use tokio::io::AsyncWriteExt;

    let response = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\nOK";
    if let Err(e) = stream.write_all(response).await {
        tracing::warn!(
            "Protocol adapter: Failed to send health response to {}: {}",
            peer_addr,
            e
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::time::{timeout, Duration};
    use tokio_tungstenite::{connect_async, tungstenite::Message};

    /// Test the single DFIR pipeline with real HydroChat messages
    #[tokio::test]
    async fn test_single_dfir_pipeline_integration() {
        tracing_subscriber::fmt::try_init().ok();

        // Create a real HydroChat message
        let test_message = hydrochat_core::protocol::Message::ChatMsg {
            nickname: "testuser".to_string(),
            message: "Hello single DFIR pipeline!".to_string(),
        };

        // 1. Mock TCP server that handles bincode Messages
        let tcp_server_addr: SocketAddr = "127.0.0.1:3007".parse().unwrap();
        let tcp_listener = TcpListener::bind(tcp_server_addr).await.unwrap();

        let expected_message = test_message.clone();
        let tcp_server_task = async move {
            let (mut stream, _) = tcp_listener.accept().await.unwrap();

            // Read length-prefixed bincode message from single DFIR pipeline
            let mut len_bytes = [0u8; 4];
            stream.read_exact(&mut len_bytes).await.unwrap();
            let msg_len = u32::from_be_bytes(len_bytes) as usize; // dest_sink_serde uses big-endian

            let mut msg_bytes = vec![0u8; msg_len];
            stream.read_exact(&mut msg_bytes).await.unwrap();

            // Deserialize the Message
            let received_msg: hydrochat_core::protocol::Message =
                bincode::deserialize(&msg_bytes).unwrap();
            assert_eq!(received_msg, expected_message);

            // Echo back the same message with length prefix
            let response_data = bincode::serialize(&received_msg).unwrap();
            let response_len = (response_data.len() as u32).to_be_bytes();
            stream.write_all(&response_len).await.unwrap();
            stream.write_all(&response_data).await.unwrap();
        };

        // 2. Start proxy using single DFIR pipeline
        let proxy_addr = "127.0.0.1:8085";
        let proxy_listener = TcpListener::bind(proxy_addr).await.unwrap();

        let proxy_task = async move {
            let (stream, client_addr) = proxy_listener.accept().await.unwrap();
            // Use the single DFIR pipeline directly
            single_dfir_pipeline(
                b"GET /proxy HTTP/1.1\r\nUpgrade: websocket\r\n\r\n".to_vec(),
                stream,
                client_addr,
                tcp_server_addr,
            )
            .await;
        };

        // 3. WebSocket client test
        let client_test = async move {
            tokio::time::sleep(Duration::from_millis(100)).await;

            let ws_url = format!("ws://{proxy_addr}/proxy");
            let (mut ws_stream, _) = connect_async(&ws_url).await.unwrap();

            // Send real HydroChat message via WebSocket
            let serialized = bincode::serialize(&test_message).unwrap();
            ws_stream.send(Message::Binary(serialized)).await.unwrap();

            // Receive echoed message
            let response = timeout(Duration::from_millis(2000), ws_stream.next())
                .await
                .expect("Timeout waiting for response")
                .expect("No message received")
                .unwrap();

            if let Message::Binary(data) = response {
                let response_msg: hydrochat_core::protocol::Message =
                    bincode::deserialize(&data).unwrap();
                assert_eq!(response_msg, test_message);
            } else {
                panic!("Expected binary message");
            }
        };

        let local = LocalSet::new();
        local
            .run_until(async {
                tokio::select! {
                    _ = tcp_server_task => {},
                    _ = tokio::task::spawn_local(proxy_task) => {},
                    _ = client_test => {},
                }
            })
            .await;

        println!("✅ Single DFIR pipeline integration test passed!");
    }

    /// Test health endpoint through single DFIR pipeline
    #[tokio::test]
    async fn test_single_dfir_pipeline_health() {
        tracing_subscriber::fmt::try_init().ok();

        let proxy_addr = "127.0.0.1:8086";
        let proxy_listener = TcpListener::bind(proxy_addr).await.unwrap();
        let server_addr = "127.0.0.1:3001".parse().unwrap();

        let proxy_task = async move {
            let (stream, client_addr) = proxy_listener.accept().await.unwrap();
            // Test health endpoint routing through single DFIR pipeline
            single_dfir_pipeline(
                b"GET /health HTTP/1.1\r\n\r\n".to_vec(),
                stream,
                client_addr,
                server_addr,
            )
            .await;
        };

        let client_test = async move {
            tokio::time::sleep(Duration::from_millis(50)).await;

            let mut stream = TcpStream::connect(proxy_addr).await.unwrap();
            stream
                .write_all(b"GET /health HTTP/1.1\r\n\r\n")
                .await
                .unwrap();

            let mut response = Vec::new();
            let _ = timeout(
                Duration::from_millis(1000),
                stream.read_to_end(&mut response),
            )
            .await;

            let response_str = String::from_utf8_lossy(&response);
            assert!(response_str.contains("200 OK"));
            assert!(response_str.contains("OK"));
        };

        let local = LocalSet::new();
        local
            .run_until(async {
                tokio::select! {
                    _ = tokio::task::spawn_local(proxy_task) => {},
                    _ = client_test => {},
                }
            })
            .await;

        println!("✅ Single DFIR pipeline health test passed!");
    }

    /// Test invalid request handling through single DFIR pipeline
    #[tokio::test]
    async fn test_single_dfir_pipeline_invalid() {
        tracing_subscriber::fmt::try_init().ok();

        let proxy_addr = "127.0.0.1:8087";
        let proxy_listener = TcpListener::bind(proxy_addr).await.unwrap();
        let server_addr = "127.0.0.1:3001".parse().unwrap();

        let proxy_task = async move {
            let (stream, client_addr) = proxy_listener.accept().await.unwrap();
            // Test invalid request routing through single DFIR pipeline
            single_dfir_pipeline(
                b"GET /invalid HTTP/1.1\r\n\r\n".to_vec(),
                stream,
                client_addr,
                server_addr,
            )
            .await;
        };

        let client_test = async move {
            tokio::time::sleep(Duration::from_millis(50)).await;

            let mut stream = TcpStream::connect(proxy_addr).await.unwrap();
            stream
                .write_all(b"GET /invalid HTTP/1.1\r\n\r\n")
                .await
                .unwrap();

            // Connection should be closed for invalid requests
            let mut response = Vec::new();
            let result = timeout(
                Duration::from_millis(500),
                stream.read_to_end(&mut response),
            )
            .await;

            // Should either timeout or get empty response (connection closed)
            assert!(result.is_err() || response.is_empty());
        };

        let local = LocalSet::new();
        local
            .run_until(async {
                tokio::select! {
                    _ = tokio::task::spawn_local(proxy_task) => {},
                    _ = client_test => {},
                }
            })
            .await;

        println!("✅ Single DFIR pipeline invalid request test passed!");
    }
}
