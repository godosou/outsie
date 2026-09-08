import { useEffect, useRef, useState, type KeyboardEvent } from 'react'
import { invoke, isTauri } from '@tauri-apps/api/core'
import { ArrowDown, ArrowUp, Check, Keyboard, Plus, RotateCcw, Smartphone, Square, Trash2 } from 'lucide-react'
import { createConsoleBridge, consoleConnectionLabel, consoleBluetoothHint, formatConsoleSequence, formatConsoleStep, MODIFIERS, MODIFIER_LABELS, moveConsoleStep, recordConsoleKey, validateConsoleConfig, type ConsoleAction, type ConsoleApp, type ConsoleConfig, type ConsoleStatus, type ConsoleStep, type WorkConsoleBridge } from '../lib/workConsole'

import { ConsoleAppPicker } from './ConsoleAppPicker'
import { appendConsoleImport, parseConsoleImport, MAX_IMPORT_BYTES, CONSOLE_AI_PROMPT } from '../lib/consoleImport'
import codexPresets from '../lib/codexPresets.json'

const nativeBridge = isTauri() ? createConsoleBridge(invoke) : undefined
const blankStep = (): ConsoleStep => ({ key: 'Enter', modifiers: [], delayMs: 0 })
const id = () => crypto.randomUUID()

