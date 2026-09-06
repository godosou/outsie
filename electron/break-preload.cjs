const { contextBridge, ipcRenderer } = require('electron')

contextBridge.exposeInMainWorld('reposeBreak', {
  onStatus(callback) {
    if (typeof callback !== 'function') return () => {}
    const listener = (_event, value) => {
      if (!value || typeof value.phase !== 'string' || !Number.isFinite(value.remaining) || !Number.isFinite(value.duration)) return
      if (typeof value.canPostpone !== 'boolean' || ![60, 300].includes(value.postponeSeconds)) return
      callback({ phase: value.phase, remaining: value.remaining, duration: value.duration, canPostpone: value.canPostpone, postponeSeconds: value.postponeSeconds, postponing: value.postponing === true })
    }
    ipcRenderer.on('repose:break-status', listener)
    return () => ipcRenderer.removeListener('repose:break-status', listener)
  },
  postpone() {
    return ipcRenderer.invoke('repose:postpone-overlay')
  },
})
