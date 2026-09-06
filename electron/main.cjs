const { app, BrowserWindow, Menu, Tray, nativeImage, ipcMain, Notification, dialog, screen, powerMonitor, shell, systemPreferences } = require('electron')
const path = require('node:path')
const { mkdirSync, existsSync } = require('node:fs')
const { execFile } = require('node:child_process')
const { pathToFileURL } = require('node:url')

const isDev = process.argv.includes('--repose-dev')
const indexPath = path.join(__dirname, '..', 'dist', 'index.html')
const appURL = isDev ? 'http://127.0.0.1:47832/' : pathToFileURL(indexPath).href
const commands = new Set(['toggle-pause', 'start-short-break', 'start-long-break', 'postpone-break', 'strict-break-finished', 'idle-lock-failed'])
const phaseNames = {
  focus: '专注中',
  short: '短休息',
  long: '长休息',
  shortBreak: '短休息',
  longBreak: '长休息',
  'short-break': '短休息',
  'long-break': '长休息',
}

let mainWindow = null
let tray = null
let quitting = false
let rendererReady = false
let pendingCommands = []
let raiseTimer = null
let lastNotificationAt = 0
let status = { running: true, phase: 'focus', remaining: 20 * 60 }
const notifications = new Set()
let preferences = { strictBreaks: true, idleLockEnabled: false, idleLockSeconds: 30 }
let strictBreak = null
let strictBreakCompleted = false
let pendingPostpone = null
let requestedPostponeId = null
const postponedBreakIds = new Set()
let breakTick = null
let idleTick = null
let idleEnabledAt = Date.now()
let previousIdle = 0
let idleAttempted = false
let sessionLocked = false
let lockInFlight = false
let lockConfirmation = null
let kioskDisplayId = null
let kioskPausedForLock = false
let kioskRelease = Promise.resolve()
const breakWindows = new Map()

app.setName('Repose')
const userDataPath = path.join(app.getPath('appData'), 'Repose')
mkdirSync(userDataPath, { recursive: true })
app.setPath('userData', userDataPath)

function validAppURL(url) {
  try {
    const candidate = new URL(url)
    const expected = new URL(appURL)
    return isDev
      ? candidate.origin === expected.origin
      : candidate.protocol === 'file:' && candidate.pathname === expected.pathname
  } catch {
    return false
  }
}

function trustedSender(event) {
  return Boolean(
    mainWindow && !mainWindow.isDestroyed() &&
    event.sender === mainWindow.webContents &&
    event.senderFrame === mainWindow.webContents.mainFrame &&
    validAppURL(event.senderFrame.url),
  )
}

function trustedBreakSender(event) {
  return [...breakWindows.values()].some(win => !win.isDestroyed() &&
    event.sender === win.webContents &&
    event.senderFrame === win.webContents.mainFrame &&
    event.senderFrame.url === pathToFileURL(path.join(__dirname, 'break.html')).href)
}

function showWindow() {
  if (!mainWindow || mainWindow.isDestroyed()) return
  if (mainWindow.isMinimized()) mainWindow.restore()
  mainWindow.show()
  mainWindow.focus()
}

function sendCommand(command) {
  if (!commands.has(command)) return
  if (rendererReady && mainWindow && !mainWindow.isDestroyed()) {
    mainWindow.webContents.send('repose:command', command)
  } else {
    pendingCommands = [...pendingCommands.slice(-9), command]
  }
}

function isBreakPhase(phase) {
  return ['short', 'long', 'shortBreak', 'longBreak', 'short-break', 'long-break'].includes(phase)
}

function breakSnapshot() {
  if (!strictBreak) return null
  return {
    phase: strictBreak.phase,
    remaining: Math.max(0, Math.ceil((strictBreak.endsAt - Date.now()) / 1000)),
    duration: strictBreak.duration,
    breakId: strictBreak.breakId,
    canPostpone: strictBreak.canPostpone && !pendingPostpone && !postponedBreakIds.has(strictBreak.breakId),
    postponeSeconds: strictBreak.postponeSeconds,
    postponing: Boolean(pendingPostpone),
  }
}

function broadcastBreakStatus() {
  const current = breakSnapshot()
  if (!current) return
  for (const win of breakWindows.values()) {
    if (!win.isDestroyed()) win.webContents.send('repose:break-status', current)
  }
}

