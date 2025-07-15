#!/bin/bash

# HydroChat Clean Script
# This script removes all build artifacts and dependencies that can be rebuilt

set -e

echo "🧹 Cleaning HydroChat build artifacts..."

# Clean Rust build artifacts
echo "🦀 Cleaning Rust artifacts..."
cargo clean

# Clean Node.js dependencies and lock files
echo "📦 Cleaning Node.js artifacts..."
rm -rf electron/node_modules
rm -f electron/package-lock.json

# Clean WASM generated files (from wasm-pack)
echo "🌐 Cleaning WASM generated files..."
rm -rf electron/pkg

# Clean any potential Electron build outputs
echo "⚡ Cleaning Electron build outputs..."
rm -rf electron/dist
rm -rf electron/out

# Clean logs and temporary files
echo "📝 Cleaning logs and temporary files..."
find . -name "*.log" -type f -delete 2>/dev/null || true
rm -rf logs/ 2>/dev/null || true

# Clean macOS system files
echo "🍎 Cleaning macOS system files..."
find . -name ".DS_Store" -type f -delete 2>/dev/null || true

echo "✅ All build artifacts cleaned!"
echo "💡 Run './run.sh' to rebuild and start the application"
