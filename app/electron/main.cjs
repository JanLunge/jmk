// Desktop shell: serves the built app, lets it talk to JMK keyboards over
// USB, and copies firmware onto a bootloader drive.
const { app, BrowserWindow, ipcMain, net, protocol, session } = require('electron')
const fs = require('node:fs')
const os = require('node:os')
const path = require('node:path')
const { pathToFileURL } = require('node:url')

const DIST = path.join(__dirname, '..', 'dist')
const ORIGIN = 'app://jmk'
// USB IDs of JMK firmware. These are the pid.codes test IDs.
const KEYBOARDS = [{ vendorId: 0x1209, productId: 0x0001 }]

const isKeyboard = (device) => KEYBOARDS.some((k) => k.vendorId === device.vendorId && k.productId === device.productId)

protocol.registerSchemesAsPrivileged([
  { scheme: 'app', privileges: { standard: true, secure: true, supportFetchAPI: true } },
])

function serveApp() {
  protocol.handle('app', (request) => {
    const relative = decodeURIComponent(new URL(request.url).pathname)
    const file = path.normalize(path.join(DIST, relative === '/' ? 'index.html' : relative))
    if (!file.startsWith(DIST + path.sep)) return new Response('Not found', { status: 404 })
    return net.fetch(pathToFileURL(file).toString())
  })
}

function allowKeyboards() {
  const ses = session.defaultSession
  // Devices already handed to the page, so "Connect" offers the next one
  // when two halves are plugged in.
  const granted = new Set()
  ses.on('select-hid-device', (event, details, callback) => {
    event.preventDefault()
    const choice = details.deviceList.filter(isKeyboard).find((device) => !granted.has(device.deviceId))
    if (choice) granted.add(choice.deviceId)
    callback(choice ? choice.deviceId : '')
  })
  ses.on('hid-device-removed', (_event, details) => granted.delete(details.device.deviceId))
  ses.setPermissionCheckHandler((_contents, permission, origin) => permission === 'hid' && origin.startsWith(ORIGIN))
  ses.setDevicePermissionHandler((details) => details.deviceType === 'hid' && isKeyboard(details.device))
}

/** Folders where a UF2 bootloader drive may be mounted. */
function mountedDrives() {
  const user = os.userInfo().username
  const parents =
    process.platform === 'darwin'
      ? ['/Volumes']
      : process.platform === 'win32'
        ? []
        : [`/media/${user}`, `/run/media/${user}`]
  const drives = parents.flatMap((parent) => {
    try {
      return fs.readdirSync(parent).map((name) => path.join(parent, name))
    } catch {
      return []
    }
  })
  if (process.platform === 'win32') {
    for (const letter of 'DEFGHIJKLMNOPQRSTUVWXYZ') drives.push(`${letter}:\\`)
  }
  // Every UF2 bootloader puts this file on its drive.
  return drives.filter((drive) => fs.existsSync(path.join(drive, 'INFO_UF2.TXT')))
}

async function flash(_event, mcu) {
  if (!/^[a-z0-9]+$/.test(mcu)) throw new Error('Unknown chip.')
  const bundled = app.isPackaged ? path.join(process.resourcesPath, 'firmware') : path.join(__dirname, '..', 'firmware')
  const image = path.join(bundled, `jmk-${mcu}.uf2`)
  if (!fs.existsSync(image)) throw new Error(`This app does not include firmware for ${mcu}.`)
  const deadline = Date.now() + 20_000
  for (;;) {
    const [drive] = mountedDrives()
    if (drive) {
      // The board restarts the moment the last block arrives, which can
      // make the copy report an error although it succeeded.
      await fs.promises.copyFile(image, path.join(drive, path.basename(image))).catch((error) => {
        if (fs.existsSync(drive)) throw error
      })
      return drive
    }
    if (Date.now() > deadline) throw new Error('The update drive did not appear. Open it in your file manager once, then try again.')
    await new Promise((resolve) => setTimeout(resolve, 400))
  }
}

app.whenReady().then(() => {
  serveApp()
  allowKeyboards()
  ipcMain.handle('jmk:flash', flash)
  const window = new BrowserWindow({
    width: 1280,
    height: 880,
    minWidth: 900,
    minHeight: 640,
    backgroundColor: '#0e1014',
    autoHideMenuBar: true,
    webPreferences: { preload: path.join(__dirname, 'preload.cjs'), contextIsolation: true, nodeIntegration: false },
  })
  window.loadURL(`${ORIGIN}/index.html`)
})

app.on('window-all-closed', () => app.quit())
