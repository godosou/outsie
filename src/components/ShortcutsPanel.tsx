// 快捷键设置 — 能按什么。
//
// This page answers one question: which keys can this Mac be asked to press,
// and in which app. WHO may ask is the other page's question (手机控制), and
// keeping them apart is the whole reason the sidebar has two entries.
//
// One app at a time: laid out flat, this desk's three apps and 87 actions ran
// past four screens, and a list that long is scrolled rather than read.
//
// The Accessibility permission is NOT asked for here, though every button on
// this page depends on it. One macOS permission covers two features — 自动锁屏
// and 替手机按键 — and both of those live on 手机控制, so the permission row went
// with them. Asking for it here sent someone who only wants auto-lock to a page
// about shortcuts they never use. What stays here is the consequence: when the
// permission is missing, this page says so and says where to go.
import { useCallback, useEffect, useState } from 'react'
import { Command, Play, Plus, Trash2, Keyboard, X } from 'lucide-react'
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
  // Which app's keys are on screen. Everything used to be laid out flat, and
  // with 87 actions across three apps the page was taller than four screens --
  // a list you scroll past rather than read. Stored as an id, not an index, so
  // removing an app cannot silently select a different one.
  const [openApp, setOpenApp] = useState<string | null>(null)

  const refresh = useCallback(async () => {
    if (!bridge) { setLoaded(true); return }
    const raw = await bridge.status().catch(() => null)
    setStatus(normalizeConsoleStatus(raw))
    setLoaded(true)
  }, [bridge])

  useEffect(() => { void refresh() }, [refresh])

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
      // Open it. An app added to a tab row you are not looking at is an app
      // that looks like it was not added.
      setOpenApp(id)
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
      // The switch to the app is the visible half of a press, so the toast
      // names it first -- the same order the phone's toast uses.
      const app = status.apps.find(a => a.id === appId)
      onToast?.(app ? `切到「${app.name}」，按了「${action.name}」` : `按了「${action.name}」`)
    } catch (e) {
      onToast?.(typeof e === 'string' ? e : `「${action.name}」没按成。再试一次。`)
    } finally {
      setRunning(null)
    }
  }, [bridge, status.apps, onToast])

  // The app whose keys are on screen. Falls back to the first, so the page is
  // never a tab row with nothing beneath it — and a selection that has just
  // stopped existing (its app was removed) resolves the same way instead of
  // leaving the panel blank.
  const open = status.apps.find(a => a.id === openApp) ?? status.apps[0] ?? null

  if (!bridge) {
    return (
      <section className="panel preferences-panel">
        <div className="section-heading">
          <div><h2>快捷键设置</h2><p>手机上能按哪些键，在这里定。</p></div>
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
            <h2>能按的操作</h2>
            {/* Activating the app is visible — whatever was in front goes
                behind. Saying it before the press, not after. */}
            <p>{status.apps.length ? '「试一次」会把那个 App 切到最前面再按键，和手机上按下去时一样。' : '还没有。'}</p>
          </div>
        </div>

        {/* 「试一次」 is greyed out without the Accessibility permission, and a
            grey button that says nothing is a dead end. The permission itself
            is not asked for here any more: it is one permission serving two
            features (自动锁屏 and 替手机按键), and both of the others live on
            手机控制 — so that is where it is granted. `status.trusted` is
            re-read whenever this page is opened, so coming back from there
            shows the new answer. */}
        {loaded && !status.trusted && (
          <p className="security-limit" style={{ marginTop: 0, marginBottom: 14 }}>
            现在按什么都不会有反应。Outsie 还没被允许替你按键。
            去「手机控制」那一页允许它，自动锁屏用的也是同一个开关。
          </p>
        )}

        {!loaded ? null : status.apps.length === 0 ? (
          // 6.4: an empty state answers what this is, not just offers a button.
          <div className="pk-device-empty">
            <p>
              一个操作就是「在某个 App 里按某几个键」。比如在终端里按 ⌃b 再按 %，就是左右分屏。
              先加一个 App，再往里面加操作。
            </p>
          </div>
        ) : (
          <div className="ks-apps">
            {/* One app at a time. Three apps and 87 actions laid out flat ran
                past four screens, and a list that long is scrolled, not read. */}
            <div className="ks-tabs" role="tablist" aria-label="选一个 App">
              {status.apps.map(app => (
                <button
                  key={app.id}
                  role="tab"
                  aria-selected={app.id === open?.id}
                  className={`ks-tab${app.id === open?.id ? ' is-open' : ''}`}
                  onClick={() => setOpenApp(app.id)}
                >
                  {app.name}<span className="ks-tab-count">{app.actions.length}</span>
                </button>
              ))}
              <button className="ks-tab ks-tab-add" disabled={busy} onClick={() => void addApp()}>
                <Plus size={13} />App
              </button>
            </div>

            {open && (
              <div className="ks-app" key={open.id}>
                <div className="ks-app-head">
                  <p className="ks-app-bundle">{open.bundleId}</p>
                  <button
                    className="text-button ks-app-remove"
                    disabled={busy}
                    onClick={() => void mutate(apps => apps.filter(a => a.id !== open.id))}
                  >
                    <Trash2 size={13} />移掉这个 App
                  </button>
                </div>
                <div className="ks-actions">
                  {open.actions.map(action => {
                    const app = open
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
                  {open.actions.length === 0 && <p className="ks-none">这个 App 下面还没有操作。</p>}
                  <button
                    className="text-button ks-add-action"
                    disabled={busy}
                    // Not window.prompt: WKWebView leaves it to the host, and
                    // Tauri does not implement it -- the button would have done
                    // nothing at all, silently. The sheet asks for the name.
                    onClick={() => setRecording({ appId: open.id, actionId: null, name: '' })}
                  >
                    <Plus size={13} />加一个操作
                  </button>
                </div>
              </div>
            )}
          </div>
        )}

        {status.apps.length === 0 && (
          <button className="button outline full-width" disabled={busy} onClick={() => void addApp()} style={{ marginTop: 16 }}>
            <Plus size={15} />加一个 App
          </button>
        )}

        {/* Said here rather than in a footnote: someone looking at these buttons
            is about to assume they take effect on every phone. */}
        <p className="security-limit" style={{ marginTop: 18 }}>
          手机上同步一次，才看得到这些按钮。哪部手机可以按，在「手机控制」里定。
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
              placeholder="比如「左右分屏」"
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
            : '先给它起个名字，再开始录。不然打字的每个字母都会被当成按键记下来。'}
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
