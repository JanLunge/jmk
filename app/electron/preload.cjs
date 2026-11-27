// The only things the page can ask the desktop shell to do.
const { contextBridge, ipcRenderer } = require('electron')

contextBridge.exposeInMainWorld('jmkDesktop', {
  platform: process.platform,
  flash: (mcu, onto) => ipcRenderer.invoke('jmk:flash', mcu, onto),
  drives: () => ipcRenderer.invoke('jmk:drives'),
  bundled: () => ipcRenderer.invoke('jmk:bundled'),
  access: () => ipcRenderer.invoke('jmk:access'),
  allowAccess: () => ipcRenderer.invoke('jmk:allow-access'),
  update: {
    state: () => ipcRenderer.invoke('jmk:update-state'),
    check: () => ipcRenderer.invoke('jmk:update-check'),
    install: () => ipcRenderer.invoke('jmk:update-install'),
    watch: (listener) => ipcRenderer.on('jmk:update', (_event, state) => listener(state)),
  },
})
