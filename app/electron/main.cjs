// Desktop shell: serves the built app, lets it talk to JMK keyboards over
// USB, and copies firmware onto a bootloader drive.
const { app, BrowserWindow, ipcMain, nativeTheme, net, protocol, session } = require('electron')
const fs = require('node:fs')
const os = require('node:os')
const path = require('node:path')
const { pathToFileURL } = require('node:url')
const { watchUpdates } = require('./updater.cjs')

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

/** Whether a path can be reached, asked without holding up the window. */
const reachable = (file) => fs.promises.access(file).then(() => true, () => false)

/**
 * Folders where a UF2 bootloader drive is mounted. Nothing here may use the
 * blocking file calls: macOS stops the call while it asks the user whether
 * the app may read removable drives, and the whole window would hang.
 */
async function mountedDrives() {
  const user = os.userInfo().username
  const parents =
    process.platform === 'darwin'
      ? ['/Volumes']
      : process.platform === 'win32'
        ? []
        : [`/media/${user}`, `/run/media/${user}`]
  const listed = await Promise.all(
    parents.map((parent) => fs.promises.readdir(parent).then((names) => names.map((name) => path.join(parent, name)), () => [])),
  )
  const drives = listed.flat()
  if (process.platform === 'win32') {
    for (const letter of 'DEFGHIJKLMNOPQRSTUVWXYZ') drives.push(`${letter}:\\`)
  }
  // Every UF2 bootloader puts this file on its drive.
  const found = await Promise.all(drives.map((drive) => reachable(path.join(drive, 'INFO_UF2.TXT'))))
  return drives.filter((_drive, index) => found[index])
}

/** Boards waiting in update mode: the drive and what it says about itself. */
async function bootDrives() {
  const drives = await mountedDrives()
  return Promise.all(
    drives.map(async (drive) => {
      // Unreadable for a moment while the drive mounts.
      const info = await fs.promises.readFile(path.join(drive, 'INFO_UF2.TXT'), 'utf8').catch(() => '')
      const field = (name) => (info.match(new RegExp(`^${name}:\\s*(.+)$`, 'm')) || [])[1]?.trim() || ''
      const id = field('Board-ID')
      const model = field('Model')
      const says = id + ' ' + model
      // An RP2350 calls itself that; an RP2040 says "RPI-RP2" and "Raspberry Pi RP2".
      const mcu = /RP2350/i.test(says) ? 'rp2350' : /RP2040|RPI-RP2\b/i.test(says) ? 'rp2040' : /nRF52840/i.test(says) ? 'nrf52840' : ''
      return { path: drive, model: model || id || path.basename(drive), mcu }
    }),
  )
}

const firmwareDir = () => (app.isPackaged ? path.join(process.resourcesPath, 'firmware') : path.join(__dirname, '..', 'firmware'))

/** The chips there is a firmware file for. */
function bundled() {
  if (!fs.existsSync(firmwareDir())) return []
  return fs.readdirSync(firmwareDir()).flatMap((name) => /^jmk-([a-z0-9]+)\.uf2$/.exec(name)?.[1] ?? [])
}

async function flash(_event, mcu, onto) {
  if (!/^[a-z0-9]+$/.test(mcu)) throw new Error('Unknown chip.')
  const image = path.join(firmwareDir(), `jmk-${mcu}.uf2`)
  if (!fs.existsSync(image)) throw new Error(`This app does not include firmware for ${mcu}.`)
  const deadline = Date.now() + 20_000
  for (;;) {
    // `onto` names one drive when several boards are in update mode.
    const drive = (await mountedDrives()).find((candidate) => !onto || candidate === onto)
    if (drive) {
      // The board restarts the moment the last block arrives, which can
      // make the copy report an error although it succeeded.
      // Plain bytes, not copyFile: that also copies the file's attributes,
      // which the small file system of a bootloader drive refuses.
      const data = await fs.promises.readFile(image)
      await fs.promises.writeFile(path.join(drive, path.basename(image)), data).catch(async (error) => {
        if (!(await reachable(drive))) return
        // macOS asks once whether an app may use removable drives; a "no" (or
        // an app started from a program that has none) ends up here.
        if (process.platform === 'darwin' && (error.code === 'EPERM' || error.code === 'EACCES')) {
          throw new Error(
            'macOS does not let this app write to the update drive. Allow it under System Settings › Privacy & Security › Files and Folders › Removable Volumes, then try again.',
          )
        }
        throw error
      })
      return drive
    }
    if (Date.now() > deadline) throw new Error('The update drive did not appear. Open it in your file manager once, then try again.')
    await new Promise((resolve) => setTimeout(resolve, 400))
  }
}

// The page draws its own title bar (see App.vue); the system only adds its
// window buttons on top. Keep the height in step with `.titlebar` there.
const TITLEBAR_HEIGHT = 40
const overlayColors = () =>
  nativeTheme.shouldUseDarkColors ? { color: '#1f1f1f', symbolColor: '#f0ece6' } : { color: '#ffffff', symbolColor: '#1d2026' }

function titleBar() {
  if (process.platform === 'darwin') return { titleBarStyle: 'hiddenInset', trafficLightPosition: { x: 14, y: 13 } }
  return { titleBarStyle: 'hidden', titleBarOverlay: { ...overlayColors(), height: TITLEBAR_HEIGHT } }
}

// The app is POG; JMK is the firmware it sets up.
app.setName('POG')

// A build run from the source tree can be inspected while it runs: point a
// browser's DevTools, or any CDP client, at http://127.0.0.1:9333 (another
// port with POG_CDP_PORT, none with POG_CDP_PORT=0). Released apps never
// open it.
if (!app.isPackaged && process.env.POG_CDP_PORT !== '0') {
  app.commandLine.appendSwitch('remote-debugging-port', process.env.POG_CDP_PORT || '9333')
}

// Started with `npm start` the app would carry Electron's own icon; a packaged
// app gets its icon from the installer.
const devIcon = app.isPackaged ? undefined : path.join(__dirname, '..', 'build', process.platform === 'darwin' ? 'icon.png' : 'icon-full.png')

app.whenReady().then(() => {
  if (devIcon && process.platform === 'darwin') app.dock?.setIcon(devIcon)
  serveApp()
  allowKeyboards()
  ipcMain.handle('jmk:flash', flash)
  ipcMain.handle('jmk:drives', bootDrives)
  ipcMain.handle('jmk:bundled', bundled)
  const window = new BrowserWindow({
    width: 1280,
    height: 880,
    minWidth: 900,
    minHeight: 640,
    backgroundColor: nativeTheme.shouldUseDarkColors ? '#171717' : '#f3f1ec',
    autoHideMenuBar: true,
    ...(devIcon ? { icon: devIcon } : {}),
    ...titleBar(),
    webPreferences: { preload: path.join(__dirname, 'preload.cjs'), contextIsolation: true, nodeIntegration: false },
  })
  if (process.platform !== 'darwin') {
    nativeTheme.on('updated', () => !window.isDestroyed() && window.setTitleBarOverlay(overlayColors()))
  }
  watchUpdates(window)
  window.loadURL(`${ORIGIN}/index.html`)
})

app.on('window-all-closed', () => app.quit())