function settlePostpone(accepted) {
  if (!pendingPostpone) return
  const request = pendingPostpone
  pendingPostpone = null
  clearTimeout(request.timeout)
  request.resolve(accepted)
}

function requestPostpone() {
  if (!strictBreak || !strictBreak.breakId || !strictBreak.canPostpone ||
      pendingPostpone || postponedBreakIds.has(strictBreak.breakId) ||
      Date.now() >= strictBreak.endsAt || sessionLocked || lockInFlight || quitting) return Promise.resolve(false)
  return new Promise(resolve => {
    // Reserve the action synchronously: simultaneous clicks on other displays
    // cannot send another command. Keep the covering windows until the timer
    // confirms that this exact break has entered its deferred focus period.
    const request = { breakId: strictBreak.breakId, resolve, timeout: null }
    pendingPostpone = request
    requestedPostponeId = request.breakId
    request.timeout = setTimeout(() => {
      if (pendingPostpone !== request) return
      settlePostpone(false)
      broadcastBreakStatus()
    }, 3000)
    broadcastBreakStatus()
    sendCommand('postpone-break')
  })
}

function leaveKiosk(win) {
  if (win.isDestroyed() || !win.isKiosk()) return Promise.resolve()
  return new Promise(resolve => {
    let settled = false
    const finish = () => {
      if (settled) return
      settled = true
      clearTimeout(timeout)
      if (!win.isDestroyed()) win.removeListener('leave-full-screen', finish)
      resolve()
    }
    // Native fullscreen transitions are asynchronous; restore presentation policy
    // before destroying a window or creating the next kiosk owner.
    const timeout = setTimeout(finish, 2000)
    win.once('leave-full-screen', finish)
    win.once('closed', finish)
    win.setKiosk(false)
  })
}

function destroyBreakWindow(win) {
  if (win.isDestroyed()) return Promise.resolve()
  if (!win.isKiosk()) { win.destroy(); return Promise.resolve() }
  const released = leaveKiosk(win).then(() => { if (!win.isDestroyed()) win.destroy() })
  kioskRelease = Promise.all([kioskRelease, released]).then(() => {})
  return released
}

function destroyBreakWindows() {
  const windows = [...breakWindows.values()]
  breakWindows.clear()
  kioskDisplayId = null
  for (const win of windows) destroyBreakWindow(win)
}

async function activateKiosk(win) {
  if (process.platform !== 'darwin') return
  await kioskRelease
  if (!strictBreak || sessionLocked || kioskPausedForLock || quitting || win.isDestroyed()) return
  if (breakWindows.get(kioskDisplayId) !== win || win.isKiosk()) return
  // Only one window owns macOS presentation policy. Other screens remain
  // covered by ordinary all-Space windows, avoiding competing kiosk restores.
  if (win.isVisibleOnAllWorkspaces()) win.setVisibleOnAllWorkspaces(false)
  win.setFullScreenable(true)
  win.show()
  win.focus()
  win.setKiosk(true)
}

function endStrictBreak(notifyRenderer = false, postponed = false) {
  if (!strictBreak) return
  settlePostpone(false)
  requestedPostponeId = null
  strictBreak = null
  strictBreakCompleted = !postponed
  clearInterval(breakTick)
  breakTick = null
  destroyBreakWindows()
  rebuildTrayMenu()
  if (notifyRenderer && !quitting) sendCommand('strict-break-finished')
}

