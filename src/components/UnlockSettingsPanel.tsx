// 手机钥匙 (Phone Key) settings panel. Lives in the settings page between the
// "Mac 屏幕保护" security panel and "提醒与声音" (App.tsx). Reuses Repose's panel
// idiom (.panel / .section-heading / .preference-row / Toggle / .security-limit)
// and the Accessibility feature's degradation pattern; the state model is
// src/lib/unlock.ts (§6 contract). Styles in src/phone-key.css.
//
// Tonight's scope: renders every state via deriveUnlockView, the install
// disclosure, device/revoke, component-status detail, and the !bridge read-only
// degradation. The live backend calls are exercised on a real Mac tomorrow; in
// the browser (!bridge) the panel renders the read-only intro.

import { useCallback, useEffect, useReducer, useRef, useState, type ReactNode } from 'react'
import { createPortal } from 'react-dom'
import { KeyRound, Smartphone, ShieldCheck, X, Monitor } from 'lucide-react'
import {
  normalizeUnlockSnapshot, deriveUnlockView, UNSUPPORTED_SNAPSHOT,
  beginRequest, finishRequest, failRequest, canIssue, healthClass, normalizePreflight,
  INITIAL_REQUEST_STATE,
  type UnlockSnapshot, type UnlockError, type PanelCommand, type RequestState,
  type PreflightReport,
  type UnlockDesktopBridge,
} from '../lib/unlock'

type Props = { bridge?: UnlockDesktopBridge; onToast?: (message: string) => void }

type SnapshotState = { snapshot: UnlockSnapshot; loaded: boolean }
function snapshotReducer(_state: SnapshotState, next: UnlockSnapshot): SnapshotState {
  return { snapshot: next, loaded: true }
}

