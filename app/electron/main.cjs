// Desktop shell: serves the built app, lets it talk to JMK keyboards over
// USB, and copies firmware onto a bootloader drive.
const { app, BrowserWindow, ipcMain, nativeTheme, net, protocol, session } = require('electron')
const { execFile } = require('node:child_process')
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

// --- Linux: leave to talk to the keyboard ---------------------------------------
// Linux gives a keyboard's settings channel (a hidraw device) to root alone
// until a udev rule says otherwise. The rule comes with the app.
const RULE = path.join(__dirname, '..', 'linux', '70-jmk-keyboard.rules')
const RULE_TARGET = '/etc/udev/rules.d/70-jmk-keyboard.rules'
const APPLY_RULE = 'udevadm control --reload-rules && udevadm trigger --subsystem-match=hidraw'
/** How the settings channel's description starts: the firmware's own usage page. */
const SETTINGS_CHANNEL = Buffer.from([0x06, 0x4a, 0xff])

/** How many JMK keyboards are plugged in that this user may not talk to. */
async function shutOut() {
  const names = await fs.promises.readdir('/sys/class/hidraw').catch(() => [])
  const each = names.map(async (name) => {
    const described = await fs.promises.readFile(`/sys/class/hidraw/${name}/device/report_descriptor`).catch(() => Buffer.alloc(0))
    if (!described.subarray(0, SETTINGS_CHANNEL.length).equals(SETTINGS_CHANNEL)) return false
    return fs.promises.access(`/dev/${name}`, fs.constants.R_OK | fs.constants.W_OK).then(() => false, () => true)
  })
  return (await Promise.all(each)).filter(Boolean).length
}

/** The rule's lines that do something, and what to type in a terminal to install them by hand. */
async function ruleByHand() {
  const lines = (await fs.promises.readFile(RULE, 'utf8')).split('\n').filter((line) => line.trim() && !line.startsWith('#'))
  const quoted = lines.map((line) => `'${line}'`).join(' ')
  return `printf '%s\\n' ${quoted} | sudo tee ${RULE_TARGET} >/dev/null && sudo ${APPLY_RULE.replace('&& ', '&& sudo ')}`
}

/** Whether a keyboard is plugged in that the app is kept from, and the way out by hand. */
async function access() {
  if (process.platform !== 'linux') return { blocked: 0, command: '' }
  return { blocked: await shutOut(), command: await ruleByHand() }
}

/**
 * Install the rule, asking for the password in the system's own dialog.
 * Resolves to '' when it worked, 'dismissed' when the user closed the
 * dialog, and otherwise to why it could not be done.
 */
async function allowAccess() {
  if (process.platform !== 'linux') return ''
  // Root cannot read inside an AppImage, so it gets a copy.
  const dir = await fs.promises.mkdtemp(path.join(os.tmpdir(), 'pog-'))
  const copy = path.join(dir, path.basename(RULE_TARGET))
  try {
    await fs.promises.writeFile(copy, await fs.promises.readFile(RULE))
    const script = `install -m 0644 "$1" ${RULE_TARGET} && ${APPLY_RULE} && udevadm settle --timeout=5`
    return await new Promise((resolve) => {
      execFile('pkexec', ['/bin/sh', '-c', script, 'sh', copy], { timeout: 120_000 }, (error, _stdout, stderr) => {
        if (!error) return resolve('')
        if (error.code === 'ENOENT') return resolve('This system has no way to ask for your password from an app (pkexec is missing).')
        // pkexec: 126 is a closed dialog, 127 a wrong password or no dialog to show.
        if (error.code === 126) return resolve('dismissed')
        if (error.code === 127) return resolve('The system did not accept the password, or could not ask for it.')
        resolve(String(stderr).trim().split('\n').pop() || error.message)
      })
    })
  } finally {
    await fs.promises.rm(dir, { recursive: true, force: true }).catch(() => {})
  }
}

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
  ipcMain.handle('jmk:access', access)
  ipcMain.handle('jmk:allow-access', allowAccess)
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
