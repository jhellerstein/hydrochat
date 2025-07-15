#![cfg(not(target_arch = "wasm32"))]

use dfir_rs::dfir_syntax;
use dfir_rs::scheduled::graph::Dfir;
use dfir_rs::util::bind_tcp_bytes;
use std::net::SocketAddr;

use hydrochat_core::protocol::{Message, MessageWithAddr};

pub fn default_server_address() -> SocketAddr {
    "127.0.0.1:3001".parse().unwrap()
}

pub async fn run_server(address: SocketAddr) -> anyhow::Result<()> {
    println!("Starting server on {:?}", address);

    let (outbound, inbound, actual_server_addr) = bind_tcp_bytes(address).await;

    println!("Server is live! Listening on {:?}", actual_server_addr);
    // Note: bind_tcp_bytes does not expose connection events directly, but we can log here to indicate readiness.
    // If you want to log each new connection, you would need to modify the hydro/dfir_rs/src/util/tcp.rs code to add a callback or log on accept.

    let mut hf: Dfir = dfir_syntax! {
        // Define shared inbound and outbound channels
        outbound_chan = union() -> dest_sink_serde(outbound);
        inbound_chan = source_stream(inbound)
    -> filter_map(|res| {
        match res {
            Ok((bytes, addr)) => {
                if bytes.len() < 4 {
                    eprintln!("Received message too short for length prefix: {:?}", bytes);
                    None
                } else {
                    let msg_bytes = &bytes[4..];
                    match bincode::deserialize::<Message>(msg_bytes) {
                        Ok(msg) => {
                            Some((msg, addr))
                        }
                        Err(e) => {
                            eprintln!("Deserialization error: {:?}", e);
                            None
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("[LOG] Error in stream: {:?}", e);
                None
            }
        }
    })
    -> map(|(msg, addr)| {
        let msg_with_addr = MessageWithAddr::from_message(msg, addr);
        msg_with_addr
    })
    -> demux_enum::<MessageWithAddr>();
        clients = inbound_chan[ConnectRequest]
            -> map(|(addr,)| addr)
            -> tee();
        inbound_chan[ConnectResponse] -> for_each(|(addr,)| println!("Received unexpected `ConnectResponse` as server from addr {}.", addr));

        // Pipeline 1: Acknowledge client connections
        clients[0]
            -> map(|addr| (Message::ConnectResponse, addr))
            -> [0]outbound_chan;

        // Pipeline 2: Broadcast messages to all clients
        inbound_chan[ChatMsg]
            -> map(|(_addr, nickname, message /*, ts */)| Message::ChatMsg { nickname, message /*, ts */ })
            -> [0]broadcast;
        clients[1]
            -> [1]broadcast;
        broadcast = cross_join::<'tick, 'static>()
            -> [1]outbound_chan;
    };

    // println!("DFIR pipeline constructed, starting execution...");
    // println!("About to run DFIR pipeline...");
    hf.run_async().await.unwrap();
    // println!("DFIR pipeline finished (this shouldn't happen normally)");
    Ok(())
}

fn main() -> anyhow::Result<()> {
    let addr = default_server_address();
    println!("Starting chat server on {}", addr);

    let local = tokio::task::LocalSet::new();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    local.block_on(&rt, run_server(addr))
}
