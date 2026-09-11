// 手机钥匙 (Phone Key) settings panel. Lives in the settings page between the
// "Mac 屏幕保护" security panel and "提醒与声音" (App.tsx). Reuses Outsie's panel
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
  INITIAL_REQUEST_STATE, normalizePairing, IDLE_PAIRING, normalizeUninstall,
  type UnlockSnapshot, type UnlockError, type PanelCommand, type RequestState,
  type PreflightReport, type PairingSession, type UninstallSummary,
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
  const [pairing, setPairing] = useState<PairingSession>(IDLE_PAIRING)
  // What the last removal actually read back off the machine. Shown rather than
  // summarised: "已移除" is a claim, a list of what is and is not still there
  // is a reading.
  const [removal, setRemoval] = useState<UninstallSummary | null>(null)
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

  // ---- Pairing (repose-pair-v2) -------------------------------------------
  //
  // The whole exchange lives behind this sheet. Until now the only way to pair
  // was a shell script, while the phone's own screen told people to press a
  // button here -- so the phone was documenting a control that did not exist.
  //
  // The poll loop is the sheet's clock. It stops the moment the exchange
  // reaches a terminal stage, and the effect's cleanup cancels the tool if the
  // sheet closes, because a pairing window left open is a connectable radio
  // surface nobody is watching.

  const startPairing = useCallback(async () => {
    if (!bridge) { onToast?.('手机钥匙只能在 Outsie Mac App 中使用'); return }
    setPairing({ stage: 'scanning', digits: null, fingerprint: null, peerName: null, detail: null })
    try {
      setPairing(normalizePairing(await bridge.beginPairing()))
    } catch (e) {
      const err = asUnlockError(e)
      setPairing({ stage: 'failed', digits: null, fingerprint: null, peerName: null, detail: err.detail || '配对没能开始' })
    }
  }, [bridge, onToast])

  const closePairing = useCallback(() => {
    void bridge?.cancelPairing().catch(() => undefined)
    setPairing(IDLE_PAIRING)
  }, [bridge])

  const confirmPairing = useCallback(async () => {
    if (!bridge) return
    // Not a stage of its own: the administrator prompt appears on top of this
    // sheet, and the digits must stay behind it. Someone who is mid-comparison
    // should still be able to look.
    try {
      const next = normalizePairing(await bridge.confirmPairing())
      setPairing(next)
      if (next.stage === 'done') {
        const raw = await bridge.getSnapshot().catch(() => null)
        if (raw) setSnapshot(normalizeUnlockSnapshot(raw))
      }
    } catch (e) {
      const err = asUnlockError(e)
      setPairing({ stage: 'failed', digits: null, fingerprint: null, peerName: null, detail: err.detail || '没有写入密钥' })
    }
  }, [bridge])

  // Poll only while something is actually in flight.
  useEffect(() => {
    if (!bridge) return
    if (pairing.stage !== 'scanning' && pairing.stage !== 'compare') return
    let alive = true
    const timer = window.setInterval(() => {
      void bridge.pollPairing()
        .then(raw => { if (alive) setPairing(normalizePairing(raw)) })
        .catch(() => undefined)
    }, 700)
    return () => { alive = false; window.clearInterval(timer) }
  }, [bridge, pairing.stage])

  // Closing the app or navigating away must not leave the tool on the radio.
  useEffect(() => () => { void bridge?.cancelPairing().catch(() => undefined) }, [bridge])

  const dispatchCommand = useCallback((command: PanelCommand) => {
    if (!bridge) { onToast?.('手机钥匙只能在 Outsie Mac App 中使用'); return }
    switch (command) {
      case 'install': setShowInstall(true); break
      case 'repair-rule': void run(command, () => bridge.repair({ target: 'rule' })); break
      case 'reinstall-component': void run(command, () => bridge.repair({ target: 'component' })); break
      // Uninstall returns an UninstallReport, which has no `state` field -- so
      // `run` never refreshed the snapshot and the panel sat there still
      // claiming the feature was installed, after a removal that had actually
      // succeeded and had already asked for an administrator password. Ask for
      // a fresh snapshot explicitly; the report is what the toast reads.
      case 'uninstall':
        void run(command, async () => {
          const report = await bridge.uninstall()
          setRemoval(normalizeUninstall(report))
          return bridge.getSnapshot()
        })
        break
      case 'resume': void run(command, () => bridge.setEnabled({ enabled: true })); break
      case 'open-bluetooth-settings': void bridge.openBluetoothSettings(); break
      // Through `run`, so a refusal reaches the user.
      //
      // These used to be fire-and-forget. The drill asked System Events for a
      // key combination, which needs Accessibility permission nobody had
      // granted, so it failed every time -- and the discarded result meant
      // 「锁屏，试一次」 did visibly nothing, over and over, with no way to find
      // out why.
      case 'start-password-drill':
        void run(command, () => bridge.startDrill({ kind: 'password-drill' })); break
      case 'start-phone-drill':
        void run(command, () => bridge.startDrill({ kind: 'phone-drill' })); break
      case 'begin-pairing': void startPairing(); break
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
              ? '手机钥匙需要修改 macOS 的锁屏授权设置，只能在 Outsie Mac App 中使用；网页版仅预览界面。'
              : '开启需要改一处 macOS 的系统设置。'}
          </p>
        </div>
        <button
          className={`toggle${enabled ? ' on' : ''}`}
          type="button" role="switch" aria-checked={enabled} aria-label="回车解锁"
          disabled={degraded || busy}
          onClick={() => {
            if (degraded) { onToast?.('手机钥匙只能在 Outsie Mac App 中使用'); return }
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
      {/* What the removal actually read back, not a claim that it worked.
          Clicking 移除 used to leave the panel completely unchanged after a
          successful uninstall and an administrator password -- indistinguishable
          from nothing having happened. */}
      {removal && (
        <div className="pk-removal" role="status">
          <div className="pk-removal-head">
            <b>已移除</b>
            <button className="pk-removal-close" aria-label="关闭" onClick={() => setRemoval(null)}>
              <X size={16} />
            </button>
          </div>
          <ul>
            <li>{removal.rightRemoved ? '✓' : '·'} 锁屏规则{removal.backupUsed ? '已从备份还原' : '已改回只认密码'}</li>
            <li>{removal.bundleRemoved ? '✓' : '·'} 组件{removal.bundleRemoved ? '已删除' : '仍在'}</li>
            <li>{removal.keysRemoved ? '✓' : '·'} 这台 Mac 上的配对密钥{removal.keysRemoved ? '已删除' : '仍在'}</li>
          </ul>
          {removal.ruleNow && <p className="pk-removal-rule">现在的锁屏规则：<code>{removal.ruleNow}</code></p>}
          {removal.residual.length > 0 && (
            <p className="pk-removal-left">还剩下：{removal.residual.join('、')}</p>
          )}
          <p className="pk-removal-note">密码登录不受影响，一直都可用。</p>
        </div>
      )}

      {pairing.stage !== 'idle' && (
        <PairingSheet
          session={pairing}
          onClose={closePairing}
          onConfirm={() => void confirmPairing()}
          onRetry={() => void startPairing()}
        />
      )}
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
          <p>不是 Outsie 装的。锁屏规则是「任一条通过即可进入」，装上 Outsie 是<b>再加一条</b>，不是加一道锁。</p>
        </div>
      )}

      <details className="pk-more">
        <summary>还有这些，想看可以展开</summary>
        <ol className="pk-steps">
          <li><b>解锁时你要做什么。</b>唤醒 Mac，密码框出现，<b>不输任何字符，按一下回车。</b>它不会在你走近时自己打开——手机替代的是你的密码，不是那一次按键。偶尔手机还没被认出来，回车会失败一次，再按一次通常就好。</li>
          <li><b>改的是哪一处。</b>把一个解锁组件装到 <code>/Library/Security/SecurityAgentPlugins/</code>，并在锁屏授权规则里加上自己的一条，排在原有密码路径<b>前面</b>。</li>
          <li><b>还原的路一直在。</b>备份、还原脚本、一份纯文本说明都落在 <code>/var/db/repose-unlock/</code>，删掉 Outsie 也不影响。</li>
          {variant === 'B' && <li><b>锁屏界面会换一个程序来画。</b>改由系统的 SecurityAgent 绘制——同样是 macOS 自己的界面，排版可能略有不同。</li>}
          <li><b>它挡不住什么。</b>有人可以转发你手机的无线信号，让这台 Mac 以为你在附近；手机被拿走且处于解锁状态时，带着它靠近仍然能进。这台 Mac 里如果有比手机更值钱的东西，就老实输密码。</li>
        </ol>
      </details>

      {/* Said before it happens, because an unannounced password box is the
          moment people are trained to be suspicious of -- and should be.
          macOS attributes the prompt to the executable that asks. Outsie used
          to ask by shelling out to /usr/bin/osascript, so the box was titled
          "osascript": a name with no relationship to anything the user
          installed. It now asks in-process via NSAppleScript and the box says
          Outsie, which is what makes "确认弹窗上写的是 Outsie" safe advice
          rather than a thing we taught them to ignore.
          See docs/issues/0003-authorization-prompt-identity.md. */}
      <p className="pk-prompt-note">
        点下面之后，macOS 会弹出密码框，问你要管理员密码。<b>确认弹窗上写的是「Outsie」</b>
        ——不是的话就别输。
      </p>
      <div className="pk-modal-actions">
        <button className="button primary" onClick={onConfirm}>开启手机钥匙</button>
        <button className="button light" onClick={onClose}>先不用</button>
      </div>
    </ModalShell>
  )
}

