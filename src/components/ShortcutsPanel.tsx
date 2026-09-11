// 快捷控制 — 能按什么。
//
// This page answers one question: which keys can this Mac be asked to press,
// and in which app. WHO may ask is the other page's question (手机控制), and
// keeping them apart is the whole reason the sidebar has two entries.
//
// The phone half is not built yet, and this page says so where a reader would
// otherwise assume it is -- next to the buttons, not in a footnote.
import { useCallback, useEffect, useState } from 'react'
import { Command, ArrowUpRight, Play, Accessibility, Plus, Trash2, Keyboard, X } from 'lucide-react'
import {
  EMPTY_CONSOLE,
  normalizeConsoleStatus,
  stepLabel,
  actionHealth,
  type ConsoleStatus,
  type ConsoleAction,
  type ConsoleDesktopBridge,
  type ConsoleApp,
  type ConsoleStep,
  type PickedApp,
  stepFromKeyboardEvent,
  slug,
} from '../lib/console'

export function ShortcutsPanel({ bridge, onToast }: {
  bridge?: ConsoleDesktopBridge
  onToast?: (message: string) => void
}) {
  const [status, setStatus] = useState<ConsoleStatus>(EMPTY_CONSOLE)
  const [loaded, setLoaded] = useState(false)
  const [running, setRunning] = useState<string | null>(null)
  // actionId null means "a new one"; the sheet then asks for its name too.
  const [recording, setRecording] = useState<{ appId: string; actionId: string | null; name: string } | null>(null)
  const [busy, setBusy] = useState(false)

  const refresh = useCallback(async () => {
    if (!bridge) { setLoaded(true); return }
    const raw = await bridge.status().catch(() => null)
    setStatus(normalizeConsoleStatus(raw))
    setLoaded(true)
  }, [bridge])

  useEffect(() => { void refresh() }, [refresh])

  // The permission can be granted in System Settings while this window is open,
  // and there is no notification for it. Polling is how the row stops saying
  // 「还没允许」 after the user has just allowed it.
  useEffect(() => {
    if (!bridge || status.trusted) return
    const timer = window.setInterval(() => { void refresh() }, 2000)
    return () => window.clearInterval(timer)
  }, [bridge, status.trusted, refresh])

  /**
   * Every structural edit writes the whole file and re-reads what came back.
   *
   * Not an optimistic local model: the config is a file on disk that the phone
   * will read, and a panel showing a shortcut the file does not contain is the
   * same class of bug as a panel showing a phone the Mac has forgotten. What is
   * on screen is what console_save returned.
   */
  const mutate = useCallback(async (change: (apps: ConsoleApp[]) => ConsoleApp[]) => {
    if (!bridge || busy) return
    setBusy(true)
    try {
      const next = change(status.apps.map(a => ({ ...a, actions: a.actions.map(x => ({ ...x })) })))
      await bridge.save({ config: { apps: next } })
      await refresh()
    } catch (e) {
      onToast?.(typeof e === 'string' ? e : '没能保存')
    } finally {
      setBusy(false)
    }
  }, [bridge, busy, status.apps, refresh, onToast])

  const addApp = useCallback(async () => {
    if (!bridge || busy) return
    setBusy(true)
    try {
      const picked = await bridge.pickApp() as PickedApp | null
      if (!picked?.bundleId) return
      // Same app twice would give two rows that press into one process, and
      // every command would be ambiguous about which row it came from.
      if (status.apps.some(a => a.bundleId === picked.bundleId)) {
        onToast?.(`「${picked.name}」已经在下面了`)
        return
      }
      const id = slug(picked.name, status.apps.map(a => a.id))
      await bridge.save({ config: { apps: [
        ...status.apps,
        { id, name: picked.name, bundleId: picked.bundleId, appPath: picked.path, actions: [] },
      ] } })
      await refresh()
    } catch (e) {
      onToast?.(typeof e === 'string' ? e : '没能加上')
    } finally {
      setBusy(false)
    }
  }, [bridge, busy, status.apps, refresh, onToast])


  const run = useCallback(async (appId: string, action: ConsoleAction) => {
    if (!bridge) return
    setRunning(action.id)
    try {
      await bridge.run({ appId, actionId: action.id })
      onToast?.(`按了「${action.name}」`)
    } catch (e) {
      onToast?.(typeof e === 'string' ? e : `「${action.name}」没有按成功`)
    } finally {
      setRunning(null)
    }
  }, [bridge, onToast])

  if (!bridge) {
    return (
      <section className="panel preferences-panel">
        <div className="section-heading">
          <div><h2>快捷控制</h2><p>能按哪些键在这里定。</p></div>
        </div>
        <div className="security-permission" style={{ marginTop: 18 }}>
          <Command size={15} />
          <p>按键只有 Mac 桌面版能做。</p>
        </div>
      </section>
    )
  }

  return (
    <>
      <section className="panel preferences-panel">
        <div className="section-heading">
          <div>
            <h2>快捷控制</h2>
            <p>这台 Mac 能被要求按哪些键。哪几部手机可以要求，在「手机控制」里。</p>
          </div>
        </div>

        {/* The permission comes first because nothing below it works without
            it, and because 「去授予权限」 is the only action on this page that
            is always available. */}
        <div className="preference-row ks-permission">
          <span className="preference-icon"><Accessibility size={21} /></span>
          <div>
            <h3>替你按键的权限</h3>
            <p className={`pk-row-state${status.trusted ? ' is-on' : ''}`}>
              {status.trusted ? '已允许' : '还没允许 · 现在按什么都不会发生'}
            </p>
            <p className="pk-row-note">
              macOS 把「替别的 App 按键」当成辅助功能权限。没有它，下面的按钮点了不会有任何事。
              顺带一提，「离开就自动锁屏」要的也是这一个。
            </p>
            {!status.trusted && (
              <button
                className="text-button"
                style={{ marginTop: 6 }}
                onClick={() => { void bridge.requestTrust().then(() => refresh()) }}
              >
                去授予权限<ArrowUpRight size={14} />
              </button>
            )}
          </div>
        </div>
      </section>

      <section className="panel preferences-panel">
        <div className="section-heading">
          <div>
            <h2>能按的操作</h2>
            <p>{status.apps.length ? '按一下可以先在这里试，看看键落在哪。' : '还没有。'}</p>
          </div>
        </div>

        {!loaded ? null : status.apps.length === 0 ? (
          // 6.4: an empty state answers what this is, not just offers a button.
          <div className="pk-device-empty">
            <p>
              一个操作就是「在某个 App 里按某几个键」，比如在终端里按 ⌃b 再按 % 来左右分屏。
              编辑器还没做——现在读的是
              <code> work-console-v1.json</code>，那是另一个分支写下的同一份文件。
            </p>
          </div>
        ) : (
          <div className="ks-apps">
            {status.apps.map(app => (
              <div className="ks-app" key={app.id}>
                <div className="ks-app-head">
                  <p className="ks-app-name">{app.name}</p>
                  <p className="ks-app-bundle">{app.bundleId}</p>
                  <button
                    className="text-button ks-app-remove"
                    disabled={busy}
                    onClick={() => void mutate(apps => apps.filter(a => a.id !== app.id))}
                  >
                    <Trash2 size={13} />移掉这个 App
                  </button>
                </div>
                <div className="ks-actions">
                  {app.actions.map(action => {
                    const health = actionHealth(action)
                    return (
                      <div className={`ks-action${health === 'ok' ? '' : ' is-broken'}`} key={action.id}>
                        <span className="ks-action-name">
                          {action.icon && <span className="ks-action-icon">{action.icon}</span>}
                          {action.name}
                        </span>
                        <span className="ks-keys">
                          {action.steps.map((s, i) => <kbd key={i}>{stepLabel(s)}</kbd>)}
                          {health === 'empty' && <em>还没配按键</em>}
                          {health === 'missing-key' && <em>有一步没填按键</em>}
                        </span>
                        {/* Not drawn when it cannot work (ui-conventions 2.1):
                            an action with no keys would fail every time, and a
                            button that always fails teaches people the feature
                            is flaky. */}
                        <button
                          className="text-button ks-icon-button"
                          title="重新录一次按键"
                          disabled={busy}
                          onClick={() => setRecording({ appId: app.id, actionId: action.id, name: action.name })}
                        >
          <Keyboard size={14} />
                        </button>
                        <button
                          className="text-button ks-icon-button"
                          title="删掉这个操作"
                          disabled={busy}
                          onClick={() => void mutate(apps => apps.map(a =>
                            a.id === app.id
                              ? { ...a, actions: a.actions.filter(x => x.id !== action.id) }
                              : a))}
                        >
                          <Trash2 size={14} />
                        </button>
                        {health === 'ok' && (
                          <button
                            className="button light ks-try"
                            disabled={!status.trusted || running !== null}
                            onClick={() => void run(app.id, action)}
                          >
                            <Play size={12} />{running === action.id ? '按下去了…' : '试一次'}
                          </button>
                        )}
                      </div>
                    )
                  })}
                  {app.actions.length === 0 && <p className="ks-none">这个 App 下面还没有操作。</p>}
                  <button
                    className="text-button ks-add-action"
                    disabled={busy}
                    // Not window.prompt: WKWebView leaves it to the host, and
                    // Tauri does not implement it -- the button would have done
                    // nothing at all, silently. The sheet asks for the name.
                    onClick={() => setRecording({ appId: app.id, actionId: null, name: '' })}
                  >
                    <Plus size={13} />加一个操作
                  </button>
                </div>
              </div>
            ))}
          </div>
        )}

        <button className="button outline full-width" disabled={busy} onClick={() => void addApp()} style={{ marginTop: 16 }}>
          <Plus size={15} />加一个 App
        </button>

        {/* Said here rather than in a footnote: someone looking at these buttons
            is about to assume their phone can already press them. */}
        <p className="security-limit" style={{ marginTop: 18 }}>
          从手机上按，还没做。现在只能在这台 Mac 上试。
        </p>
      </section>

      {recording && (
        <KeyRecorder
          initialName={recording.name}
          asksForName={recording.actionId === null}
          onCancel={() => setRecording(null)}
          onDone={(steps, name) => {
            const { appId, actionId } = recording
            setRecording(null)
            void mutate(apps => apps.map(a => {
              if (a.id !== appId) return a
              if (actionId === null) {
                const id = slug(name, a.actions.map(x => x.id))
                // kind left null so the desktop fills it in from the shape; the
                // panel must never invent a value for a field it does not read.
                return { ...a, actions: [...a.actions, { id, name, icon: null, kind: null, steps }] }
              }
              return { ...a, actions: a.actions.map(x => x.id === actionId ? { ...x, name, steps } : x) }
            }))
          }}
        />
      )}
    </>
  )
}

