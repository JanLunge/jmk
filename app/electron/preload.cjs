// The only things the page can ask the desktop shell to do.
const { contextBridge, ipcRenderer } = require('electron')

contextBridge.exposeInMainWorld('jmkDesktop', {
  flash: (mcu) => ipcRenderer.invoke('jmk:flash', mcu),
})