function syncBreakWindows() {
  if (!strictBreak || sessionLocked || quitting) return
  const displays = screen.getAllDisplays()
  const present = new Set(displays.map(display => display.id))
  if (!present.has(kioskDisplayId)) kioskDisplayId = screen.getPrimaryDisplay().id
  for (const [id, win] of breakWindows) {
    if (!present.has(id)) { breakWindows.delete(id); destroyBreakWindow(win) }
  }
  for (const display of displays) {
    const ownsKiosk = process.platform === 'darwin' && display.id === kioskDisplayId
    const existing = breakWindows.get(display.id)
    if (existing && !existing.isDestroyed()) {
      existing.setBounds(display.bounds)
      if (ownsKiosk && existing.isVisible()) void activateKiosk(existing)
      continue
    }
    const win = new BrowserWindow({
      ...display.bounds,
      title: 'Repose · 屏幕休息中',
      frame: false,
      show: false,
      skipTaskbar: true,
      alwaysOnTop: true,
      movable: false,
      resizable: false,
      minimizable: false,
      maximizable: false,
      closable: false,
      fullscreenable: ownsKiosk,
      enableLargerThanScreen: true,
      backgroundColor: '#EEF2E8',
      webPreferences: {
        preload: path.join(__dirname, 'break-preload.cjs'),
        contextIsolation: true,
        sandbox: true,
        nodeIntegration: false,
        backgroundThrottling: false,
        devTools: false,
      },
    })
    breakWindows.set(display.id, win)
    win.setMenu(null)
    win.setAlwaysOnTop(true, 'screen-saver')
    // A kiosk window has its own fullscreen Space; joining all Spaces would
    // eject it and restore the menu bar (also documented in Stretchly's code).
    if (process.platform === 'darwin' && !ownsKiosk) win.setVisibleOnAllWorkspaces(true, { visibleOnFullScreen: true })
    win.on('close', event => { if (strictBreak && !quitting) event.preventDefault() })
    win.on('minimize', () => { if (strictBreak && !sessionLocked) win.restore() })
    win.on('blur', () => {
      if (!strictBreak || sessionLocked || quitting) return
      setTimeout(() => {
        if (!strictBreak || sessionLocked || quitting || win.isDestroyed()) return
        const focused = BrowserWindow.getFocusedWindow()
        if (![...breakWindows.values()].includes(focused)) (breakWindows.get(kioskDisplayId) || win).focus()
      }, 100)
    })
    win.webContents.on('before-input-event', (event, input) => {
      // The only focusable control is the one-time postpone button. Permit its
      // normal keyboard activation, while continuing to consume app shortcuts.
      const buttonKey = ['Tab', 'Enter', ' ', 'Space'].includes(input.key)
      const usableButton = strictBreak?.canPostpone && !pendingPostpone && !postponedBreakIds.has(strictBreak.breakId)
      if (!usableButton || !buttonKey || input.meta || input.control || input.alt) event.preventDefault()
    })
    win.webContents.on('will-navigate', event => event.preventDefault())
    win.webContents.setWindowOpenHandler(() => ({ action: 'deny' }))
    win.webContents.once('did-finish-load', () => {
      if (!strictBreak || win.isDestroyed()) return
      win.webContents.send('repose:break-status', breakSnapshot())
      if (!sessionLocked) {
        if (ownsKiosk) { win.show(); void activateKiosk(win) }
        else win.showInactive()
      }
    })
    win.webContents.on('render-process-gone', () => endStrictBreak(true))
    win.loadFile(path.join(__dirname, 'break.html')).catch(() => endStrictBreak(true))
  }
}

function startStrictBreak(value) {
  if (strictBreak || strictBreakCompleted || value.remaining <= 0 || !preferences.strictBreaks) return
  strictBreak = {
    phase: value.phase,
    endsAt: Date.now() + value.remaining * 1000,
    duration: value.remaining,
    breakId: value.breakId,
    canPostpone: value.canPostpone && !postponedBreakIds.has(value.breakId),
    postponeSeconds: value.phase.toLowerCase().includes('long') ? 300 : 60,
  }
  rebuildTrayMenu()
  syncBreakWindows()
  breakTick = setInterval(() => {
    const current = breakSnapshot()
    if (!current) return
    if (current.remaining <= 0) { endStrictBreak(true); return }
    broadcastBreakStatus()
  }, 250)
}

function reportLockFailure() {
  lockInFlight = false
  kioskPausedForLock = false
  clearTimeout(lockConfirmation)
  lockConfirmation = null
  if (!sessionLocked && !quitting) { syncBreakWindows(); sendCommand('idle-lock-failed') }
}

