// 快捷控制 — 能按什么。
//
// This page answers one question: which keys can this Mac be asked to press,
// and in which app. WHO may ask is the other page's question (手机控制), and
// keeping them apart is the whole reason the sidebar has two entries.
//
// The phone half is not built yet, and this page says so where a reader would
// otherwise assume it is -- next to the buttons, not in a footnote.
import { useCallback, useEffect, useState } from 'react'
import { Command, ArrowUpRight, Play, Accessibility } from 'lucide-react'
import {
  EMPTY_CONSOLE,
  normalizeConsoleStatus,
  stepLabel,
  actionHealth,
  type ConsoleStatus,
  type ConsoleAction,
  type ConsoleDesktopBridge,
} from '../lib/console'

export function ShortcutsPanel({ bridge, onToast }: {
  bridge?: ConsoleDesktopBridge
  onToast?: (message: string) => void
}) {
  const [status, setStatus] = useState<ConsoleStatus>(EMPTY_CONSOLE)
  const [loaded, setLoaded] = useState(false)
  const [running, setRunning] = useState<string | null>(null)

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
                </div>
              </div>
            ))}
          </div>
        )}

        {/* Said here rather than in a footnote: someone looking at these buttons
            is about to assume their phone can already press them. */}
        <p className="security-limit" style={{ marginTop: 18 }}>
          从手机上按，还没做。现在只能在这台 Mac 上试。
        </p>
      </section>
    </>
  )
}
