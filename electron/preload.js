const { contextBridge } = require('electron');

// Placeholder: you can expose APIs here if you want to use IPC or Node features
contextBridge.exposeInMainWorld('hydro', {
  // Add APIs if needed
}); 