export function UnlockSettingsPanel({ bridge, onToast }: Props) {
  const degraded = !bridge
  const [{ snapshot, loaded }, setSnapshot] = useReducer(
    snapshotReducer,
    { snapshot: { ...UNSUPPORTED_SNAPSHOT, state: degraded ? 'not-installed' : 'not-installed' }, loaded: false },
  )
  const [request, setRequest] = useState<RequestState>(INITIAL_REQUEST_STATE)
  const [showInstall, setShowInstall] = useState(false)
  // Read before the sheet opens, not after the user agrees: the sheet's job is
  // to say what will happen on THIS Mac, and on some Macs that includes "a third
  // party's authorization plugin is already in the unlock path".
  const [pre, setPre] = useState<PreflightReport | null>(null)
  const [showManifest, setShowManifest] = useState(false)
  const [armedRevoke, setArmedRevoke] = useState<string | null>(null)
  const [detailsOpen, setDetailsOpen] = useState(false)
  const requestRef = useRef(request)
  requestRef.current = request
  // Keep a ref so the presence listener can patch just the presence axis without
  // racing the reducer's latest full snapshot. Declared before the mount effect
  // that reads it.
  const requestSnapshotRef = useRef(snapshot)
  requestSnapshotRef.current = snapshot

  // Mount: pull the first snapshot, subscribe to updates.
  useEffect(() => {
    if (!bridge) return
    let alive = true
    void (async () => {
      try {
        const raw = await bridge.getSnapshot()
        if (alive) setSnapshot(normalizeUnlockSnapshot(raw))
      } catch {
        if (alive) setSnapshot(normalizeUnlockSnapshot(null))
      }
    })()
    const offSnap = bridge.onSnapshot(raw => { if (alive) setSnapshot(normalizeUnlockSnapshot(raw)) })
    const offPres = bridge.onPresence(p => {
      if (!alive || !p || typeof p !== 'object') return
      const presence = (p as { presence?: unknown }).presence
      if (presence === 'near' || presence === 'away' || presence === 'transport-unavailable') {
        setSnapshot({ ...requestSnapshotRef.current, presence })
      }
    })
    return () => { alive = false; offSnap(); offPres() }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [bridge])

  // Run a bridge call behind the single-in-flight guard; apply the returned
  // snapshot or surface the UnlockError.
  const run = useCallback(async (command: PanelCommand, call: () => Promise<unknown>) => {
    if (!bridge || !canIssue(requestRef.current)) return
    const started = beginRequest(requestRef.current, command)
    setRequest(started)
    try {
      const raw = await call()
      if (raw && typeof raw === 'object' && 'state' in (raw as object)) {
        setSnapshot(normalizeUnlockSnapshot(raw))
      }
      setRequest(r => finishRequest(r, started.sequence))
    } catch (e) {
      const err = asUnlockError(e)
      setRequest(r => failRequest(r, started.sequence, err))
      onToast?.(err.detail || '这一步没有成功')
    }
  }, [bridge, onToast])

  const dispatchCommand = useCallback((command: PanelCommand) => {
    if (!bridge) { onToast?.('手机钥匙只能在 Repose Mac App 中使用'); return }
    switch (command) {
      case 'install': setShowInstall(true); break
      case 'repair-rule': void run(command, () => bridge.repair({ target: 'rule' })); break
      case 'reinstall-component': void run(command, () => bridge.repair({ target: 'component' })); break
      case 'uninstall': void run(command, () => bridge.uninstall()); break
      case 'resume': void run(command, () => bridge.setEnabled({ enabled: true })); break
      case 'open-bluetooth-settings': void bridge.openBluetoothSettings(); break
      case 'start-password-drill': void bridge.startDrill({ kind: 'password-drill' }); break
      case 'start-phone-drill': void bridge.startDrill({ kind: 'phone-drill' }); break
      case 'begin-pairing': void run(command, () => bridge.beginPairing()); break
      case 'calibrate': void run(command, () => bridge.calibrateSample({ kind: 'far' })); break
    }
  }, [bridge, run, onToast])

  const confirmInstall = useCallback(async () => {
    setShowInstall(false)
    if (!bridge) return
    await run('install', async () => {
      const p = await bridge.preflight().catch(() => null)
      const variant = (p && typeof p === 'object' && 'variant' in p)
        ? ((p as { variant: 'A' | 'B' | null }).variant) : null
      const snap = await bridge.install({ variant })
      // Installing the component is not the same as watching for the phone.
      // Leaving presence stopped here would put the panel in the one state the
      // user cannot diagnose: installed, switched on, and nothing happening.
      await bridge.setPresenceRunning({ enabled: true }).catch(() => null)
      return snap
    })
  }, [bridge, run])

  const view = deriveUnlockView(snapshot)
  const busy = !canIssue(request)
  const enabled = snapshot.state === 'ready' || snapshot.state === 'awaiting-verification'

  return (
    <section className={`panel preferences-panel phone-key-panel${degraded ? ' is-degraded' : ''}`}>
      <div className="section-heading">
        <div><h2>手机钥匙</h2><p>手机在身边时，回车就是你的密码。</p></div>
        <span className="subtle-badge"><Monitor size={13} />{degraded ? '桌面版专属' : 'Mac 桌面版'}</span>
      </div>

      <div className="preference-row">
        <span className="preference-icon"><KeyRound size={21} /></span>
        <div>
          <h3>回车解锁</h3>
          <p>
            解锁 Mac 时，如果配对的手机在身边，密码框留空、直接按一下回车就能进入。
            <b>它不会在你走近时自己打开</b>——密码框还是会出现，你只是不用输任何字符。
            {degraded
              ? '手机钥匙需要修改 macOS 的锁屏授权设置，只能在 Repose Mac App 中使用；网页版仅预览界面。'
              : '开启需要改一处 macOS 的系统设置。'}
          </p>
        </div>
        <button
          className={`toggle${enabled ? ' on' : ''}`}
          type="button" role="switch" aria-checked={enabled} aria-label="回车解锁"
          disabled={degraded || busy}
          onClick={() => {
            if (degraded) { onToast?.('手机钥匙只能在 Repose Mac App 中使用'); return }
            if (enabled) {
              // Stop watching first, then record the preference. The other order
              // leaves a scanner running for a feature the panel says is off.
              void run('resume', async () => {
                await bridge!.setPresenceRunning({ enabled: false }).catch(() => null)
                return bridge!.setEnabled({ enabled: false })
              })
            } else if (snapshot.state !== 'not-installed') {
              // Already installed: turning it back on is just resuming, no need
              // to re-disclose an install that already happened.
              void run('resume', async () => {
                await bridge!.setEnabled({ enabled: true })
                return bridge!.setPresenceRunning({ enabled: true })
              })
            } else {
              // Never flip green on click; disclose, then install.
              setPre(null)
              setShowInstall(true)
              void bridge!.preflight().then(r => setPre(normalizePreflight(r))).catch(() => setPre(null))
            }
          }}
        ><span /></button>
      </div>

      {/* Status line (axis C). Only `near` is a green dot; `away` is neutral. */}
      {!degraded && loaded && (
        <div className={`phone-key-status tone-${view.status.tone}`}>
          <span className={`pk-dot${view.showReadyDot ? ' ready' : ''}`} />
          <span className="pk-status-main">{view.status.main}</span>
          {view.status.sub && <span className="pk-status-sub">{view.status.sub}</span>}
        </div>
      )}

      {/* Primary action for the current state (exactly one, a verb).

          Except when it would be a second way to do what the switch above
          already does. On a Mac with nothing installed the switch says 回车解锁
          and the button said 开始设置, side by side, both opening the same
          disclosure -- two controls for one decision, which makes a reader stop
          and work out whether they differ. They do not. */}
      {!degraded && view.primaryAction && view.primaryAction.command !== 'install' && (
        <div className="phone-key-primary">
          <button className="button primary" disabled={busy} onClick={() => dispatchCommand(view.primaryAction!.command)}>
            {view.primaryAction.verb}
          </button>
        </div>
      )}

      {/* Paired device + revoke (§5.13). Revoke never needs the phone present. */}
      {!degraded && snapshot.device && (
        <div className="phone-key-device">
          <span className="pk-device-icon"><Smartphone size={18} /></span>
          <div className="pk-device-meta">
            <b>{snapshot.device.name}</b>
            <span>{snapshot.device.platform} · {snapshot.device.pairedAt} 配对</span>
          </div>
          {armedRevoke === snapshot.device.id ? (
            <span className="pk-revoke-confirm">
              <button className="text-button" onClick={() => setArmedRevoke(null)}>取消</button>
              <button
                className="text-button danger-text"
                onClick={() => { const id = snapshot.device!.id; setArmedRevoke(null); void run('resume', () => bridge!.revokeDevice({ deviceId: id })); onToast?.(`已撤销 ${snapshot.device!.name}，这台 Mac 现在只接受密码`) }}
              >确认撤销</button>
            </span>
          ) : (
            <button className="text-button" disabled={busy} onClick={() => setArmedRevoke(snapshot.device!.id)}>撤销这台设备</button>
          )}
        </div>
      )}

      {/* Component status, collapsed. "never-observed" must read as "还没观察到", not a green tick. */}
      {!degraded && loaded && snapshot.state !== 'not-installed' && (
        <details className="phone-key-detail" open={detailsOpen} onToggle={e => setDetailsOpen((e.target as HTMLDetailsElement).open)}>
          <summary>查看组件状态</summary>
          <dl className="pk-klist">
            {snapshot.components.map(c => (
              <div className="pk-krow" key={c.id}>
                <dt>{componentLabel(c.id)}</dt>
                <dd className={healthClass(c.health)}>{c.detail}</dd>
              </div>
            ))}
            <div className="pk-krow">
              <dt>被系统调用</dt>
              <dd className={snapshot.componentInvocation.kind === 'observed' ? 'good' : ''}>
                {snapshot.componentInvocation.kind === 'observed'
                  ? `最近一次确认：${formatTime(snapshot.componentInvocation.at)}`
                  : '还没有观察到被调用'}
              </dd>
            </div>
          </dl>
        </details>
      )}

      <div className="security-permission pk-safety">
        <ShieldCheck size={15} />
        <p>
          解锁时不会自动打开：密码框出现后，不输字符按一下回车即可。手机不在，或者功能出问题，
          <b>密码始终照常可用。</b>
        </p>
      </div>
      <p className="security-limit">
        判断的是「手机在不在附近」，不是「是不是你本人」。有人可转发你手机的信号让这台 Mac 误判；
        手机被拿走且处于解锁状态时，带着它靠近仍会解锁。不放心的场合，用上面的开关暂时关闭。
      </p>

      {!degraded && snapshot.state !== 'not-installed' && (
        <div className="phone-key-remove">
          <button className="button outline full-width" disabled={busy} onClick={() => setShowManifest(true)}>
            移除手机钥匙并还原系统设置
          </button>
        </div>
      )}

      {showInstall && <InstallDisclosure onClose={() => setShowInstall(false)} onConfirm={() => void confirmInstall()} variant={snapshot.variant} pre={pre} />}
      {showManifest && <RemoveConfirm onClose={() => setShowManifest(false)} onConfirm={() => { setShowManifest(false); dispatchCommand('uninstall') }} />}
    </section>
  )
}

// ---- install disclosure (§5.3) -------------------------------------------

// The consent sheet.
//
// The first version listed five numbered steps, a third-party-plugin block and
// three "what it cannot stop" bullets, all at full weight. Every fact in it was
// true and the whole thing was unreadable -- a wall of text at the moment of
// consent gets clicked through, which costs exactly the sentences that mattered.
// Being exhaustive and being honest are not the same thing.
//
// So: the three facts that can change the answer, in full weight, and everything
// else one disclosure away. Nothing was deleted -- an accordion is a different
// claim from a paragraph, but it is not a missing one, and a reader who wants
// the detail is one click from all of it.
function InstallDisclosure(
  { onClose, onConfirm, variant, pre }:
  { onClose: () => void; onConfirm: () => void; variant: 'A' | 'B' | null; pre: PreflightReport | null },
) {
  const foreign = pre?.foreign ?? []
  return (
    <ModalShell label="开启前，有三件事" onClose={onClose} className="phone-key-modal">
      <button className="modal-close icon-button" aria-label="关闭" onClick={onClose}><X size={21} /></button>
      <div className="eyebrow">A KEY IN YOUR POCKET</div>
      <h2>开启前，有三件事</h2>

      <ul className="pk-three">
        <li>
          <b>它认的是手机，不是你。</b>
          手机在身边时，任何坐到这台 Mac 前的人，按一下回车就能进。
        </li>
        <li>
          <b>会改一处 macOS 系统设置。</b>
          需要一次管理员密码；随时可以在这里一键还原。
        </li>
        <li>
          <b>密码永远还能用。</b>
          手机没电、蓝牙关了、功能出问题，照常输密码进入。
        </li>
      </ul>

      {foreign.length > 0 && (
        <div className="pk-foreign">
          <strong>这台 Mac 的锁屏里已经有别的解锁组件</strong>
          <ul>{foreign.map(f => <li key={f}><code>{f}</code></li>)}</ul>
          <p>不是 Repose 装的。锁屏规则是「任一条通过即可进入」，装上 Repose 是<b>再加一条</b>，不是加一道锁。</p>
        </div>
      )}

      <details className="pk-more">
        <summary>还有这些，想看可以展开</summary>
        <ol className="pk-steps">
          <li><b>解锁时你要做什么。</b>唤醒 Mac，密码框出现，<b>不输任何字符，按一下回车。</b>它不会在你走近时自己打开——手机替代的是你的密码，不是那一次按键。偶尔手机还没被认出来，回车会失败一次，再按一次通常就好。</li>
          <li><b>改的是哪一处。</b>把一个解锁组件装到 <code>/Library/Security/SecurityAgentPlugins/</code>，并在锁屏授权规则里加上自己的一条，排在原有密码路径<b>前面</b>。</li>
          <li><b>还原的路一直在。</b>备份、还原脚本、一份纯文本说明都落在 <code>/var/db/repose-unlock/</code>，删掉 Repose 也不影响。</li>
          {variant === 'B' && <li><b>锁屏界面会换一个程序来画。</b>改由系统的 SecurityAgent 绘制——同样是 macOS 自己的界面，排版可能略有不同。</li>}
          <li><b>它挡不住什么。</b>有人可以转发你手机的无线信号，让这台 Mac 以为你在附近；手机被拿走且处于解锁状态时，带着它靠近仍然能进。这台 Mac 里如果有比手机更值钱的东西，就老实输密码。</li>
        </ol>
      </details>

      {/* Said before it happens, because the dialog does not say it itself.
          macOS attributes an authorization prompt to the executable that asks,
          and Repose asks through osascript -- so the box is titled "osascript",
          a name the user has no reason to recognise, at the exact moment they
          are being asked for an administrator password. The real fix is for the
          app process to request authorization itself; until then, saying what
          is coming is better than letting an unexplained prompt appear.
          Tracked in docs/issues/0003-authorization-prompt-identity.md. */}
      <p className="pk-prompt-note">
        点下面之后，macOS 会弹出密码框。<b>它的标题会显示「osascript」</b>——那是 Repose
        用来向系统请求授权的工具，不是别的程序。
      </p>
      <div className="pk-modal-actions">
        <button className="button primary" onClick={onConfirm}>开启手机钥匙</button>
        <button className="button light" onClick={onClose}>先不用</button>
      </div>
    </ModalShell>
  )
}

function RemoveConfirm({ onClose, onConfirm }: { onClose: () => void; onConfirm: () => void }) {
  return (
    <ModalShell label="移除手机钥匙并还原系统设置" onClose={onClose} className="phone-key-modal">
      <button className="modal-close icon-button" aria-label="关闭" onClick={onClose}><X size={21} /></button>
      <h2>移除手机钥匙并还原系统设置</h2>
      <p className="modal-intro">Repose 会按安装的逆序还原：从备份还原锁屏规则、删除授权规则、删除组件与 <code>/var/db/repose-unlock/</code>、删除这台 Mac 上的配对密钥。需要一次管理员密码，完成后把实际读数给你看。</p>
      <div className="pk-modal-actions">
        <button className="button primary" onClick={onConfirm}>移除并还原</button>
        <button className="button light" onClick={onClose}>取消</button>
      </div>
    </ModalShell>
  )
}

// A local Modal shell mirroring App.tsx's Modal (focus trap + Esc + body lock),
// so the panel stays self-contained without importing from App.tsx.
function ModalShell({ children, onClose, className = '', label }: { children: ReactNode; onClose: () => void; className?: string; label: string }) {
  const ref = useRef<HTMLDivElement>(null)
  const closeRef = useRef(onClose)
  closeRef.current = onClose
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null
    ref.current?.querySelector<HTMLElement>('button, [href], input, select, [tabindex="0"]')?.focus()
    const handler = (event: KeyboardEvent) => {
      if (event.key === 'Escape') closeRef.current()
      if (event.key !== 'Tab') return
      const items = ref.current?.querySelectorAll<HTMLElement>('button:not([disabled]), [href], input, select, [tabindex="0"]')
      if (!items?.length) return
      if (event.shiftKey && document.activeElement === items[0]) { event.preventDefault(); items[items.length - 1].focus() }
      else if (!event.shiftKey && document.activeElement === items[items.length - 1]) { event.preventDefault(); items[0].focus() }
    }
    const previousOverflow = document.body.style.overflow
    document.body.style.overflow = 'hidden'
    document.addEventListener('keydown', handler)
    return () => { document.body.style.overflow = previousOverflow; document.removeEventListener('keydown', handler); previous?.focus() }
  }, [])
  // Portalled to <body>, and that is load-bearing rather than tidy.
  //
  // The settings page is `.page-enter`, whose animation ends on
  // `transform: translateY(0)` with fill-mode `both` -- so the final keyframe
  // stays applied forever, and a transform that is not `none` makes the element
  // a containing block for every `position: fixed` descendant. The backdrop then
  // positions against the page instead of the viewport, `max-height: 100dvh`
  // stops meaning the visible area, and the dialog runs off the bottom of the
  // window with no way to scroll to its buttons.
  //
  // Found by opening the install sheet on a real Mac: the disclosure showed its
  // title and one list item, and both "我了解了，继续" and the third-party plugin
  // warning were below the window edge. A consent dialog you cannot read to the
  // end, on the screen where someone agrees to change their lock screen, is the
  // worst place in this app for that bug to live.
  return createPortal(
    <div className="modal-backdrop" onMouseDown={e => { if (e.target === e.currentTarget) onClose() }}>
      <div className={`modal ${className}`} role="dialog" aria-modal="true" aria-label={label} ref={ref}>{children}</div>
    </div>,
    document.body,
  )
}

