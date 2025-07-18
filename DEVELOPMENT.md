# HydroChat Development Guide

## Project Structure

```
hydrochat/
├── hydrochat-core/        # Shared protocol and types (WASM-safe)
│   ├── src/lib.rs        # ChatClient type and shared utilities
│   └── src/protocol.rs   # Message types and serialization
├── hydrochat-server/      # Hydro-based TCP chat server
│   └── src/main.rs       # Server implementation using Hydro DFIR
├── hydrochat-proxy/       # Custom WebSocket-to-TCP proxy
│   ├── src/main.rs       # Proxy server using Hydro DFIR and tokio-tungstenite
│   └── Cargo.toml        # Dependencies including Hydro DFIR
├── hydrochat-wasm/        # WASM chat client library
│   ├── src/lib.rs        # WebAssembly bindings and client logic
│   └── Cargo.toml        # WASM-specific dependencies
├── electron/              # Electron desktop application
│   ├── main.js           # Electron main process
│   ├── preload.js        # Preload script for security
│   ├── index.html        # Chat UI with embedded JavaScript
│   ├── package.json      # Node.js dependencies
│   └── pkg/              # Generated WASM files (auto-generated by wasm-pack)
├── Cargo.toml             # Workspace configuration
├── rust-toolchain.toml    # Specifies nightly-2025-04-27 requirement
├── build.rs              # Build script for Hydro (stageleft_tool::gen_final!())
├── run.sh                # Convenience script to build and run all components
├── clean.sh              # Script to clean all build artifacts
└── .gitignore            # Comprehensive ignore file for Rust/Node.js/WASM/system files
```

## Architecture Overview

### 1. Protocol Layer (`hydrochat-core/src/protocol.rs`)
Defines the shared message format used between all components:
- `Message`: Core message types (ConnectRequest, ConnectResponse, ChatMsg)
- `MessageWithAddr`: Server-side wrapper that includes socket addresses
- Uses `serde` for serialization and `dfir_macro::DemuxEnum` for Hydro integration

### 2. Server (`hydrochat-server/src/main.rs`)
- Built with Hydro DFIR (Dataflow IR) for high-performance stream processing
- TCP server listening on port 3001
- Handles client connections and message broadcasting using Hydro's dataflow primitives
- Sends connection confirmations to new clients
- Broadcasts chat messages to all connected clients

### 3. Proxy (`hydrochat-proxy/src/main.rs`)
- **Custom implementation** (not from external hydro repo as mentioned in old docs)
- WebSocket server on port 8080 using `tokio-tungstenite`
- Bridges WebSocket connections from Electron to TCP server using Hydro DFIR
- Single-threaded async runtime with LocalSet
- Includes health check endpoint (`/health`)
- Binary message forwarding between WebSocket and TCP protocols

### 4. WASM Client (`hydrochat-wasm/src/lib.rs` + `electron/`)
- WebAssembly-powered chat client with `wasm-bindgen`
- WebSocket connection to proxy via `web-sys`
- Message serialization/deserialization using `bincode`
- Electron provides desktop UI and native integration
- Modern chat UI with bubble styling similar to macOS Messages

## Development Workflow

### Prerequisites
- **Rust nightly**: The project uses `nightly-2025-04-27` (specified in `rust-toolchain.toml`)
- **wasm-pack**: For building WebAssembly packages
- **Node.js/npm**: For Electron dependencies

### Quick Development Cycle

1. **Clean build** (if needed):
   ```bash
   ./clean.sh
   ```

2. **Build and run everything**:
   ```bash
   ./run.sh
   ```

3. **Development hot-reload**: For UI changes, you can edit `electron/index.html` and reload the Electron window.

### Adding New Features

1. **Protocol Changes**: 
   - Update `hydrochat-core/src/protocol.rs` first
   - Add new message types to `Message` enum
   - Update `MessageWithAddr` if server-side addressing is needed

2. **Server Logic**: 
   - Modify `hydrochat-server/src/main.rs`
   - Use Hydro DFIR primitives for dataflow logic
   - Consider message routing and broadcast requirements

3. **Proxy Logic**: 
   - Modify `hydrochat-proxy/src/main.rs` if protocol changes affect WebSocket handling
   - The proxy handles binary message forwarding, so changes here are rare

4. **Client Logic**: 
   - Update `hydrochat-wasm/src/lib.rs` for WASM functionality
   - Handle new message types in JavaScript integration
   - Rebuild WASM: `cd hydrochat-wasm && wasm-pack build --target web --out-dir ../electron/pkg`

5. **UI Changes**: 
   - Modify `electron/index.html` for user interface updates
   - The file contains embedded CSS and JavaScript
   - Chat bubbles use responsive design with proper overflow handling

### Testing

