const { app, BrowserWindow } = require('electron');
const path = require('path');
const { spawn } = require('child_process');
const fs = require('fs');

let proxyProcess = null;

function startProxy() {
  // Path to the built Rust proxy server binary
  const bin = path.join(__dirname, 'proxy_server');
  
  // Check if binary exists
  if (!fs.existsSync(bin)) {
    console.error(`Proxy server binary not found: ${bin}`);
    return;
  }
  
  console.log(`Starting proxy server: ${bin}`);
  proxyProcess = spawn(bin, [], { 
    stdio: ['pipe', 'pipe', 'pipe'],
    detached: false
  });
  
  // Capture stdout and stderr
  proxyProcess.stdout.on('data', (data) => {
    console.log(`Proxy server: ${data.toString().trim()}`);
  });
  
  proxyProcess.stderr.on('data', (data) => {
    console.error(`Proxy server error: ${data.toString().trim()}`);
  });
  
  proxyProcess.on('error', (err) => {
    console.error(`Proxy server error: ${err}`);
  });
  
  proxyProcess.on('exit', (code, signal) => {
    console.log(`Proxy server exited with code: ${code}, signal: ${signal}`);
    if (code !== 0 && code !== null) {
      console.error('Proxy server crashed unexpectedly');
    }
  });
  
  // Give the proxy server a moment to start
  setTimeout(() => {
    console.log('Proxy server should be ready on port 8080');
  }, 1000);
}

function stopProxy() {
  if (proxyProcess) {
    console.log('Stopping proxy server...');
    proxyProcess.kill();
    proxyProcess = null;
  }
}

function createWindow() {
  const win = new BrowserWindow({
    width: 600,
    height: 800,
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
    },
  });
  win.loadFile('index.html');
  
  // Open DevTools for debugging (commented out to not show by default)
  // win.webContents.openDevTools();
}

app.whenReady().then(() => {
  // startProxy(); // Proxy is now started by Rust, not Electron
  
  // Give proxy time to start before creating window
  setTimeout(() => {
    createWindow();
  }, 2000);

  app.on('activate', function () {
    if (BrowserWindow.getAllWindows().length === 0) createWindow();
  });
});

app.on('window-all-closed', () => {
  // stopProxy(); // Proxy is now managed by Rust
  if (process.platform !== 'darwin') app.quit();
});

app.on('before-quit', () => {/* stopProxy(); */}); // Proxy is now managed by Rust 