// Updates of the app itself: looks at the latest GitHub release, downloads
// a newer one in the background, and installs it when the page says so.
const { app, ipcMain } = require('electron')

/**
 * What the page is shown. `kind` is one of:
 * off (this build cannot update itself), idle, checking, current,
 * downloading, ready (restart installs it), failed.
 */
let state = { kind: 'off', version: app.getVersion() }

/** An AppImage is the only Linux package that can replace itself. */
const supported = () => app.isPackaged && (process.platform !== 'linux' || !!process.env.APPIMAGE)

function watchUpdates(window) {
  const tell = (next) => {
    state = { version: app.getVersion(), ...next }
    if (!window.isDestroyed()) window.webContents.send('jmk:update', state)
  }
  ipcMain.handle('jmk:update-state', () => state)
  if (!supported()) {
    ipcMain.handle('jmk:update-check', () => state)
    ipcMain.handle('jmk:update-install', () => {})
    return
  }
  const { autoUpdater } = require('electron-updater')
  autoUpdater.autoDownload = true
  // A downloaded update also goes in when the app is closed the normal way.
  autoUpdater.autoInstallOnAppQuit = true
  autoUpdater.on('checking-for-update', () => tell({ kind: 'checking' }))
  autoUpdater.on('update-not-available', () => tell({ kind: 'current' }))
  autoUpdater.on('update-available', (info) => tell({ kind: 'downloading', next: info.version, progress: 0 }))
  autoUpdater.on('download-progress', (progress) => tell({ kind: 'downloading', next: state.next, progress: progress.percent / 100 }))
  autoUpdater.on('update-downloaded', (info) => tell({ kind: 'ready', next: info.version }))
  autoUpdater.on('error', (error) => tell({ kind: 'failed', message: String(error?.message ?? error).split('\n')[0] }))

  const check = async () => {
    if (state.kind === 'checking' || state.kind === 'downloading' || state.kind === 'ready') return state
    // The outcome arrives through the events above; a rejection is the same error again.
    await autoUpdater.checkForUpdates().catch(() => {})
    return state
  }
  ipcMain.handle('jmk:update-check', check)
  ipcMain.handle('jmk:update-install', () => {
    if (state.kind === 'ready') autoUpdater.quitAndInstall(true, true)
  })
  state = { kind: 'idle', version: app.getVersion() }
  check()
  // An app left open for days still hears about a release.
  setInterval(check, 6 * 60 * 60 * 1000).unref()
}

module.exports = { watchUpdates }