async function requestSessionLock() {
  if (lockInFlight || sessionLocked || quitting) return
  lockInFlight = true
  if (process.platform !== 'darwin') { reportLockFailure(); return }
  // Kiosk disables session termination shortcuts. Release that policy first,
  // retaining the covering windows until macOS confirms its authenticated lock.
  kioskPausedForLock = true
  const owner = breakWindows.get(kioskDisplayId)
  if (owner && !owner.isDestroyed()) {
    const released = leaveKiosk(owner)
    kioskRelease = Promise.all([kioskRelease, released]).then(() => {})
    await released
  }
  if (quitting || sessionLocked) { lockInFlight = false; return }
  const legacy = '/System/Library/CoreServices/Menu Extras/User.menu/Contents/Resources/CGSession'
  const executable = existsSync(legacy) ? legacy : '/usr/bin/osascript'
  const args = existsSync(legacy) ? ['-suspend'] : ['-e', 'tell application "System Events" to key code 12 using {control down, command down}']
  execFile(executable, args, { timeout: 12000 }, error => {
    if (sessionLocked || quitting) { lockInFlight = false; return }
    if (error) { reportLockFailure(); return }
    lockConfirmation = setTimeout(() => {
      if (powerMonitor.getSystemIdleState(1) === 'locked') { sessionLocked = true; lockInFlight = false; destroyBreakWindows() }
      else reportLockFailure()
    }, 3000)
  })
}

function startIdleMonitor() {
  powerMonitor.on('lock-screen', () => {
    sessionLocked = true
    lockInFlight = false
    clearTimeout(lockConfirmation)
    destroyBreakWindows()
  })
  powerMonitor.on('unlock-screen', () => {
    sessionLocked = false
    kioskPausedForLock = false
    idleAttempted = false
    idleEnabledAt = Date.now()
    syncBreakWindows()
  })
  powerMonitor.on('resume', () => { idleEnabledAt = Date.now(); idleAttempted = false })
  idleTick = setInterval(() => {
    if (!preferences.idleLockEnabled || sessionLocked || quitting) return
    const idle = powerMonitor.getSystemIdleTime()
    if (idle < previousIdle) idleAttempted = false
    previousIdle = idle
    const effectiveIdle = Math.min(idle, (Date.now() - idleEnabledAt) / 1000)
    if (effectiveIdle >= preferences.idleLockSeconds && !idleAttempted) {
      idleAttempted = true
      requestSessionLock()
    }
  }, 1000)
}

function rebuildTrayMenu() {
  if (!tray) return
  const deferredBreak = status.phase === 'focus' && status.breakId && !status.canPostpone
  tray.setContextMenu(Menu.buildFromTemplate([
    { label: '打开 Repose · 歇一会', click: showWindow },
    { type: 'separator' },
    { label: strictBreak ? '强制休息中' : deferredBreak ? '已延迟，等待休息' : status.running ? '暂停提醒' : '继续提醒', enabled: !strictBreak && !deferredBreak, click: () => { if (!strictBreak && !deferredBreak) sendCommand('toggle-pause') } },
    { label: '现在短休息', enabled: !strictBreak, click: () => { if (!strictBreak) { showWindow(); sendCommand('start-short-break') } } },
    { label: '现在长休息', enabled: !strictBreak, click: () => { if (!strictBreak) { showWindow(); sendCommand('start-long-break') } } },
    { type: 'separator' },
    { label: '退出 Repose', accelerator: 'CommandOrControl+Q', enabled: !strictBreak, click: () => { if (!strictBreak) app.quit() } },
  ]))
  const applicationMenu = Menu.getApplicationMenu()
  for (const id of ['repose-quit', 'repose-hide']) {
    const item = applicationMenu?.getMenuItemById(id)
    if (item) item.enabled = !strictBreak
  }
}

function updateTrayTooltip() {
  if (!tray) return
  const minutes = Math.floor(status.remaining / 60)
  const seconds = Math.floor(status.remaining % 60).toString().padStart(2, '0')
  const description = status.running ? (phaseNames[status.phase] || '专注中') : '已暂停'
  tray.setToolTip(`Repose · ${description} · ${minutes}:${seconds}`)
}

function createTray() {
  const icon = nativeImage.createFromPath(path.join(__dirname, 'trayTemplate.png'))
  if (process.platform === 'darwin') icon.setTemplateImage(true)
  tray = new Tray(icon)
  tray.on('click', showWindow)
  tray.on('double-click', showWindow)
  rebuildTrayMenu()
  updateTrayTooltip()
}

