// The only things the page can ask the desktop shell to do.
const { contextBridge, ipcRenderer } = require('electron')

contextBridge.exposeInMainWorld('jmkDesktop', {
  platform: process.platform,
  flash: (mcu, onto) => ipcRenderer.invoke('jmk:flash', mcu, onto),
  drives: () => ipcRenderer.invoke('jmk:drives'),
  bundled: () => ipcRenderer.invoke('jmk:bundled'),
})