/**
 * The pairing sheet — four stages of one exchange.
 *
 * The `compare` stage is the only place in this product where a human decision
 * is cryptographically load-bearing. Six digits appear here and six appear on
 * the phone; if they match, nobody is in the middle. So that stage is built to
 * make comparing feel like the point rather than a dialog to dismiss: the
 * digits are the largest thing on screen, the confirm button says what is being
 * claimed ("和手机上一样") instead of "确定", and the mismatch button is a real
 * answer rather than a cancel.
 *
 * Deliberately absent: a "跳过核对" escape, and any auto-confirm after a
 * timeout. Both would turn the defence into a formality.
 */
function PairingSheet(
  { session, onClose, onConfirm, onRetry }:
  { session: PairingSession; onClose: () => void; onConfirm: () => void; onRetry: () => void },
) {
  const title = session.stage === 'done' ? '配对完成'
    : session.stage === 'failed' ? '配对没有完成'
    : session.stage === 'compare' ? '核对这六位数字'
    : '正在找你的手机'

  return (
    <ModalShell label={title} onClose={onClose} className="phone-key-modal pk-pair-modal">
      <button className="modal-close icon-button" aria-label="关闭" onClick={onClose}><X size={21} /></button>
      <h2>{title}</h2>

      {session.stage === 'scanning' && (
        <>
          <p className="modal-intro">
            在手机上打开 Outsie，点「开始配对」，然后把手机放在这台 Mac 旁边。
          </p>
          <div className="pk-pair-waiting" role="status" aria-live="polite">
            <span className="pk-pair-dot" /><span className="pk-pair-dot" /><span className="pk-pair-dot" />
          </div>
          <p className="pk-pair-hint">找到之后，两边会各显示一串六位数字。</p>
          <div className="pk-modal-actions">
            <button className="button light" onClick={onClose}>取消</button>
          </div>
        </>
      )}

      {session.stage === 'compare' && (
        <>
          <p className="modal-intro">手机上现在也应该显示这串数字。</p>
          <p className="pk-pair-digits" aria-label={`配对数字 ${session.digits?.split('').join(' ')}`}>
            {session.digits}
          </p>
          <p className="pk-pair-hint">
            两边一样，就说明中间没有人冒充——这一眼是整个配对唯一的安全保障。
            不一样就按「不一样」，然后换个地方重新配一次。
          </p>
          <div className="pk-modal-actions">
            <button className="button primary" onClick={onConfirm}>和手机上一样</button>
            <button className="button light" onClick={onClose}>不一样，停下</button>
          </div>
        </>
      )}

      {session.stage === 'done' && (
        <>
          <p className="modal-intro">{session.detail ?? '这台 Mac 已经认得你的手机了。'}</p>
          <p className="pk-pair-hint">
            以后锁屏时，手机在身边，密码框留空、直接按回车就能进。
          </p>
          {/* The fingerprint used to sit here in large type labelled 配对编号,
              directly after a sheet whose entire point was comparing six other
              digits. Two codes, one flow, and no way to tell from the screen
              which one mattered -- being unsure about that is precisely the
              confusion a man in the middle needs. It is still here, and still
              comparable against the phone; it is just no longer competing with
              the digits for the reader's attention. */}
          {session.fingerprint && (
            <details className="pk-pair-tech">
              <summary>技术细节</summary>
              <p className="pk-pair-fingerprint">
                <span>密钥指纹</span><b>{session.fingerprint}</b>
              </p>
              <p>手机上「这把钥匙 → 技术细节」里是同一串。核对它不是必须的——刚才的六位数字已经做完了这件事。</p>
            </details>
          )}
          <div className="pk-modal-actions">
            <button className="button primary" onClick={onClose}>好</button>
          </div>
        </>
      )}

      {session.stage === 'failed' && (
        <>
          <p className="modal-intro">{session.detail ?? '配对没有完成，没有写入任何密钥。'}</p>
          <div className="pk-modal-actions">
            <button className="button primary" onClick={onRetry}>重新配对</button>
            <button className="button light" onClick={onClose}>先不配</button>
          </div>
        </>
      )}
    </ModalShell>
  )
}

function RemoveConfirm({ onClose, onConfirm }: { onClose: () => void; onConfirm: () => void }) {
  return (
    <ModalShell label="移除手机钥匙并还原系统设置" onClose={onClose} className="phone-key-modal">
      <button className="modal-close icon-button" aria-label="关闭" onClick={onClose}><X size={21} /></button>
      <h2>移除手机钥匙并还原系统设置</h2>
      <p className="modal-intro">Outsie 会按安装的逆序还原：从备份还原锁屏规则、删除授权规则、删除组件与 <code>/var/db/repose-unlock/</code>、删除这台 Mac 上的配对密钥。需要一次管理员密码，完成后把实际读数给你看。</p>
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
