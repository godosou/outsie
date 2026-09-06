const { contextBridge, ipcRenderer } = require('electron')

const commands = new Set(['toggle-pause', 'start-short-break', 'start-long-break', 'postpone-break', 'strict-break-finished', 'idle-lock-failed'])

contextBridge.exposeInMainWorld('repose', {
  isDesktop: true,
  onCommand(callback) {
    if (typeof callback !== 'function') throw new TypeError('onCommand requires a callback')
    const listener = (_event, command) => { if (commands.has(command)) callback(command) }
    ipcRenderer.on('repose:command', listener)
    ipcRenderer.send('repose:ready')
    return () => ipcRenderer.removeListener('repose:command', listener)
  },
  setStatus(value) {
    if (!value || typeof value !== 'object' || typeof value.running !== 'boolean') return
    if (typeof value.phase !== 'string' || value.phase.length > 40) return
    if (typeof value.remaining !== 'number' || !Number.isFinite(value.remaining) || value.remaining < 0 || value.remaining > 604800) return
    if (value.breakId !== null && (typeof value.breakId !== 'string' || !value.breakId || value.breakId.length > 200)) return
    if (typeof value.canPostpone !== 'boolean' || ![0, 60, 300].includes(value.postponeSeconds)) return
    ipcRenderer.send('repose:status', { running: value.running, phase: value.phase, remaining: value.remaining, breakId: value.breakId, canPostpone: value.canPostpone, postponeSeconds: value.postponeSeconds })
  },
  notify(value) {
    if (!value || typeof value !== 'object' || typeof value.title !== 'string' || typeof value.body !== 'string') return
    const title = value.title.trim().slice(0, 100)
    if (title) ipcRenderer.send('repose:notify', { title, body: value.body.slice(0, 400) })
  },
  setPreferences(value) {
    if (!value || typeof value !== 'object' || typeof value.strictBreaks !== 'boolean' || typeof value.idleLockEnabled !== 'boolean') return
    if (value.idleLockSeconds !== 30) return
    ipcRenderer.send('repose:preferences', { strictBreaks: value.strictBreaks, idleLockEnabled: value.idleLockEnabled, idleLockSeconds: value.idleLockSeconds })
  },
  showBreak() {
    ipcRenderer.send('repose:show-break')
  },
  postponeBreak() {
    return ipcRenderer.invoke('repose:postpone-break')
  },
  openSecuritySettings() {
    ipcRenderer.send('repose:open-security-settings')
  },
})
