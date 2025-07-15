//! HydroChat WebSocket <-> DFIR TCP Proxy (tokio-tungstenite, single-threaded)

use dfir_rs::bytes::Bytes;
use dfir_rs::dfir_syntax;
use dfir_rs::scheduled::graph::Dfir;
use dfir_rs::util::connect_tcp_bytes;
use futures::future::join3;
use futures_util::{SinkExt, StreamExt};
use hydrochat_core::protocol::Message;
use std::net::SocketAddr;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Builder;
use tokio::sync::mpsc;
use tokio::task::LocalSet;
use tokio_stream::wrappers::ReceiverStream;
use tokio_tungstenite::accept_hdr_async;
use tokio_tungstenite::tungstenite::handshake::server::{Request, Response};
use tokio_tungstenite::tungstenite::protocol::Message as WsMessage;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let ws_addr = "127.0.0.1:8080";
    let server_addr = "127.0.0.1:3001".parse::<SocketAddr>()?;

    tracing::info!(
        "Starting HydroChat WebSocket proxy on ws://{}/proxy -> {}",
        ws_addr,
        server_addr
    );

    let rt = Builder::new_current_thread().enable_all().build()?;
    let local = LocalSet::new();
    local.block_on(&rt, async move {
        // Start health endpoint in background
        tokio::task::spawn_local(async move {
            if let Ok(listener) = TcpListener::bind("127.0.0.1:8080").await {
                loop {
                    if let Ok((mut stream, _)) = listener.accept().await {
                        // Minimal HTTP health check
                        let mut buf = [0u8; 1024];
                        if let Ok(n) = stream.peek(&mut buf).await {
                            let req = String::from_utf8_lossy(&buf[..n]);
                            if req.starts_with("GET /health") {
                                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\nOK").await;
                            }
                        }
                    }
                }
            }
        });

        // WebSocket server (single-threaded)
        let listener = TcpListener::bind(ws_addr).await.unwrap();
        loop {
            let (stream, addr) = listener.accept().await.unwrap();
            // Only handle /proxy path for WebSocket upgrade
            tokio::task::spawn_local(handle_incoming(stream, addr, server_addr));
        }
    });
    Ok(())
}

async fn handle_incoming(stream: TcpStream, peer_addr: SocketAddr, server_addr: SocketAddr) {
    // Peek at the HTTP request to check for /proxy path and log headers
    let mut buf = [0u8; 1024];
    let n = match stream.peek(&mut buf).await {
        Ok(n) => n,
        Err(e) => {
            tracing::error!("Failed to peek stream from {}: {}", peer_addr, e);
            return;
        }
    };
    let req = String::from_utf8_lossy(&buf[..n]);
    // tracing::info!("Incoming HTTP request from {}:\n{}", peer_addr, req);
    if !req.contains("GET /proxy") {
        tracing::warn!("Rejected non-/proxy request from {}", peer_addr);
        return;
    }
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
    // tracing::info!("WebSocket connection established from {}", peer_addr);
    if let Err(e) = handle_ws(ws_stream, server_addr).await {
        tracing::error!("Proxy error for {}: {}", peer_addr, e);
    }
}

async fn handle_ws(
    ws: tokio_tungstenite::WebSocketStream<TcpStream>,
    server_addr: SocketAddr,
) -> anyhow::Result<()> {
    // Set up DFIR TCP channels
    let (outbound, inbound) = connect_tcp_bytes();
    // Channels for bridging WebSocket <-> DFIR
    let (ws_to_dfir_tx, ws_to_dfir_rx) = mpsc::channel::<Message>(16);
    let (dfir_to_ws_tx, mut dfir_to_ws_rx) = mpsc::channel::<Message>(16);
    // Split the WebSocket stream for concurrent read/write
    let (mut ws_tx, mut ws_rx) = ws.split();
    // Spawn the DFIR pipeline (runs on current thread)
    let mut hf: Dfir = {
        let dfir_to_ws_tx_ref = &dfir_to_ws_tx;
        dfir_syntax! {
            outbound_chan = dest_sink(outbound);
            inbound_chan = source_stream_serde(inbound);
            ws_source = source_stream(ReceiverStream::new(ws_to_dfir_rx).map(Ok::<_, ()>))
                -> filter_map(|msg| {
                    if let Ok(ref msg) = msg {
                        // Serialize to bincode
                        match bincode::serialize(&msg) {
                            Ok(mut bytes) => {
                                // Add length prefix (u32 LE)
                                let len = bytes.len() as u32;
                                let mut framed = len.to_le_bytes().to_vec();
                                framed.append(&mut bytes);
                                // tracing::info!("Proxy outbound: sending to server: {:?} bytes={:02x?}", msg, framed);
                                Some((Bytes::from(framed), server_addr))
                            }
                            Err(e) => {
                                tracing::error!("Failed to serialize message for TCP: {}", e);
                                None
                            }
                        }
                    } else {
                        None
                    }
                })
                -> outbound_chan;
            inbound_chan
                -> for_each(move |res| {
                    if let Ok((msg, _addr)) = res {
                        let _ = dfir_to_ws_tx_ref.try_send(msg);
                    }
                });
        }
    };
    // WebSocket -> DFIR: receive WebSocket messages, deserialize, send to ws_to_dfir_tx
    let ws_to_dfir_task = async move {
        while let Some(msg_result) = ws_rx.next().await {
            match msg_result {
                Ok(msg) => {
                    if msg.is_binary() || msg.is_text() {
                        let data = msg.clone().into_data();
                        // tracing::info!("Raw WebSocket message bytes: {:02x?}", data);
                        match bincode::deserialize::<Message>(&data) {
                            Ok(message) => {
                                // tracing::info!("Deserialized WebSocket message as: {:?}", message);
                                if let Err(e) = ws_to_dfir_tx.send(message).await {
                                    tracing::error!("Failed to send to DFIR pipeline: {}", e);
                                    break;
                                }
                            }
                            Err(e) => {
                                tracing::error!("Failed to deserialize WebSocket message: {}", e);
                            }
                        }
                    } else if msg.is_close() {
                        break;
                    } else {
                        tracing::warn!("Received unexpected WebSocket message type: {:?}", msg);
                    }
                }
                Err(e) => {
                    tracing::error!("WebSocket receive error: {}", e);
                    break;
                }
            }
        }
    };
    // DFIR -> WebSocket: receive messages from dfir_to_ws_rx, serialize, send to WebSocket
    let dfir_to_ws_task = async move {
        while let Some(message) = dfir_to_ws_rx.recv().await {
            match bincode::serialize(&message) {
                Ok(data) => {
                    if let Err(e) = ws_tx.send(WsMessage::binary(data)).await {
                        tracing::error!("Failed to send message to WebSocket: {}", e);
                        break;
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to serialize message for WebSocket: {}", e);
                }
            }
        }
    };
    // Run all three tasks concurrently on the current thread
    join3(
        async {
            hf.run_async().await.unwrap();
        },
        ws_to_dfir_task,
        dfir_to_ws_task,
    )
    .await;
    Ok(())
}