function createWindow() {
  mainWindow = new BrowserWindow({
    width: 1440,
    height: 940,
    minWidth: 960,
    minHeight: 720,
    title: 'Repose · 歇一会',
    backgroundColor: '#F8F9F5',
    show: false,
    autoHideMenuBar: true,
    webPreferences: {
      preload: path.join(__dirname, 'preload.cjs'),
      contextIsolation: true,
      sandbox: true,
      nodeIntegration: false,
      webSecurity: true,
      backgroundThrottling: false,
    },
  })

  mainWindow.once('ready-to-show', showWindow)
  mainWindow.on('close', event => {
    if (!quitting && tray) {
      event.preventDefault()
      mainWindow.hide()
    }
  })
  mainWindow.on('closed', () => { mainWindow = null; rendererReady = false })
  mainWindow.webContents.on('did-start-loading', () => { rendererReady = false })
  mainWindow.webContents.on('will-navigate', (event, url) => {
    if (!validAppURL(url)) event.preventDefault()
  })
  mainWindow.webContents.setWindowOpenHandler(() => ({ action: 'deny' }))
  mainWindow.webContents.session.setPermissionRequestHandler((_contents, _permission, callback) => callback(false))
  mainWindow.webContents.session.setPermissionCheckHandler(() => false)
  mainWindow.webContents.on('render-process-gone', (_event, details) => {
    endStrictBreak()
    if (!quitting && details.reason !== 'clean-exit') {
      dialog.showErrorBox('Repose 已停止计时', '应用页面意外关闭。请退出并重新打开 Repose，以继续接收休息提醒。')
    }
  })

  const loaded = isDev ? mainWindow.loadURL(appURL) : mainWindow.loadFile(indexPath)
  loaded.catch(error => {
    if (quitting) return
    dialog.showErrorBox('无法打开 Repose', isDev
      ? `请确认本地开发服务器正在运行：${appURL}\n\n${error.message}`
      : `请先运行 npm run build，再运行 npm run desktop。\n\n${error.message}`)
    app.quit()
  })
}

function registerIPC() {
  ipcMain.on('repose:ready', event => {
    if (!trustedSender(event)) return
    rendererReady = true
    const queued = pendingCommands
    pendingCommands = []
    queued.forEach(sendCommand)
  })

  ipcMain.on('repose:status', (event, value) => {
    if (!trustedSender(event) || !value || typeof value !== 'object') return
    if (typeof value.running !== 'boolean' || typeof value.phase !== 'string' || value.phase.length > 40) return
    if (typeof value.remaining !== 'number' || !Number.isFinite(value.remaining) || value.remaining < 0 || value.remaining > 604800) return
    if (value.breakId !== null && (typeof value.breakId !== 'string' || !value.breakId || value.breakId.length > 200)) return
    if (typeof value.canPostpone !== 'boolean' || ![0, 60, 300].includes(value.postponeSeconds)) return
    if (requestedPostponeId && strictBreak && value.phase === 'focus' && value.running && value.breakId === requestedPostponeId && value.canPostpone === false) {
      // A stalled renderer can acknowledge after the request timeout; only
      // the original, still-active break is eligible for that late ack.
      postponedBreakIds.add(requestedPostponeId)
      if (postponedBreakIds.size > 128) postponedBreakIds.delete(postponedBreakIds.values().next().value)
      settlePostpone(true)
      endStrictBreak(false, true)
    } else if (pendingPostpone && isBreakPhase(value.phase)) {
      // In-flight ticks describe the break before its timer handles the
      // command. They cannot undo the reservation or extend its deadline.
      return
    }
    const runningChanged = value.running !== status.running
    const phaseChanged = value.phase !== status.phase
    const breakChanged = value.breakId !== status.breakId
    status = { running: value.running, phase: value.phase, remaining: Math.floor(value.remaining), breakId: value.breakId, canPostpone: value.canPostpone, postponeSeconds: value.postponeSeconds }
    if (!isBreakPhase(status.phase)) strictBreakCompleted = false
    if (preferences.strictBreaks && isBreakPhase(status.phase)) startStrictBreak(status)
    if (!isBreakPhase(status.phase) && strictBreak && Date.now() >= strictBreak.endsAt) endStrictBreak()
    if (runningChanged || phaseChanged || breakChanged) rebuildTrayMenu()
    updateTrayTooltip()
  })

  ipcMain.handle('repose:postpone-break', event => {
    if (!trustedSender(event)) return false
    return requestPostpone()
  })

  ipcMain.handle('repose:postpone-overlay', event => {
    if (!trustedBreakSender(event)) return false
    return requestPostpone()
  })

  ipcMain.on('repose:preferences', (event, value) => {
    if (!trustedSender(event) || !value || typeof value !== 'object') return
    if (typeof value.strictBreaks !== 'boolean' || typeof value.idleLockEnabled !== 'boolean') return
    if (value.idleLockSeconds !== 30) return
    if (value.idleLockEnabled !== preferences.idleLockEnabled) { idleEnabledAt = Date.now(); idleAttempted = false; previousIdle = 0 }
    preferences = { strictBreaks: value.strictBreaks, idleLockEnabled: value.idleLockEnabled, idleLockSeconds: value.idleLockSeconds }
  })

  ipcMain.on('repose:open-security-settings', event => {
    if (!trustedSender(event) || process.platform !== 'darwin') return
    systemPreferences.isTrustedAccessibilityClient(true)
    shell.openExternal('x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility').catch(() => sendCommand('idle-lock-failed'))
  })

  ipcMain.on('repose:notify', (event, value) => {
    if (!trustedSender(event) || !value || typeof value !== 'object') return
    if (typeof value.title !== 'string' || !value.title.trim() || value.title.length > 100) return
    if (typeof value.body !== 'string' || value.body.length > 400) return
    if (!Notification.isSupported() || Date.now() - lastNotificationAt < 1500) return
    lastNotificationAt = Date.now()
    const notification = new Notification({ title: value.title, body: value.body, silent: true })
    notifications.add(notification)
    notification.on('click', showWindow)
    notification.on('close', () => notifications.delete(notification))
    notification.on('failed', () => notifications.delete(notification))
    notification.show()
  })

  ipcMain.on('repose:show-break', event => {
    if (!trustedSender(event)) return
    if (strictBreak) { syncBreakWindows(); return }
    showWindow()
    clearTimeout(raiseTimer)
    mainWindow.setAlwaysOnTop(true)
    raiseTimer = setTimeout(() => {
      if (mainWindow && !mainWindow.isDestroyed()) mainWindow.setAlwaysOnTop(false)
    }, 1500)
  })
}

