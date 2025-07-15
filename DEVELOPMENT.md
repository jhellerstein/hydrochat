# HydroChat Development Guide (Workspace Edition)

## Project Structure

```
hydrochat/
├── hydrochat-core/        # Shared protocol and types (WASM-safe)
│   ├── src/lib.rs
│   └── src/protocol.rs
├── hydrochat-server/      # TCP chat server
│   └── src/main.rs
├── hydrochat-wasm/        # WASM chat client library
│   └── src/lib.rs
├── electron/              # Electron app
│   ├── main.js
│   ├── index.html
│   ├── package.json
│   └── pkg/               # WASM output from hydrochat-wasm
├── Cargo.toml             # Workspace root
├── run.sh                 # Convenience script to run all components
└── README.md              # Main documentation

Note: WebSocket-to-TCP proxy is provided by hydro_proxy crate from the hydro repo
```

## Architecture Overview

### 1. Protocol Layer (`hydrochat-core/src/protocol.rs`)
Defines the shared message format used between all components:
- `ChatMsg`: Contains nickname, message, and timestamp
- `ConnectResponse`: Server confirmation message

### 2. Server (`hydrochat-server/src/main.rs`)
- TCP server listening on port 3001
- Handles client connections and message broadcasting
- Sends connection confirmations to new clients
- Broadcasts chat messages to all connected clients

### 3. Proxy (`hydro_proxy` from hydro repo)
- WebSocket server on port 8080
- Bridges WebSocket connections from the browser to TCP server
- Handles binary message forwarding between WebSocket and TCP
- Generic, reusable proxy server for Hydro applications

### 4. WASM Client (`hydrochat-wasm/src/lib.rs` + `electron/`)
- WASM-powered chat client
- WebSocket connection to proxy
- Message serialization/deserialization
- Electron UI for user interaction

## Development Workflow

### Adding New Features

1. **Protocol Changes**: Update `hydrochat-core/src/protocol.rs` first
2. **Server Logic**: Modify `hydrochat-server/src/main.rs`
3. **Proxy Logic**: Modify `hydro_proxy` crate in the hydro repo (if needed)
4. **Client Logic**: Update `hydrochat-wasm/src/lib.rs` for WASM functionality
5. **UI Changes**: Modify `electron/index.html`

### Testing

```bash
# Run all tests
cargo test --workspace

# Build all components
cargo build --workspace

# Build WASM for client
cd hydrochat-wasm
wasm-pack build --target web --out-dir ../electron/pkg
cd ..
```

### Debugging

1. **Server Logs**: Check server output for connection and message logs
2. **Proxy Logs**: Monitor proxy for WebSocket connection issues (proxy runs from hydro repo)
3. **Client Logs**: Use Electron DevTools (F12) to see JavaScript/WASM logs
4. **Network**: Use browser DevTools Network tab to monitor WebSocket traffic

## Common Issues

### WASM Build Issues
- Ensure `wasm-pack` is installed: `cargo install wasm-pack`
- Add WASM target: `rustup target add wasm32-unknown-unknown`
- Rebuild: `cd hydrochat-wasm && wasm-pack build --target web --out-dir ../electron/pkg`

### Port Conflicts
- Server uses port 3001
- Proxy uses port 8080 (runs from hydro repo)
- Check if ports are available: `lsof -i :3001` or `lsof -i :8080`

### Electron Issues
- Ensure Node.js dependencies are installed: `cd electron && npm install`
- Check Electron version compatibility
- Use DevTools for debugging JavaScript/WASM interactions

## Extending the Application

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