/**
 * Press the keys, in order, and they are recorded.
 *
 * Its own overlay rather than the shared ModalShell: that one swallows Escape
 * and Tab to trap focus, and those are two of the keys a person is most likely
 * to want in a shortcut.
 *
 * Nothing is saved until 「就这些」 — a recorder that committed on every
 * keystroke would make a mistyped key a change you have to undo rather than one
 * you retype.
 */
function KeyRecorder({ onDone, onCancel, initialName, asksForName }: {
  onDone: (steps: ConsoleStep[], name: string) => void
  onCancel: () => void
  initialName: string
  asksForName: boolean
}) {
  const [steps, setSteps] = useState<ConsoleStep[]>([])
  const [name, setName] = useState(initialName)
  const [armed, setArmed] = useState(!asksForName)

  // Only while armed. Otherwise every character typed into the name field would
  // also be recorded as a shortcut.
  useEffect(() => {
    if (!armed) return
    const handler = (e: KeyboardEvent) => {
      e.preventDefault()
      e.stopPropagation()
      const step = stepFromKeyboardEvent(e)
      // A bare modifier is not a keystroke: without this, reaching for ⌃ would
      // record "control" and close the recorder before the real key arrived.
      if (step) setSteps(prev => [...prev, step])
    }
    // Capture phase, so nothing else in the app sees these keys first.
    window.addEventListener('keydown', handler, true)
    return () => window.removeEventListener('keydown', handler, true)
  }, [armed])

  return (
    <div className="modal-backdrop" role="dialog" aria-label="录一组按键">
      <div className="modal phone-key-modal ks-recorder">
        <button className="modal-close icon-button" aria-label="关闭" onClick={onCancel}><X size={21} /></button>
        <h2>{asksForName ? '加一个操作' : '重新录一次'}</h2>
        {asksForName && (
          <label className="ks-name">
            <span>叫什么</span>
            <input
              type="text"
              value={name}
              autoFocus
              placeholder="比如：左右分屏"
              onChange={e => setName(e.target.value)}
            />
          </label>
        )}
        <p className="modal-intro">
          按什么就记什么，按几下记几下。比如先 <kbd>⌃b</kbd> 再 <kbd>%</kbd>，就是终端里的左右分屏。
        </p>
        <div className="ks-recorded">
          {steps.length === 0
            ? <span className="ks-recorded-empty">还没按</span>
            : steps.map((s, i) => <kbd key={i}>{stepLabel(s)}</kbd>)}
        </div>
        <p className="pk-pair-hint">
          {armed
            ? '这里按的键不会传到别的 App，只是记下来。'
            : '先给它起个名字，再开始录——不然打字的每个字母都会被当成按键记下来。'}
        </p>
        <div className="pk-modal-actions">
          <button className="button light" onClick={onCancel}>算了</button>
          {!armed
            ? <button className="button primary" disabled={!name.trim()} onClick={() => setArmed(true)}>开始录</button>
            : <>
                <button className="button light" disabled={!steps.length} onClick={() => setSteps([])}>重来</button>
                <button
                  className="button primary"
                  disabled={!steps.length || !name.trim()}
                  onClick={() => onDone(steps, name.trim())}
                >就这些</button>
              </>}
        </div>
      </div>
    </div>
  )
}