// ---- helpers -------------------------------------------------------------

function asUnlockError(e: unknown): UnlockError {
  if (e && typeof e === 'object' && 'code' in (e as object)) {
    const err = e as Partial<UnlockError>
    return { code: (err.code ?? 'daemon-unavailable') as UnlockError['code'], detail: err.detail ?? '', remediation: err.remediation, evidence: err.evidence }
  }
  return { code: 'daemon-unavailable', detail: typeof e === 'string' ? e : '后台没有响应' }
}
// These four names are what the reader has to map onto a sentence like "组件不在
// 了，而锁屏规则还指着它". Generic words ('传输', '发放') made that sentence refer to
// rows that were not obviously the ones named, so each label now says what the
// thing is:
//   rule       the macOS lock-screen authorization rule we add an entry to
//   component  the plugin bundle macOS loads at the lock screen
//   daemon     the background check that repairs the rule if the bundle vanishes
//   transport  the presence key -- what makes a beacon yours rather than anyone's
function componentLabel(id: string): string {
  switch (id) {
    case 'rule': return '锁屏规则'
    case 'component': return '解锁组件'
    case 'daemon': return '自动修复'
    case 'transport': return '配对密钥'
    default: return id
  }
}
function formatTime(iso: string): string {
  const d = new Date(iso)
  return Number.isNaN(d.getTime()) ? iso : d.toLocaleTimeString('zh-CN', { hour: '2-digit', minute: '2-digit' })
}