export function WorkConsolePanel({ bridge = nativeBridge, onOpenPairing }: { bridge?: WorkConsoleBridge; onOpenPairing?: () => void }) {
  const [status, setStatus] = useState<ConsoleStatus | null>(null)
  const [draft, setDraft] = useState<ConsoleConfig | null>(null)
  const [appId, setAppId] = useState('')
  const [actionId, setActionId] = useState('')
  const [error, setError] = useState('')
  const [notice, setNotice] = useState('')
  const [busy, setBusy] = useState(false)
  const [picker, setPicker] = useState<'new' | 'replace' | null>(null)
  const [showAI, setShowAI] = useState(false)
  const fileInput = useRef<HTMLInputElement>(null)
  const [recording, setRecording] = useState(false)
  const recorder = useRef<HTMLDivElement>(null)
  const recordedSteps = useRef<ConsoleStep[]>([])
  const previousKeyTime = useRef(0)
  const busyRef = useRef(false)
  const operationEpoch = useRef(0)
  const mounted = useRef(true)

  useEffect(() => {
    mounted.current = true
    if (!bridge) return
    let active = true
    let polling = false
    const poll = async () => {
      if (busyRef.current || polling) return
      polling = true
      const epoch = operationEpoch.current
      try {
        const next = await bridge.status()
        if (active && !busyRef.current && epoch === operationEpoch.current) {
          setStatus(previous => previous && previous.config.revision > next.config.revision ? previous : next)
        }
      } catch (reason) { if (active && !busyRef.current && epoch === operationEpoch.current) setError(String(reason)) }
      finally { polling = false }
    }
    void poll()
    const timer = window.setInterval(() => void poll(), 1000)
    return () => { active = false; mounted.current = false; window.clearInterval(timer) }
  }, [bridge])

  useEffect(() => {
    if (!recording) return
    const stop = () => setRecording(false)
    const onVisibility = () => { if (document.hidden) stop() }
    window.addEventListener('blur', stop)
    document.addEventListener('visibilitychange', onVisibility)
    return () => { window.removeEventListener('blur', stop); document.removeEventListener('visibilitychange', onVisibility) }
  }, [recording])

  const config = draft ?? status?.config
  const selectedApp = config?.apps.find(app => app.id === appId) ?? config?.apps[0]
  const selectedAction = selectedApp?.actions.find(action => action.id === actionId) ?? selectedApp?.actions[0]
  const conflict = Boolean(draft && status && draft.revision !== status.config.revision)
  const invalid = config ? validateConsoleConfig(config) : null
  const stopRecording = () => setRecording(false)

  const updateApp = (update: (app: ConsoleApp) => ConsoleApp) => {
    if (!config || !selectedApp) return
    setDraft({ ...config, apps: config.apps.map(app => app.id === selectedApp.id ? update(app) : app) })
    setNotice('')
  }
  const updateAction = (update: (action: ConsoleAction) => ConsoleAction) => {
    if (!selectedAction) return
    updateApp(app => ({ ...app, actions: app.actions.map(action => action.id === selectedAction.id ? update(action) : action) }))
  }
  const updateStep = (index: number, patch: Partial<ConsoleStep>) => updateAction(action => ({ ...action, steps: action.steps.map((step, i) => i === index ? { ...step, ...patch } : step) }))

  async function perform(operation: () => Promise<ConsoleStatus>, message = '') {
    if (busyRef.current) return
    operationEpoch.current += 1
    busyRef.current = true; setBusy(true); setError(''); setNotice('')
    try {
      const next = await operation()
      if (mounted.current) {
        setStatus(next)
        if (message) setNotice(message)
      }
    } catch (reason) { if (mounted.current) setError(String(reason)) }
    finally { busyRef.current = false; if (mounted.current) setBusy(false) }
  }

  const save = () => {
    if (!bridge || !draft || invalid) return
    void perform(async () => {
      const next = await bridge.save(draft)
      if (mounted.current) setDraft(null)
      return next
    }, '配置已保存，手机会同步更新。')
  }

  const startRecording = () => {
    recordedSteps.current = []; previousKeyTime.current = 0
    setRecording(true); setNotice(''); recorder.current?.focus()
  }
  const recordKey = (event: KeyboardEvent<HTMLDivElement>) => {
    if (!recording || !selectedAction) return
    event.preventDefault(); event.stopPropagation()
    if (event.key === 'Escape') { stopRecording(); return }
    const now = performance.now()
    const step = recordConsoleKey(event.nativeEvent, previousKeyTime.current ? now - previousKeyTime.current : 0)
    if (!step) return
    previousKeyTime.current = now
    const steps = [...recordedSteps.current, step]
    recordedSteps.current = steps
    updateAction(action => ({ ...action, steps }))
    if (selectedAction.kind === 'hotkey' || steps.length === 20) stopRecording()
  }

  if (!bridge) return <section className="panel wc-panel wc-unavailable"><Keyboard size={28} /><h2>在 Mac App 中配置快捷操作</h2><p>网页版不连接手机，也不会执行系统快捷键。请打开 Repose Mac App，进入「App 工作台」。</p><p>选择 App → 配置快捷键或录制键盘序列 → 手机通过蓝牙连接使用。</p></section>

  return <div className="wc-root page-enter">
    {error && <div className="wc-message wc-error" role="alert">{error}<button type="button" aria-label="关闭错误" onClick={() => setError('')}>×</button></div>}
    {notice && <p className="wc-message" role="status"><Check size={16} />{notice}</p>}
    <section className="panel wc-panel wc-connection">
      <div className="wc-heading"><div><h2><Smartphone size={19} />手机连接</h2><p>沿用「手机钥匙」的蓝牙配对，在手机工作台选择已配对的 Mac。</p></div><span className={`wc-badge ${status?.connected ? 'online' : ''}`}>{consoleConnectionLabel(status)}</span></div>
      {!status ? <p role="status">正在读取 Mac 配置…</p> : <>
        <div className="wc-connect-controls">{status.enabled ? <button className="wc-button" disabled={busy} onClick={() => void perform(() => bridge.stop(), '蓝牙控制已关闭，当前操作已停止。')}>关闭蓝牙控制</button> : <button className="wc-button primary" disabled={busy} onClick={() => void perform(() => bridge.start(), '已启用蓝牙控制，请查看下方连接状态。')}>开启蓝牙控制</button>}{onOpenPairing && <button className="wc-button" disabled={busy} onClick={onOpenPairing}>前往手机钥匙配对</button>}</div>
        <p className="wc-hint">先在「手机钥匙」完成两端配对确认，再开启蓝牙控制。手机靠近 Mac 后选择连接。</p>
        {consoleBluetoothHint(status) && <p className="wc-hint" role="status">{consoleBluetoothHint(status)}</p>}
        {status.pairedDevices && <div className="wc-paired-devices">{status.pairedDevices.length ? status.pairedDevices.map(device => <span className="wc-badge" key={device.id}>{device.name}</span>) : <span>还没有完成授权的手机，请先前往「手机钥匙」配对。</span>}</div>}
        <div className="wc-permission"><span>{status.accessibility ? '✓ 辅助功能权限已开启' : '执行快捷键需要辅助功能权限'}</span>{!status.accessibility && <button className="wc-button" disabled={busy} onClick={() => void perform(() => bridge.accessibility())}>打开权限设置</button>}{status.blocked && <strong>锁屏、休眠或强制休息中，操作已暂停。</strong>}</div>
        {(status.running || status.lastError) && <div className="wc-run-status" role="status"><span>{status.running ? '键盘序列执行中…' : status.lastError}</span>{status.running && <button className="wc-button" disabled={busy} onClick={() => void perform(() => bridge.cancel(), '已停止剩余步骤。')}><Square size={14} />停止执行</button>}</div>}
      </>}
    </section>

    {config && selectedApp && <section className="panel wc-panel">
      <div className="wc-heading"><div><h2>App 快捷操作</h2><p>手机切换 App 后，显示该 App 的操作。按钮顺序可在手机拖动调整。</p></div><button className="wc-button" disabled={busy || recording || config.apps.length >= 16} onClick={() => setPicker('new')}><Plus size={14} />添加 App</button></div>
      <div className="wc-app-tabs" role="tablist" aria-label="配置 App">{config.apps.map(app => <button key={app.id} role="tab" aria-selected={selectedApp.id === app.id} className={selectedApp.id === app.id ? 'selected' : ''} disabled={busy || recording} onClick={() => { setAppId(app.id); setActionId('') }}>{app.name || '未命名 App'}</button>)}</div>
      <div className="wc-import-controls"><button className="wc-button" disabled={busy || recording} onClick={() => fileInput.current?.click()}>导入配置文件</button><button className="wc-button" disabled={recording} onClick={() => setShowAI(!showAI)}>让 AI 生成配置</button><span className="wc-hint">支持 .json；导入为新增 App 草稿，检查并选择本机 App 后保存。</span></div>
      <input ref={fileInput} type="file" accept=".json,application/json" aria-label="导入 JSON 配置文件" hidden disabled={busy || recording} onChange={async event => {
        const file = event.target.files?.[0]; event.target.value = ''
        if (!file || busyRef.current) return
        busyRef.current = true; setBusy(true); setError('')
        try {
          if (file.size > MAX_IMPORT_BYTES) throw new Error('配置文件不能超过 192 KiB。')
          const apps = parseConsoleImport(await file.text())
          if (!mounted.current) return
          setDraft(appendConsoleImport(config, apps)); setAppId(apps[0].id); setActionId('')
          setNotice(`已导入 ${apps.length} 个 App、${apps.reduce((sum, app) => sum + app.actions.length, 0)} 个操作到草稿。请逐个选择本机 App，检查后保存。`)
        } catch (reason) { if (mounted.current) setError(String(reason)) }
        finally { busyRef.current = false; if (mounted.current) setBusy(false) }
      }} />
      {showAI && <div className="wc-ai-help"><h3>把这段说明交给 AI</h3><p>替换 App 名称和需求，让 AI 生成 .json 文件，再点「导入配置文件」。快捷键可继续编辑，也支持录制和等待步骤。</p><textarea aria-label="AI 配置生成说明" readOnly value={CONSOLE_AI_PROMPT} rows={9} /><button className="wc-button" onClick={async () => { try { await navigator.clipboard.writeText(CONSOLE_AI_PROMPT); setNotice('生成说明已复制。') } catch { setError('无法自动复制，请选中上方说明并复制。') } }}>复制生成说明</button></div>}
      <fieldset disabled={busy || recording} className="wc-app-settings"><label>手机上显示的名称<input value={selectedApp.name} maxLength={64} onChange={event => updateApp(app => ({ ...app, name: event.target.value }))} /></label><div className="wc-target-app"><span>控制的本机 App</span><strong>{selectedApp.appPath?.split('/').pop()?.replace(/\.app$/i, '') || (selectedApp.bundleId ? selectedApp.id === 'tmux' ? 'Terminal（默认）' : `${selectedApp.name}（默认）` : '尚未选择')}</strong><button className="wc-button" onClick={() => setPicker('replace')}>{selectedApp.bundleId ? '更换 App' : '选择 App'}</button></div><button className="wc-button danger" aria-label={`删除 App ${selectedApp.name}`} disabled={config.apps.length <= 1} onClick={() => { setDraft({ ...config, apps: config.apps.filter(app => app.id !== selectedApp.id) }); setAppId(''); setActionId('') }}><Trash2 size={15} /></button></fieldset>
      <p className="wc-hint">tmux 请选择运行它的终端，如 Terminal、iTerm 或 Ghostty。按键由 Mac 当前键盘布局解析；输入法合成文字不会录制。</p>
      {(selectedApp.id === 'codex' || selectedApp.bundleId === 'com.openai.codex') && <div className="wc-preset-help"><p className="wc-hint">Codex 预置覆盖官方 macOS 有默认按键的功能，每个功能选用一个按键组合。部分操作需要先聚焦终端、输入框或浏览器；若在 Codex 改过快捷键，请在此同步修改。搜索历史对话需先在 Codex 分配快捷键；Appshot 的双 Command 暂不支持。</p><button className="wc-button" disabled={busy || recording} onClick={() => { const missing = (codexPresets as ConsoleAction[]).filter(preset => !selectedApp.actions.some(action => action.id === preset.id)); if (selectedApp.actions.length + missing.length > 96) { setError('补齐后超过 96 个操作，请先删除不需要的操作。'); return } updateApp(app => ({ ...app, actions: [...app.actions, ...structuredClone(missing)] })); setNotice(`已补充 ${missing.length} 个默认操作，请保存。已有操作保留。`) }}>补齐 Codex 默认操作</button></div>}
      {picker && <ConsoleAppPicker bridge={bridge} onClose={() => setPicker(null)} onChoose={chosen => {
        if (picker === 'new') { const app: ConsoleApp = { id: id(), name: chosen.name, bundleId: chosen.bundleId, appPath: chosen.path, actions: chosen.bundleId === 'com.openai.codex' ? structuredClone(codexPresets) as ConsoleAction[] : [] }; setDraft({ ...config, apps: [...config.apps, app] }); setAppId(app.id); setActionId('') }
        else updateApp(app => ({ ...app, bundleId: chosen.bundleId, appPath: chosen.path }))
        setPicker(null)
      }} />}
      <div className="wc-workspace"><div className="wc-actions"><div className="wc-action-grid">{selectedApp.actions.map(action => <button key={action.id} className={`wc-action ${selectedAction?.id === action.id ? 'selected' : ''} ${action.kind === 'sequence' ? 'sequence' : ''}`} disabled={busy || recording} onClick={() => setActionId(action.id)}><span className="wc-action-icon">{action.icon}</span><strong>{action.name || '未命名操作'}</strong><small>{action.kind === 'sequence' ? `${action.steps.length} 步键盘序列` : action.steps[0] ? formatConsoleStep(action.steps[0]) : '未配置按键'}</small></button>)}</div><button className="wc-button wc-add" disabled={busy || recording || selectedApp.actions.length >= 96} onClick={() => { const action: ConsoleAction = { id: id(), name: '新操作', icon: '⌘', kind: 'hotkey', steps: [blankStep()] }; updateApp(app => ({ ...app, actions: [...app.actions, action] })); setActionId(action.id) }}><Plus size={15} />添加操作 · {selectedApp.actions.length}/96</button></div>
        {selectedAction ? <div className="wc-editor"><fieldset disabled={busy || recording}>
          <div className="wc-fields"><label className="wc-icon-field">图标<input value={selectedAction.icon} maxLength={16} onChange={event => updateAction(action => ({ ...action, icon: event.target.value }))} /></label><label>按钮名称<input value={selectedAction.name} maxLength={64} onChange={event => updateAction(action => ({ ...action, name: event.target.value }))} /></label></div>
          <label>操作类型<select value={selectedAction.kind} onChange={event => updateAction(action => ({ ...action, kind: event.target.value as ConsoleAction['kind'], steps: event.target.value === 'hotkey' ? action.steps.slice(0, 1) : action.steps }))}><option value="hotkey">单个快捷键</option><option value="sequence">键盘序列</option></select></label>
        </fieldset>
        {selectedAction.kind === 'sequence' && <div className="wc-sequence-summary"><p>快捷键 → 等待 → 下一个快捷键；可继续追加。</p><strong aria-label="执行顺序">{formatConsoleSequence(selectedAction.steps)}</strong></div>}
        <div className={`wc-recorder ${recording ? 'recording' : ''}`} ref={recorder} tabIndex={0} role="group" aria-label="键盘录制区域" onKeyDown={recordKey} onBlur={event => { if (!event.currentTarget.contains(event.relatedTarget)) stopRecording() }}>
          <div><Keyboard size={17} /><strong>{recording ? '正在录制，按 Escape 结束' : '录制键盘操作'}</strong></div><p>{recording ? '请在此区域按键。切换焦点会停止录制。' : '重新录制会替换当前步骤。仅记录此区域内的按键，不操作其他 App。'}</p>
          <button className="wc-button" disabled={busy} onClick={recording ? stopRecording : startRecording}>{recording ? '结束录制' : '开始录制'}</button>
        </div>
        <fieldset disabled={busy || recording}><ol className="wc-steps">{selectedAction.steps.map((step, index) => <li key={index}><div className="wc-step-heading"><span>步骤 {index + 1} · {formatConsoleStep(step)}</span><div>{selectedAction.kind === 'sequence' && <><button className="wc-mini" aria-label={`上移步骤 ${index + 1}`} disabled={index === 0} onClick={() => updateAction(action => ({ ...action, steps: moveConsoleStep(action.steps, index, -1) }))}><ArrowUp size={13} /></button><button className="wc-mini" aria-label={`下移步骤 ${index + 1}`} disabled={index === selectedAction.steps.length - 1} onClick={() => updateAction(action => ({ ...action, steps: moveConsoleStep(action.steps, index, 1) }))}><ArrowDown size={13} /></button><button className="wc-mini danger" aria-label={`删除步骤 ${index + 1}`} disabled={selectedAction.steps.length <= 1} onClick={() => updateAction(action => ({ ...action, steps: action.steps.filter((_, i) => i !== index) }))}><Trash2 size={13} /></button></>}</div></div><div className="wc-step-fields"><label>按键<input aria-label={`步骤 ${index + 1} 按键`} value={step.key} onChange={event => updateStep(index, { key: event.target.value.length === 1 ? event.target.value.toLowerCase() : event.target.value })} /></label><label>{index === 0 ? '开始前等待（秒）' : '与上一步间隔（秒）'}<input type="number" min={0} max={5} step={0.1} aria-label={`步骤 ${index + 1} 等待秒数`} value={Number.isFinite(step.delayMs) ? step.delayMs / 1000 : ''} onChange={event => updateStep(index, { delayMs: event.target.value.trim() === '' ? Number.NaN : Math.round(Number(event.target.value) * 1000) })} /></label></div><div className="wc-modifiers">{MODIFIERS.map(modifier => <label key={modifier}><input type="checkbox" checked={step.modifiers.includes(modifier)} onChange={event => updateStep(index, { modifiers: event.target.checked ? [...step.modifiers, modifier] : step.modifiers.filter(item => item !== modifier) })} />{MODIFIER_LABELS[modifier]} {modifier}</label>)}</div></li>)}</ol>
        {selectedAction.kind === 'sequence' && <button className="wc-button" disabled={selectedAction.steps.length >= 20} onClick={() => updateAction(action => ({ ...action, steps: [...action.steps, { ...blankStep(), delayMs: 200 }] }))}><Plus size={14} />添加步骤</button>}
        <div className="wc-editor-footer"><button className="wc-button" disabled={Boolean(draft) || !status?.accessibility || status.blocked || status.running} onClick={() => void perform(() => bridge.run(selectedApp.id, selectedAction.id), '已启动已保存的操作。')}>在 Mac 试运行</button><button className="wc-button danger" onClick={() => { updateApp(app => ({ ...app, actions: app.actions.filter(action => action.id !== selectedAction.id) })); setActionId('') }}><Trash2 size={14} />删除操作</button></div>
        </fieldset><p className="wc-hint">试运行会激活目标 App 并发送按键。编辑后请先保存。</p></div> : <div className="wc-empty"><Keyboard size={28} /><p>添加第一个快捷操作。</p></div>}
      </div>
      {conflict && <div className="wc-message wc-error" role="alert">手机已更新按钮顺序，此草稿基于旧版本。请先取消编辑以载入最新配置，再进行修改。</div>}
      {invalid && <p className="wc-validation" role="status">{invalid}</p>}
      <div className="wc-save-bar"><span>{draft ? '有未保存的更改' : `已保存 · 版本 ${config.revision}`}</span><div><button className="wc-button" disabled={busy || recording || Boolean(draft) || !['tmux', 'codex', 'feishu'].includes(selectedApp.id)} onClick={() => void perform(() => bridge.reset(selectedApp.id, config.revision), '已恢复该 App 的默认操作。')}><RotateCcw size={13} />恢复默认</button><button className="wc-button" disabled={!draft || busy || recording} onClick={() => { setDraft(null); setError(''); setNotice('已取消编辑。') }}>取消编辑</button><button className="wc-button primary" disabled={!draft || busy || recording || Boolean(invalid) || conflict} onClick={save}>{busy ? '处理中…' : '保存配置'}</button></div></div>
    </section>}
  </div>
}