```bash
# Run all tests in workspace
cargo test --workspace

# Build all components
cargo build --workspace

# Build WASM specifically
cd hydrochat-wasm
wasm-pack build --target web --out-dir ../electron/pkg
cd ..

# Test Electron app
cd electron
npm start
```

### Debugging

1. **Server Logs**: 
   - Server uses `tracing` for structured logging
   - Check terminal output for connection and message logs
   - Hydro DFIR provides detailed dataflow execution logs

2. **Proxy Logs**: 
   - Proxy also uses `tracing`
   - Monitor for WebSocket connection issues and TCP forwarding problems
   - Health check: `curl http://localhost:8080/health`

3. **Client Logs**: 
   - Use Electron DevTools (F12) to see JavaScript/WASM logs
   - Console shows WebSocket connection status and message handling
   - Network tab shows WebSocket traffic

4. **WASM Debugging**:
   - Enable debug symbols: `wasm-pack build --dev`
   - Use browser's WebAssembly debugging tools
   - Check for `wasm-bindgen` integration issues

## Common Issues

### Rust Toolchain Issues
- **Solution**: Ensure you're using the correct nightly: `rustup toolchain install nightly-2025-04-27`
- The `rust-toolchain.toml` should handle this automatically

### WASM Build Issues
- **Missing wasm-pack**: `cargo install wasm-pack`
- **Missing target**: `rustup target add wasm32-unknown-unknown`
- **Build failures**: Try `./clean.sh` then rebuild
- **Browser compatibility**: Ensure modern browser with WebAssembly support

### Port Conflicts
- **Server**: Port 3001 (check with `lsof -i :3001`)
- **Proxy**: Port 8080 (check with `lsof -i :8080`)
- **Solution**: Kill conflicting processes or change ports in source code

### Electron Issues
- **Dependencies**: `cd electron && npm install`
- **Version conflicts**: Check Node.js version compatibility
- **DevTools**: Use F12 for JavaScript debugging
- **WASM loading**: Ensure `pkg/` directory exists and contains WASM files

### Connection Issues
- **Startup order**: Server must start before proxy, proxy before client
- **WebSocket errors**: Check browser console for detailed error messages
- **TCP errors**: Verify server is listening on correct port
- **CORS issues**: Proxy handles CORS for WebSocket connections

## Extending the Application

### Adding New Message Types
1. Add variants to `Message` enum in `hydrochat-core/src/protocol.rs`
2. Update server dataflow in `hydrochat-server/src/main.rs`
3. Update WASM client handling in `hydrochat-wasm/src/lib.rs`
4. Update UI in `electron/index.html` to display new message types

### Adding Authentication
1. Extend protocol with auth messages (`LoginRequest`, `LoginResponse`, etc.)
2. Add user management to server using Hydro's stateful operations
3. Implement login UI in Electron
4. Add session management and token validation

### Adding Persistence
1. Add database dependencies to `hydrochat-server/Cargo.toml`
2. Integrate database operations with Hydro dataflow
3. Implement message storage and retrieval
4. Add message history UI in Electron
5. Update WASM client to request and display history

### Adding File Sharing
1. Extend protocol with file transfer messages
2. Implement file upload/download in server
3. Add file handling to proxy (may need protocol changes)
4. Update WASM client for file operations
5. Add drag-and-drop UI in Electron

## Performance Considerations

- **Hydro DFIR**: Provides high-performance dataflow execution with compile-time optimizations
- **Server**: Uses Hydro's efficient broadcast primitives for message distribution
- **Proxy**: Single-threaded async with `tokio` for optimal WebSocket handling
- **WASM**: Minimal serialization overhead with `bincode`
- **Electron**: Modern CSS with hardware acceleration for smooth UI
- **Memory**: Consider message history limits for long-running sessions
- **Network**: Binary WebSocket messages minimize bandwidth usage

## Build System Notes

- **Workspace**: All Rust crates share dependencies via `Cargo.lock`
- **WASM**: Built separately with `wasm-pack` for web target
- **Electron**: Standard Node.js package with npm dependencies
- **Scripts**: `run.sh` handles complete build and startup workflow
- **Cleaning**: `clean.sh` removes all generated artifacts for fresh builds

### Adding New Message Types
1. Add new variants to `Message` enum in `hydrochat-core/src/protocol.rs`
2. Update server handling in `hydrochat-server/src/main.rs`
3. Update client handling in `hydrochat-wasm/src/lib.rs`
4. Update UI to display new message types

### Adding Authentication
1. Extend protocol with auth messages
2. Add user management to server
3. Implement login UI in Electron
4. Add session management

### Adding Persistence
1. Add database dependencies to `hydrochat-server/Cargo.toml`
2. Implement message storage in server
3. Add message history retrieval
4. Update client to request message history

## Performance Considerations

- Server uses broadcast channels for efficient message distribution
- Proxy uses binary WebSocket messages for minimal overhead
- WASM client processes messages asynchronously
- Consider connection pooling for high-traffic scenarios 