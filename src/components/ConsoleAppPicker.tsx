import { useEffect, useRef, useState } from 'react'
import type { InstalledConsoleApp, WorkConsoleBridge } from '../lib/workConsole'

export function ConsoleAppPicker({ bridge, onChoose, onClose }: { bridge: WorkConsoleBridge; onChoose(app: InstalledConsoleApp): void; onClose(): void }) {
  const dialog = useRef<HTMLDialogElement>(null)
  const [apps, setApps] = useState<InstalledConsoleApp[]>([])
  const [query, setQuery] = useState('')
  const [loading, setLoading] = useState(true)
  const [picking, setPicking] = useState(false)
  const [error, setError] = useState('')
  useEffect(() => {
    let active = true
    dialog.current?.showModal()
    void bridge.listApps().then(value => { if (active) setApps(value) }).catch(reason => { if (active) setError(String(reason)) }).finally(() => { if (active) setLoading(false) })
    return () => { active = false }
  }, [bridge])
  const filtered = apps.filter(app => app.name.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()))
  return <dialog ref={dialog} className="wc-app-picker" aria-labelledby="wc-picker-title" onCancel={event => { event.preventDefault(); if (!picking) onClose() }}>
    <div className="wc-heading"><div><h2 id="wc-picker-title">选择本机 App</h2><p>选择要在手机上控制的应用程序。</p></div><button className="wc-button" disabled={picking} onClick={onClose}>取消</button></div>
    <input autoFocus type="search" aria-label="搜索已安装 App" placeholder="搜索 App 名称…" value={query} onChange={event => setQuery(event.target.value)} />
    {error && <p className="wc-error" role="alert">{error}</p>}
    <div className="wc-installed-apps">{loading ? <p role="status">正在读取已安装 App…</p> : filtered.length ? filtered.map(app => <button key={app.path} disabled={picking} className="wc-installed-app" onClick={() => onChoose(app)}>{app.icon ? <img src={app.icon} alt="" /> : <span className="wc-app-placeholder">▣</span>}<span><strong>{app.name}</strong><small>{app.path}</small></span></button>) : <p>未找到匹配的 App，可从文件夹选择。</p>}</div>
    <button className="wc-button" disabled={picking} onClick={async () => { setPicking(true); setError(''); try { const app = await bridge.pickApp(); if (app) onChoose(app) } catch (reason) { setError(String(reason)) } finally { setPicking(false) } }}>{picking ? '正在选择…' : '从文件夹选择 .app…'}</button>
  </dialog>
}