if (!app.requestSingleInstanceLock()) {
  app.quit()
} else {
  app.on('second-instance', showWindow)
  app.on('before-quit', event => {
    if (strictBreak && !quitting) { event.preventDefault(); syncBreakWindows(); return }
    quitting = true
    clearTimeout(raiseTimer)
    clearInterval(idleTick)
    clearTimeout(lockConfirmation)
    endStrictBreak()
  })
  app.on('window-all-closed', () => { if (quitting || !tray) app.quit() })
  app.on('activate', () => { if (mainWindow) showWindow(); else createWindow() })

  app.whenReady().then(() => {
    registerIPC()
    createWindow()
    createTray()
    startIdleMonitor()
    screen.on('display-added', syncBreakWindows)
    screen.on('display-removed', syncBreakWindows)
    screen.on('display-metrics-changed', syncBreakWindows)
    Menu.setApplicationMenu(Menu.buildFromTemplate([
      ...(process.platform === 'darwin' ? [{
        label: 'Repose',
        submenu: [
          { label: '关于 Repose', role: 'about' },
          { type: 'separator' },
          { id: 'repose-hide', label: '隐藏 Repose', role: 'hide' },
          { label: '隐藏其他应用', role: 'hideOthers' },
          { label: '显示全部', role: 'unhide' },
          { type: 'separator' },
          { id: 'repose-quit', label: '退出 Repose', role: 'quit' },
        ],
      }] : []),
      { label: '编辑', submenu: [{ role: 'undo' }, { role: 'redo' }, { type: 'separator' }, { role: 'cut' }, { role: 'copy' }, { role: 'paste' }, { role: 'selectAll' }] },
      { label: '窗口', submenu: [{ role: 'minimize' }, { label: '打开 Repose', click: showWindow }, ...(isDev ? [{ role: 'toggleDevTools' }] : [])] },
    ]))
  }).catch(error => {
    dialog.showErrorBox('Repose 无法启动', error.message)
    app.quit()
  })
}
