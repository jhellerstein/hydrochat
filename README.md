# HydroChat

A real-time chat application built with [Hydro](https://github.com/hydro-project/hydro), featuring a Rust backend server, custom WebSocket proxy, and Electron client with WASM.

## Architecture

This project is a Cargo workspace with four main crates:

1. **hydrochat-core**: Shared protocol and types (WASM-safe)
2. **hydrochat-server**: Hydro-based TCP chat server (port 3001)
3. **hydrochat-proxy**: Custom WebSocket-to-TCP proxy using Hydro DFIR (port 8080)
4. **hydrochat-wasm**: WASM chat client library (used by Electron)

The Electron app in `electron/` uses the WASM output from `hydrochat-wasm` and connects through the proxy to the server.

## Prerequisites

- Rust nightly (specifically `nightly-2025-04-27` as specified in `rust-toolchain.toml`)
- Node.js and npm
- wasm-pack (for building WASM)
- wasm32-unknown-unknown target

### Installing Prerequisites

1. **Install Rust and Cargo:**
   ```bash
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   ```

2. **The project uses a specific nightly toolchain** which will be automatically installed when you first run `cargo` commands in this directory.

3. **Install wasm-pack:**
   ```bash
   cargo install wasm-pack
   ```

4. **Add WASM target:**
   ```bash
   rustup target add wasm32-unknown-unknown
   ```

5. **Install Node.js and npm:**
   Download from [nodejs.org](https://nodejs.org/) or use your package manager.

## Quick Start

The easiest way to run the entire application:

```bash
./run.sh
```

This script will:
- Build all Rust workspace binaries
- Build the WASM package for Electron using wasm-pack
- Install Electron dependencies if needed
- Start the chat server (port 3001)
- Start the WebSocket proxy (port 8080)
- Launch the Electron client

## Cleaning Build Artifacts

To clean all build artifacts and start fresh:

```bash
./clean.sh
```

This will remove:
- Rust build outputs (`target/`)
- Node.js dependencies (`electron/node_modules/`)
- WASM generated files (`electron/pkg/`)
- Log files and system files

## Manual Setup & Build

If you prefer to run components manually:

1. **Install Node.js dependencies:**
   ```bash
   cd electron
   npm install
   cd ..
   ```

2. **Build all Rust workspace components:**
   ```bash
   cargo build
   ```

3. **Build the WASM for Electron:**
   ```bash
   cd hydrochat-wasm
   wasm-pack build --target web --out-dir ../electron/pkg
   cd ..
   ```

4. **Start the chat server:**
   ```bash
   cargo run -p hydrochat-server
   ```

5. **Start the WebSocket proxy:**
   ```bash
   cargo run -p hydrochat-proxy
   ```

6. **Start the Electron client:**
   ```bash
   cd electron
   npm start
   ```

## Usage

1. Run `./run.sh` or start all components manually
2. The Electron app will open automatically
3. Enter a nickname and click "Connect"
4. Start chatting! Messages will be broadcast to all connected clients

## Project Structure

```
hydrochat/
├── hydrochat-core/        # Shared protocol and types
├── hydrochat-server/      # Hydro-based TCP chat server
├── hydrochat-proxy/       # WebSocket-to-TCP proxy using Hydro DFIR
├── hydrochat-wasm/        # WASM chat client library
├── electron/              # Electron desktop app
│   ├── main.js           # Electron main process
│   ├── index.html        # Chat UI with embedded JavaScript
│   ├── package.json      # Node.js dependencies
│   └── pkg/              # Generated WASM files (from wasm-pack)
├── run.sh                # Start all components
├── clean.sh              # Clean all build artifacts
└── rust-toolchain.toml   # Specifies required Rust nightly version
```

## Development

- Shared protocol/types: `hydrochat-core/`
- Server logic: `hydrochat-server/` (using Hydro)
- Proxy logic: `hydrochat-proxy/` (custom implementation with Hydro DFIR)
- WASM client: `hydrochat-wasm/`
- Electron UI: `electron/index.html`

## Troubleshooting

- **Port conflicts**: Make sure ports 3001 (server) and 8080 (proxy) are available
- **WASM build issues**: Ensure `wasm-pack` is installed and the `wasm32-unknown-unknown` target is added
- **Rust toolchain**: The project requires nightly-2025-04-27, which should be automatically used via `rust-toolchain.toml`
- **Connection issues**: Verify all three components (server, proxy, electron) are running
- **Build errors**: Try running `./clean.sh` followed by `./run.sh` for a fresh build
