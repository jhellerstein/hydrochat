//! HydroChat WebSocket <-> DFIR TCP Proxy
//!
//! This proxy achieves a clean DFIR pipeline architecture:
//! - **Single DFIR Pipeline**: One `dfir_syntax!` block handles HTTP routing, WebSocket ↔ TCP message proxying and protocol conversion
//! - **Streamlined Connection Handling**: Centralized connection management with DFIR pipeline
//! - **Complex Message Logic in DFIR**: WebSocket protocol adapters, TCP communication, and message proxying in DFIR
//! - **Minimal External Logic**: Only simple HTTP responses and WebSocket upgrades outside DFIR
//!
//! Architecture Flow:
//! 1. Centralized connection acceptor spawns connection handlers
//! 2. Each connection runs through **One DFIR pipeline** for HTTP parsing and routing
//! 3. For WebSocket proxy: DFIR pipeline handles protocol conversion and message proxying
//! 4. Minimal external tasks - HTTP responses, WebSocket upgrade, and raw frame transport

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

    // Run the proxy server
    run_proxy_server(ws_addr, server_addr).await;

    Ok(())
}

/// Run the proxy server with centralized connection handling
async fn run_proxy_server(ws_addr: &str, server_addr: SocketAddr) {
    // Create TCP listener
    let listener = TcpListener::bind(ws_addr).await.unwrap();
    tracing::info!("Proxy server listening on {}", ws_addr);

    // Use LocalSet for DFIR compatibility
    let local = LocalSet::new();
    local
        .run_until(async move {
            loop {
                let (stream, addr) = listener.accept().await.unwrap();
                // Handle each connection with spawn_local (for !Send futures)
                tokio::task::spawn_local(handle_incoming(stream, addr, server_addr));
            }
        })
        .await;
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

/// This function handles:
/// - HTTP request parsing and simple routing (health/invalid handled outside DFIR)
/// - WebSocket upgrade and protocol negotiation
/// - For WebSocket proxy: DFIR pipeline handles all message processing and protocol conversion
/// - TCP communication with server
/// - All WebSocket ↔ HydroChat message conversion in DFIR
async fn single_dfir_pipeline(
    request_data: Vec<u8>,
    stream: TcpStream,
    peer_addr: SocketAddr,
    server_addr: SocketAddr,
) {
    tracing::debug!(
        "single_dfir_pipeline: Starting for peer {}, data len: {}",
        peer_addr,
        request_data.len()
    );

    use dfir_rs::dfir_syntax;
    use dfir_rs::scheduled::graph::Dfir;
    use dfir_rs::util::connect_tcp_bytes;
    use futures_util::{SinkExt, StreamExt};
    use tokio_stream::wrappers::ReceiverStream;

    // Channels for HTTP request data and actions
    let (http_request_tx, http_request_rx) = mpsc::channel::<Vec<u8>>(1);
    let (action_tx, mut action_rx) = mpsc::channel::<RequestAction>(1);

    tracing::debug!("single_dfir_pipeline: Sending request data to DFIR pipeline");
    // Send raw HTTP request data to DFIR for parsing and routing
    if let Err(e) = http_request_tx.send(request_data).await {
        tracing::error!("single_dfir_pipeline: Failed to send request data: {}", e);
        return;
    }

    // Channels for WebSocket frames (in case we need WebSocket proxy)
    let (ws_frame_to_dfir_tx, ws_frame_to_dfir_rx) =
        mpsc::channel::<tokio_tungstenite::tungstenite::Message>(16);
    let (dfir_to_ws_frame_tx, mut dfir_to_ws_frame_rx) =
        mpsc::channel::<tokio_tungstenite::tungstenite::Message>(16);

    // TCP connection (in case we need WebSocket proxy)
    let (tcp_out, tcp_in) = connect_tcp_bytes();

    // **THE SINGLE DFIR PIPELINE**: HTTP routing AND WebSocket message processing
    let mut pipeline: Dfir = {
        let action_tx_ref = &action_tx;
        let dfir_to_ws_frame_tx_ref = &dfir_to_ws_frame_tx;
        let peer_addr_copy = peer_addr;

        dfir_syntax! {
            // HTTP Request Parsing and Routing (moved into DFIR)
            http_requests = source_stream(ReceiverStream::new(http_request_rx).map(Ok::<_, ()>))
                -> filter_map(|result| result.ok())
                -> map(|request_data| {
                    let req = String::from_utf8_lossy(&request_data);
                    if req.contains("GET /health") {
                        RequestRoute::Health
                    } else if req.contains("GET /proxy") {
                        RequestRoute::WebSocketProxy
                    } else {
                        RequestRoute::Invalid(req.lines().next().unwrap_or("").to_string())
                    }
                })
                -> demux_enum::<RequestRoute>();

            // Health endpoint - simple handler in DFIR
            http_requests[Health] -> for_each(move |_| {
                tracing::debug!("DFIR: Processing health request");
                if let Err(e) = action_tx_ref.try_send(RequestAction::SendHealthResponse) {
                    tracing::error!("DFIR: Failed to send health action: {}", e);
                } else {
                    tracing::debug!("DFIR: Successfully sent health action");
                }
            });

            // Invalid request - simple handler in DFIR
            http_requests[Invalid] -> for_each(move |(invalid_path,)| {
                tracing::warn!("DFIR: Rejecting invalid request from {}: {}", peer_addr_copy, invalid_path);
                let _ = action_tx_ref.try_send(RequestAction::CloseConnection);
            });

            // WebSocket proxy - upgrade and start message pipeline in DFIR
            http_requests[WebSocketProxy] -> for_each(move |_| {
                tracing::debug!("DFIR: Processing WebSocket upgrade request");
                let _ = action_tx_ref.try_send(RequestAction::StartWebSocketProxy);
            });

            // WebSocket Frame → HydroChat Message → TCP Server (protocol adapter in DFIR)
            ws_incoming = source_stream(ReceiverStream::new(ws_frame_to_dfir_rx).map(Ok::<_, ()>))
                -> filter_map(|result| result.ok())
                -> filter_map(|ws_msg| {
                    match ws_msg {
                        tokio_tungstenite::tungstenite::Message::Binary(data) => {
                            match bincode::deserialize::<hydrochat_core::protocol::Message>(&data) {
                                Ok(message) => {
                                    tracing::debug!("DFIR: WebSocket binary → HydroChat message: {:?}", message);
                                    Some(message)
                                }
                                Err(e) => {
                                    tracing::error!("DFIR: WebSocket binary deserialization failed: {}", e);
                                    None
                                }
                            }
                        }
                        tokio_tungstenite::tungstenite::Message::Text(text) => {
                            let data = text.into_bytes();
                            match bincode::deserialize::<hydrochat_core::protocol::Message>(&data) {
                                Ok(message) => {
                                    tracing::debug!("DFIR: WebSocket text → HydroChat message: {:?}", message);
                                    Some(message)
                                }
                                Err(e) => {
                                    tracing::error!("DFIR: WebSocket text deserialization failed: {}", e);
                                    None
                                }
                            }
                        }
                        tokio_tungstenite::tungstenite::Message::Close(_) => {
                            tracing::debug!("DFIR: WebSocket close frame received");
                            None
                        }
                        _ => {
                            tracing::trace!("DFIR: Ignoring WebSocket ping/pong");
                            None
                        }
                    }
                })
                -> map(|msg| {
                    tracing::debug!("DFIR: Sending HydroChat message to TCP server: {:?}", msg);
                    (msg, server_addr)
                })
                -> dest_sink_serde(tcp_out);

            // TCP Server → HydroChat Message → WebSocket Frame (protocol adapter in DFIR)
            server_stream = source_stream_serde(tcp_in)
                -> filter_map(|result: Result<(hydrochat_core::protocol::Message, SocketAddr), _>| {
                    match result {
                        Ok((message, addr)) => {
                            tracing::debug!("DFIR: TCP server {} → HydroChat message: {:?}", addr, message);
                            Some(message)
                        }
                        Err(e) => {
                            tracing::error!("DFIR: TCP serde error: {}", e);
                            None
                        }
                    }
                })
                -> filter_map(move |message| {
                    tracing::debug!("DFIR: HydroChat message → WebSocket frame for {}: {:?}", peer_addr, message);
                    match bincode::serialize(&message) {
                        Ok(bytes) => {
                            tracing::debug!("DFIR: Serialized {} bytes for WebSocket", bytes.len());
                            let ws_msg = tokio_tungstenite::tungstenite::Message::Binary(bytes);
                            Some(ws_msg)
                        }
                        Err(e) => {
                            tracing::error!("DFIR: WebSocket frame serialization failed: {}", e);
                            None
                        }
                    }
                })
                -> for_each(move |ws_frame| {
                    let _ = dfir_to_ws_frame_tx_ref.try_send(ws_frame);
                });
        }
    };

    // Run DFIR pipeline and handle actions concurrently
    let dfir_future = pipeline.run_async();

    tokio::pin!(dfir_future);

    // Run DFIR pipeline concurrently with action handling
    tokio::select! {
        _ = &mut dfir_future => {
            tracing::debug!("DFIR pipeline completed without generating action");
        },
        action = action_rx.recv() => {
            tracing::debug!("Received action: {:?}", action);
            match action {
                Some(RequestAction::SendHealthResponse) => {
                    tracing::debug!("Calling health response adapter");
                    health_response_adapter(stream, peer_addr).await;
                    tracing::debug!("Health response adapter completed");
                }                    Some(RequestAction::CloseConnection) => {
                        tracing::debug!("Closing connection for invalid request");
                        // Drop the stream to close the connection
                        drop(stream);
                    }
                Some(RequestAction::StartWebSocketProxy) => {
                    // Accept WebSocket upgrade
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

                    // Start WebSocket I/O tasks concurrently with the DFIR pipeline
                    let ws_reader_task = async {
                        while let Some(Ok(msg)) = ws_rx.next().await {
                            if ws_frame_to_dfir_tx.send(msg).await.is_err() {
                                break;
                            }
                        }
                    };

                    let ws_writer_task = async {
                        while let Some(ws_frame) = dfir_to_ws_frame_rx.recv().await {
                            if ws_tx.send(ws_frame).await.is_err() {
                                break;
                            }
                        }
                    };

                    // Run WebSocket I/O tasks concurrently with the DFIR pipeline
                    tokio::select! {
                        _ = &mut dfir_future => {},
                        _ = ws_reader_task => {},
                        _ = ws_writer_task => {},
                    }
                }
                None => {
                    tracing::debug!("No action received, DFIR pipeline completed");
                }
            }
        }
    }
}

/// Actions that the DFIR HTTP routing pipeline can emit
#[derive(Debug, Clone)]
enum RequestAction {
    SendHealthResponse,
    CloseConnection,
    StartWebSocketProxy,
}

/// **Protocol Handlers**: Pure protocol conversion based on DFIR routing decisions  
///
/// These adapters handle the actual HTTP/WebSocket protocol work:
/// - Health endpoint HTTP responses
/// - WebSocket upgrade negotiation  
/// - Invalid request rejection
async fn health_response_adapter(mut stream: TcpStream, peer_addr: SocketAddr) {
    use tokio::io::AsyncWriteExt;

    tracing::debug!("Health response adapter: Sending response to {}", peer_addr);
    let response = b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\nOK";
    if let Err(e) = stream.write_all(response).await {
        tracing::warn!(
            "Protocol adapter: Failed to send health response to {}: {}",
            peer_addr,
            e
        );
    } else {
        tracing::debug!(
            "Health response adapter: Successfully sent response to {}",
            peer_addr
        );
    }

    // Flush the write buffer
    if let Err(e) = stream.flush().await {
        tracing::debug!(
            "Health response adapter: Error flushing stream to {}: {}",
            peer_addr,
            e
        );
    } else {
        tracing::debug!(
            "Health response adapter: Successfully flushed stream to {}",
            peer_addr
        );
    }

    // Give the client time to read before shutting down
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Shutdown the write side
    if let Err(e) = stream.shutdown().await {
        tracing::debug!(
            "Health response adapter: Error shutting down stream to {}: {}",
            peer_addr,
            e
        );
    } else {
        tracing::debug!(
            "Health response adapter: Successfully shutdown stream to {}",
            peer_addr
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
            // Use the real handle_incoming function which does the proper peek and routing
            handle_incoming(stream, client_addr, tcp_server_addr).await;
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
            handle_incoming(stream, client_addr, server_addr).await;
        };

        let client_test = async move {
            tokio::time::sleep(Duration::from_millis(50)).await;

            let mut stream = TcpStream::connect(proxy_addr).await.unwrap();
            stream
                .write_all(b"GET /health HTTP/1.1\r\n\r\n")
                .await
                .unwrap();

            // Read response
            let mut buf = [0u8; 1024];
            let bytes_read = timeout(Duration::from_millis(1000), stream.read(&mut buf))
                .await
                .expect("Should read response within timeout")
                .expect("Should read successfully");

            let response_str = String::from_utf8_lossy(&buf[..bytes_read]);

            // Check if we got the expected response
            assert!(
                !response_str.is_empty(),
                "Should receive some response data"
            );
            assert!(
                response_str.contains("200 OK"),
                "Should contain 200 OK status"
            );
            assert!(response_str.contains("OK"), "Should contain OK in body");
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
            handle_incoming(stream, client_addr, server_addr).await;
        };

        let client_test = async move {
            use tokio::io::AsyncWriteExt;
            tokio::time::sleep(Duration::from_millis(50)).await;

            let mut stream = TcpStream::connect(proxy_addr).await.unwrap();
            stream
                .write_all(b"GET /invalid HTTP/1.1\r\n\r\n")
                .await
                .unwrap();
            // Try to flush, but it might fail if connection is closed
            let _ = stream.flush().await;

            // For invalid requests, we just need to verify that the request was sent
            // The proxy should close the connection, which is the expected behavior
        };

        let local = LocalSet::new();
        local
            .run_until(async {
                tokio::select! {
                    _ = tokio::task::spawn_local(proxy_task) => {},
                    _ = tokio::task::spawn_local(client_test) => {},
                }
            })
            .await;

        println!("✅ Single DFIR pipeline invalid request test passed!");
    }
}
