# HydroChat (Workspace Edition)

A real-time chat application built with [Hydro](https://github.com/hydro-project/hydro), featuring a Rust backend server, WebSocket proxy, and Electron client with WASM.

## Architecture

This project is a Cargo workspace with three main crates:

1. **hydrochat-core**: Shared protocol and types (WASM-safe)
2. **hydrochat-server**: Rust TCP chat server (port 3001)
3. **hydrochat-wasm**: WASM chat client library (used by Electron)

The WebSocket-to-TCP proxy (port 8080) is provided by the `hydro_proxy` crate from the [hydro repo](https://github.com/hydro-project/hydro).

The Electron app in `electron/` uses the WASM output from `hydrochat-wasm`.

## Prerequisites

- Rust (latest stable)
- Node.js and npm
- wasm-pack (for building WASM)
- wasm32-unknown-unknown target

### Installing Prerequisites

1. **Install Rust and Cargo:**
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```
2. **Install wasm-pack:**
   ```bash
   cargo install wasm-pack
   ```
3. **Add WASM target:**
   ```bash
   rustup target add wasm32-unknown-unknown
   ```
4. **Install Node.js and npm:**
   Download from [nodejs.org](https://nodejs.org/) or use your package manager.

## Setup & Build

1. **Install Node.js dependencies:**
   ```bash
   cd electron
   npm install
   cd ..
   ```
2. **Build all Rust binaries and WASM:**
   ```bash
   ./run.sh
   ```
   This script will:
   - Build all Rust workspace binaries
   - Build the WASM package for Electron
   - Install Electron dependencies if needed
   - Start all components (server, proxy, electron)

## Manual Running (Advanced)

1. **Start the chat server:**
   ```bash
   cargo run -p hydrochat-server
   ```
2. **Start the WebSocket proxy:**
   ```bash
   cargo run --manifest-path ../../hydro/hydro_proxy/Cargo.toml --example simple_proxy 127.0.0.1:3001 8080
   ```
3. **Build the WASM for Electron:**
   ```bash
   cd hydrochat-wasm
   wasm-pack build --target web --out-dir ../electron/pkg
   cd ..
   ```
4. **Start the Electron client:**
   ```bash
   cd electron
   npm start
   ```

## Usage

1. Open the Electron app
2. Enter a nickname and click "Connect"
3. Start chatting! Messages will be broadcast to all connected clients

## Development

- Shared protocol/types: `hydrochat-core/`
- Server logic: `hydrochat-server/`
- Proxy logic: `hydro_proxy` (from hydro repo)
- WASM client: `hydrochat-wasm/`
- Electron UI: `electron/index.html`

## Troubleshooting

- Make sure all three components (server, proxy, electron) are running
- Check that ports 3001 (server) and 8080 (proxy) are available
- Ensure WASM is built correctly with `wasm-pack build --target web --out-dir ../electron/pkg` from `hydrochat-wasm/`
