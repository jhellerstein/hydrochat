#!/bin/bash

# HydroChat Runner Script (Workspace Edition)
# This script helps you run all components of the HydroChat application

set -e

echo "🚀 HydroChat - Starting all components..."

cleanup() {
    echo "🛑 Shutting down HydroChat..."
    kill $SERVER_PID $PROXY_PID 2>/dev/null || true
    exit 0
}

trap cleanup SIGINT SIGTERM

# Build all Rust binaries
echo "🔨 Building Rust workspace..."
cargo build

# Build WASM for Electron
echo "🔨 Building WASM for Electron..."
cd hydrochat-wasm
wasm-pack build --target web --out-dir ../electron/pkg
cd ..

# Check if Electron dependencies are installed
if [ ! -d "electron/node_modules" ]; then
    echo "📦 Installing Electron dependencies..."
    cd electron
    npm install
    cd ..
fi

echo "✅ All dependencies ready!"

# Start server in background
echo "🖥️  Starting chat server on port 3001..."
cargo run -p hydrochat-server &
SERVER_PID=$!

sleep 2

# Start the proxy server
echo "🌐 Starting WebSocket proxy on port 8080..."
cargo run -p hydrochat-proxy &
PROXY_PID=$!

sleep 2

echo "📱 Starting Electron client..."
cd electron
npm start &
ELECTRON_PID=$!
cd ..

echo "🎉 HydroChat is running!"
echo "   - Server: localhost:3001"
echo "   - Proxy: localhost:8080"
echo "   - Electron client should open automatically"
echo ""
echo "Press Ctrl+C to stop all components"

wait 