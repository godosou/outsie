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
import { KeyRound, Smartphone, X, Monitor, LockKeyhole, ArrowUpRight } from 'lucide-react'
import {
  normalizeUnlockSnapshot, deriveUnlockView, UNSUPPORTED_SNAPSHOT,
  beginRequest, finishRequest, failRequest, canIssue, healthClass, normalizePreflight,
  INITIAL_REQUEST_STATE, normalizePairing, IDLE_PAIRING, normalizeUninstall,
  type UnlockSnapshot, type UnlockError, type PanelCommand, type RequestState,
  type PreflightReport, type PairingSession, type UninstallSummary,
  type UnlockDesktopBridge,
  type PairedDevice,
  type CalibrationProgress,
  type CalibrationResult,
  normalizeCalibration,
  normalizeCalibrationProgress,
  calibrationLegReady,
} from '../lib/unlock'

/**
 * 自动锁屏 arrives as a prop rather than being read here, because its state
 * lives in App.tsx's desktopPreferences (localStorage, shared with the break
 * machinery). What matters is that it is RENDERED here: the phone key exists so
 * that locking aggressively stops costing anything, and on separate pages the
 * product was inviting the very failure it was built to prevent.
 */
type IdleLock = {
  enabled: boolean
  error: boolean
  onToggle: () => void
  onOpenSettings: () => void
}

type Props = {
  bridge?: UnlockDesktopBridge
  onToast?: (message: string) => void
  idleLock?: IdleLock
}

type SnapshotState = { snapshot: UnlockSnapshot; loaded: boolean }
function snapshotReducer(_state: SnapshotState, next: UnlockSnapshot): SnapshotState {
  return { snapshot: next, loaded: true }
}

export function UnlockSettingsPanel({ bridge, onToast, idleLock }: Props) {
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
  // An error that names a setting should be able to open it. The panel has
  // already shipped one 「前往设置」 that led to the page the reader was
  // standing on, so this one has to land somewhere real.
  const [fixHint, setFixHint] = useState<string | null>(null)
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
      setFixHint(err.detail?.includes('锁定屏幕') ? err.detail : null)
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

  // ---- calibration -------------------------------------------------------
  // Its own sheet rather than a stage of pairing: you re-walk it when you move
  // desks, and a measurement you can only take once, during setup, is one the
  // Mac goes on using long after the room stopped matching it.
  const [calibrating, setCalibrating] = useState(false)
  const [calLeg, setCalLeg] = useState<'intro' | 'near' | 'walk' | 'far' | 'done'>('intro')
  const [calProgress, setCalProgress] = useState<CalibrationProgress | null>(null)
  const [calResult, setCalResult] = useState<CalibrationResult | null>(null)

  const startLeg = useCallback(async (kind: 'near' | 'far') => {
    if (!bridge) return
    setCalResult(null)
    setCalProgress(normalizeCalibrationProgress(await bridge.calibrateStart({ kind }).catch(() => null)))
    setCalLeg(kind)
  }, [bridge])

  // Poll only while a leg is being walked.
  useEffect(() => {
    if (!bridge || (calLeg !== 'near' && calLeg !== 'far')) return
    let alive = true
    const timer = window.setInterval(() => {
      void bridge.calibrateSample().then(raw => {
        if (alive) setCalProgress(normalizeCalibrationProgress(raw))
      }).catch(() => undefined)
    }, 1000)
    return () => { alive = false; window.clearInterval(timer) }
  }, [bridge, calLeg])

  const finishCalibration = useCallback(async () => {
    if (!bridge) return
    const r = normalizeCalibration(await bridge.calibrateFinish().catch(() => null))
    setCalResult(r)
    setCalLeg('done')
    if (r.outcome.kind === 'ok') {
      const snap = await bridge.getSnapshot().catch(() => null)
      if (snap) setSnapshot(normalizeUnlockSnapshot(snap))
    }
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
    const live = ['scanning', 'compare', 'waiting-for-phone']
    if (!live.includes(pairing.stage)) return
    let alive = true
    const timer = window.setInterval(() => {
      // Two different questions. Before the key is written, "what is the
      // pairing tool doing"; after, "have both ends got the same key" -- and
      // only the second one has an answer once the tool has exited.
      const ask = pairing.stage === 'waiting-for-phone'
        ? bridge.awaitPhonePairing()
        : bridge.pollPairing()
      void ask
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
      case 'calibrate': setCalibrating(true); break
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
  // The switch shows whether anything is watching for the phone -- the one
  // thing it can actually change. It used to be derived from `state`, which
  // stays AwaitingVerification whether the monitor runs or not: the switch
  // rendered ON permanently, only its turn-OFF branch was ever reachable, and
  // clicking it did nothing visible. Forever.
  const enabled = snapshot.presenceRunning

  return (
    <>
    <section className={`panel preferences-panel phone-key-panel${degraded ? ' is-degraded' : ''}`}>
      <div className="section-heading">
        {/* The page is already titled 手机就是钥匙; repeating 手机钥匙 here says
            nothing. This panel is the pair of switches, so it is named for
            them. The 「Mac 桌面版」 badge is noise inside the Mac app -- kept
            only where it is news, which is the web preview. */}
        <div><h2>锁与开</h2><p>锁得紧不紧，和回来要不要输密码。</p></div>
        {degraded && <span className="subtle-badge"><Monitor size={13} />桌面版专属</span>}
      </div>

      <div className="preference-row">
        <span className="preference-icon"><KeyRound size={21} /></span>
        <div>
          {/* The name stays put; the line under it carries the state.
              For one build the title itself changed — 正在等你的手机 /
              没有在等手机 — which is not how Chinese apps read and is not how
              anyone says it out loud. A heading names the thing; whether it is
              on belongs underneath. */}
          <h3>回来就不用再输密码</h3>
          {!degraded && loaded && (
            <p className={`pk-row-state${enabled ? ' is-on' : ''}`}>
              {/* "Watching for your phone" is only true if there is a phone to
                  recognise. With the key deleted the monitor keeps running and
                  keeps rejecting everything, and saying 正在留意你的手机 over
                  that is the panel describing a phone that no longer exists. */}
              {!enabled
                ? '已关闭 · 现在只能用密码登录'
                : snapshot.device
                  ? '已开启 · 正在留意你的手机'
                  : '已开启 · 但还没有哪部手机能用来解锁'}
            </p>
          )}
          {/* Says what you get, then reassures. It used to describe the
              mechanism twice -- 「回车解锁」 as a heading and 「锁屏时留空回车」
              under it -- and the reassurance read like a disclaimer rather than
              like someone telling you it is fine. 「留空」 is jargon: nobody
              thinks "leave the field empty", they think "don't type anything". */}
          <p>
            手机在身边的时候，锁屏上直接按一下回车就进来了。
            <b>它不会自己打开</b>——那一下回车还是要你按。
            {degraded
              ? '这个功能要改一处 macOS 的锁屏设置，只能在 Outsie 桌面版里用；网页上只能看看界面。'
              : '第一次开启时要改一处 macOS 的系统设置。'}
          </p>
        </div>
        <button
          className={`toggle${enabled ? ' on' : ''}`}
          type="button" role="switch" aria-checked={enabled} aria-label="用手机解锁"
          disabled={degraded || busy}
          onClick={() => {
            if (degraded) { onToast?.('手机钥匙只能在 Outsie Mac App 中使用'); return }
            if (enabled) {
              void run('resume', () => bridge!.setPresenceRunning({ enabled: false }))
            } else if (snapshot.state !== 'not-installed') {
              // Already installed: turning it back on is just starting the
              // monitor, with no install to re-disclose.
              //
              // setEnabled used to be called on both sides of this. It is a
              // placeholder that changes nothing and re-reads, so it could only
              // ever return the same snapshot the switch was already wrong
              // about -- which is what made the OFF state unreachable.
              void run('resume', () => bridge!.setPresenceRunning({ enabled: true }))
            } else {
              // Never flip green on click; disclose, then install.
              setPre(null)
              setShowInstall(true)
              void bridge!.preflight().then(r => setPre(normalizePreflight(r))).catch(() => setPre(null))
            }
          }}
        ><span /></button>
      </div>

      {/* The other half of the pair. Turning unlock off is the moment somebody
          is most likely to reach for the lock delay next, so that is where this
          has to be standing. */}
      {idleLock && (
        <div className="preference-row">
          <span className="preference-icon"><LockKeyhole size={21} /></span>
          <div>
            <h3>离开 30 秒就自动锁屏</h3>
            {/* The state line and the switch must not disagree. This said
                「开着，但没有生效」 whenever securityError was set -- and the
                error path in App.tsx also switches it OFF, so the sentence
                claimed 开着 above a grey switch. */}
            <p className={`pk-row-state${idleLock.enabled ? ' is-on' : ''}`}>
              {idleLock.enabled
                ? '已开启'
                : idleLock.error
                  ? '已关闭 · macOS 拒绝了，因为缺辅助功能权限'
                  : '已关闭 · 离开电脑不会自动锁屏'}
            </p>
            <p>
              这两个是一对：锁得越紧越安全，而上面那个负责让你不为此多输一次密码。
              {enabled
                ? ''
                : '关掉解锁之前，先想想会不会顺手也把这个关了——那才是真正变不安全的那一步。'}
            </p>
            {idleLock.error && (
              <button className="text-button" onClick={idleLock.onOpenSettings} style={{ marginTop: 6 }}>
                去授予权限<ArrowUpRight size={14} />
              </button>
            )}
          </div>
          <button
            className={`toggle${idleLock.enabled ? ' on' : ''}`}
            type="button" role="switch" aria-checked={idleLock.enabled} aria-label="离开 30 秒就自动锁屏"
            onClick={idleLock.onToggle}
          ><span /></button>
        </div>
      )}

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

      {/* Component status, collapsed. "never-observed" must read as "还没观察到", not a green tick. */}
      {!degraded && loaded && snapshot.state !== 'not-installed' && (
        <details className="phone-key-detail" open={detailsOpen} onToggle={e => setDetailsOpen((e.target as HTMLDetailsElement).open)}>
          <summary>技术细节</summary>
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

          {/* Kept, not deleted (ui-conventions 3.1). This is what the feature
              cannot protect against -- the person who wants to know must be
              able to find it on the day they go looking. */}
          <p className="pk-fold-note">
            它判断的是「手机在不在附近」，不是「是不是你本人」。有人可以转发你手机的信号让这台 Mac
            误判；手机被别人拿走、而且还没锁屏时，带着它靠近一样会解锁。不放心的场合，把上面的开关
            临时关掉。
          </p>

          {!degraded && (
            <button className="button outline full-width" disabled={busy} onClick={() => setShowManifest(true)} style={{ marginTop: 14 }}>
              不再使用，把 Mac 改回原样
            </button>
          )}
        </details>
      )}

      {/* One line, always present, everywhere. The single most important
          sentence in this feature: whatever is broken, the password works. */}
      <p className="security-limit">
        不管这些开关是什么状态，<b>Mac 密码一直都能登录。</b>
      </p>

      {showInstall && <InstallDisclosure onClose={() => setShowInstall(false)} onConfirm={() => void confirmInstall()} variant={snapshot.variant} pre={pre} />}
      {showManifest && <RemoveConfirm onClose={() => setShowManifest(false)} onConfirm={() => { setShowManifest(false); dispatchCommand('uninstall') }} />}
      {/* An error the user can act on, with the control that acts on it. */}
      {fixHint && (
        <div className="pk-removal" role="status">
          <div className="pk-removal-head">
            <b>还差一步</b>
            <button className="pk-removal-close" aria-label="关闭" onClick={() => setFixHint(null)}>
              <X size={16} />
            </button>
          </div>
          <p className="pk-removal-note" style={{ marginTop: 8 }}>{fixHint}</p>
          <button
            className="button outline full-width"
            style={{ marginTop: 12 }}
            onClick={() => { void bridge?.openLockScreenSettings() }}
          >打开「锁定屏幕」设置</button>
        </div>
      )}

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
          onTryLock={() => { closePairing(); dispatchCommand('start-phone-drill') }}
          onCalibrate={() => { closePairing(); setCalibrating(true) }}
        />
      )}

      {calibrating && (
        <CalibrationSheet
          leg={calLeg}
          progress={calProgress}
          result={calResult}
          onClose={() => { setCalibrating(false); setCalLeg('intro'); setCalResult(null) }}
          onStartLeg={kind => void startLeg(kind)}
          onWalk={() => setCalLeg('walk')}
          onFinish={() => void finishCalibration()}
        />
      )}
    </section>

    {!degraded && loaded && snapshot.state !== 'not-installed' && (
      <PhoneList
        device={snapshot.device}
        // The panel above already offers 配对手机 as its primary action while
        // there is no key. Two buttons for one decision made the reader stop
        // and work out whether they were the same thing (ui-conventions 2.5).
        onPair={view.primaryAction?.command === 'begin-pairing' ? null : () => dispatchCommand('begin-pairing')}
        busy={busy}
        armed={armedRevoke}
        onArm={setArmedRevoke}
        onRevoke={id => {
          setArmedRevoke(null)
          void run('revoke-device', () => bridge!.revokeDevice({ deviceId: id }))
        }}
        onCalibrate={() => setCalibrating(true)}
      />
    )}
    </>
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


// ---------------------------------------------------------------------------
// Two walks, and a refusal when they look the same.
//
// The far leg is the awkward one: you are walking away from the screen that is
// telling you what to do. So the instruction is given BEFORE you go, the leg
// finishes itself once it has enough samples, and the result is waiting when
// you come back. Nothing on this sheet requires you to read it from the far
// side of the room.
function CalibrationSheet({ leg, progress, result, onClose, onStartLeg, onWalk, onFinish }: {
  leg: 'intro' | 'near' | 'walk' | 'far' | 'done'
  progress: CalibrationProgress | null
  result: CalibrationResult | null
  onClose: () => void
  onStartLeg: (kind: 'near' | 'far') => void
  onWalk: () => void
  onFinish: () => void
}) {
  const title = leg === 'done' ? (result?.outcome.kind === 'ok' ? '量好了' : '这次没量出来')
    : leg === 'near' ? '站在你平时的位置'
    : leg === 'walk' ? '接下来要走开'
    : leg === 'far' ? '走开，别看这块屏幕'
    : '量一下「多近算在身边」'

  const enough = calibrationLegReady(progress)
  // The bar tracks whichever requirement is further from being met, so it can
  // never sit full while the button is still disabled.
  const pct = progress
    ? Math.min(100, Math.round(100 * Math.min(
        progress.samples / Math.max(1, progress.needed),
        progress.elapsedMs / Math.max(1, progress.neededMs),
      )))
    : 0
  const left = progress ? Math.max(0, Math.ceil((progress.neededMs - progress.elapsedMs) / 1000)) : 0

  return (
    <ModalShell label={title} onClose={onClose} className="phone-key-modal pk-pair-modal">
      <button className="modal-close icon-button" aria-label="关闭" onClick={onClose}><X size={21} /></button>
      <h2>{title}</h2>

      {leg === 'intro' && (
        <>
          <p className="modal-intro">
            Mac 靠信号强弱猜你在不在。强弱跟房间有关，所以要在<b>你实际用它的地方</b>量一次。
          </p>
          <p className="pk-pair-hint">
            两段，各十五秒以上：先站着不动，再走开待一会儿。手机要带在身上。
          </p>
          <div className="pk-modal-actions">
            <button className="button light" onClick={onClose}>以后再说</button>
            <button className="button primary" onClick={() => onStartLeg('near')}>开始</button>
          </div>
        </>
      )}

      {(leg === 'near' || leg === 'far') && (
        <>
          <p className="modal-intro">
            {leg === 'near'
              ? '就坐在或站在你平常的位置，手机放在平常放的地方，别动它。'
              : '已经在记了。走开就行，够了会自己停，回来再看结果。'}
          </p>
          <CalMeter progress={progress} pct={pct} />
          {progress && !progress.monitorRunning && (
            <p className="pk-pair-hint">
              {/* Otherwise an empty leg looks like "the phone is far away",
                  which during the NEAR leg is the opposite of the truth. */}
              没有在收信号——监测没在跑，这样量不到东西。先把上面那个开关打开。
            </p>
          )}
          <div className="pk-modal-actions">
            <button className="button light" onClick={onClose}>取消</button>
            {leg === 'near'
              ? <button className="button primary" disabled={!enough} onClick={onWalk}>
                  {enough ? '这段够了，下一步' : `再站 ${left} 秒`}
                </button>
              : <button className="button primary" disabled={!enough} onClick={onFinish}>
                  {enough ? '我回来了，看结果' : `还要 ${left} 秒`}
                </button>}
          </div>
        </>
      )}

      {leg === 'walk' && (
        <>
          <p className="modal-intro">
            现在请<b>带着手机走开</b>——走到你希望 Mac 自动锁屏的那个距离之外，比如出了这个房间。
          </p>
          <p className="pk-pair-hint">
            点下面这个按钮再走。到了那边什么都不用做，待上二十来秒再回来——不够久它会直说。
          </p>
          <div className="pk-modal-actions">
            <button className="button light" onClick={() => onStartLeg('near')}>重量这一段</button>
            <button className="button primary" onClick={() => onStartLeg('far')}>我这就走</button>
          </div>
        </>
      )}

      {leg === 'done' && result && <CalibrationVerdict result={result} onRedo={() => onStartLeg('near')} onClose={onClose} />}
    </ModalShell>
  )
}

function CalMeter({ progress, pct }: { progress: CalibrationProgress | null; pct: number }) {
  return (
    <div className="pk-cal-meter" role="status" aria-live="polite">
      <div className="pk-cal-bar"><span style={{ width: `${pct}%` }} /></div>
      <p className="pk-cal-now">
        {progress?.latestDbm != null
          // The number is deliberately unlabelled and small: it is here so the
          // bar is visibly tied to something real and moves when you move, not
          // so anyone has to know what dBm means (ui-conventions 3.4).
          ? <>正在记 · 现在 <b>{progress.latestDbm}</b> dBm</>
          : '还没收到信号'}
      </p>
    </div>
  )
}

function CalibrationVerdict({ result, onRedo, onClose }: {
  result: CalibrationResult
  onRedo: () => void
  onClose: () => void
}) {
  const { outcome, near, far } = result
  if (outcome.kind === 'ok') {
    return (
      <>
        <p className="modal-intro">
          这台 Mac 现在知道你那个位置该有多强的信号了。走开一会儿它会自己锁，回来按回车就进。
        </p>
        <dl className="pk-cal-stats">
          <div><dt>在身边时</dt><dd>{near.mean.toFixed(0)} dBm（{near.max} 到 {near.min}，{near.n} 次）</dd></div>
          <div><dt>走开之后</dt><dd>{far.mean.toFixed(0)} dBm（{far.max} 到 {far.min}，{far.n} 次）</dd></div>
          <div><dt>判定的分界</dt><dd>强于 {outcome.nearDbm} 算在身边，弱于 {outcome.farDbm} 算走了</dd></div>
        </dl>
        <p className="pk-pair-hint">
          中间那段留空是故意的：正好卡在边上时它保持原样，免得你一动就锁一下、开一下。
        </p>
        <div className="pk-modal-actions"><button className="button primary" onClick={onClose}>好</button></div>
      </>
    )
  }

  // Both failures answer the same first question (ui-conventions 5.3): nothing
  // changed, the Mac is exactly as it was a minute ago.
  const why =
    outcome.kind === 'not-enough-samples'
      ? `两段里至少有一段没收够信号（近处 ${outcome.near} 次、远处 ${outcome.far} 次，各要 ${outcome.needed} 次）。多半是监测中途停了，或者手机没带在身上。`
      : outcome.kind === 'too-brief'
        ? `收到的信号够多，但都挤在很短的时间里（近处 ${Math.round(outcome.nearMs / 1000)} 秒、远处 ${Math.round(outcome.farMs / 1000)} 秒，各要 ${Math.round(outcome.neededMs / 1000)} 秒）。每段都要真的待满那段时间，短时间里的十几次读数其实是同一个瞬间。`
        : `两段测出来太像了，只差 ${Math.abs(outcome.gapDb).toFixed(0)} dB，要 ${outcome.neededDb.toFixed(0)} dB 才分得开。走得再远一点，或者换个位置再试——隔一堵墙通常就够了。`

  return (
    <>
      <p className="modal-intro">
        没有改动任何设置，这台 Mac 还是刚才那样。
      </p>
      <p className="pk-pair-hint">{why}</p>
      <dl className="pk-cal-stats">
        <div><dt>在身边时</dt><dd>{near.n ? `${near.mean.toFixed(0)} dBm（${near.n} 次）` : '没收到'}</dd></div>
        <div><dt>走开之后</dt><dd>{far.n ? `${far.mean.toFixed(0)} dBm（${far.n} 次）` : '没收到'}</dd></div>
      </dl>
      <p className="security-limit" style={{ marginTop: 14 }}>
        {/* 1.3: say what not calibrating actually costs, rather than offering
            「先用默认值」 as if it were a neutral second option. */}
        不量也能用，只是那个距离用的是别人机器上量出来的数——可能你还在座位上它就锁了，
        也可能你走到门口它还当你在。
      </p>
      <div className="pk-modal-actions">
        <button className="button light" onClick={onClose}>先这样</button>
        <button className="button primary" onClick={onRedo}>再量一次</button>
      </div>
    </>
  )
}

// ---------------------------------------------------------------------------
// Who can touch this Mac.
//
// One row today, because one key slot exists. Protocol v3 gives the phone its
// own keyId and this becomes a real list -- the shape is here already so that
// the second phone does not need a new page.
//
// Deliberately NOT here: a per-phone 解锁 switch. With a single phone it would
// be a second control for the decision the switch above already makes, which
// is the duplicate-control bug this page was reorganised to remove. The row
// shows the state that switch produces, and says so.
//
// Also deliberately not here: a 快捷控制 switch. Nothing reads such a flag yet,
// and a switch that stores a preference no code enforces is the same lie as a
// hardcoded `keys_removed: true`.
function PhoneList({ device, onPair, busy, armed, onArm, onRevoke, onCalibrate }: {
  device: PairedDevice | null
  onPair: (() => void) | null
  busy: boolean
  armed: string | null
  onArm: (id: string | null) => void
  onRevoke: (id: string) => void
  onCalibrate: () => void
}) {
  return (
    <section className="panel preferences-panel">
      <div className="section-heading">
        <div><h2>能打开这台 Mac 的</h2><p>{device ? '现在只有它。' : '还没有。'}</p></div>
      </div>

      {device ? (
        <div className={`pk-device${device.canUnlock ? '' : ' is-off'}`}>
          <div className="pk-device-icon"><Smartphone size={19} /></div>
          <div className="pk-device-body">
            <p className="pk-device-name">{device.name}</p>
            <p className="pk-device-state">
              {device.canUnlock ? '可以解锁' : device.blockedReason ?? '现在不能解锁'}
            </p>
            <p className="pk-device-meta">
              {device.paired
                ? `在这台 Mac 上配对${formatPairedAt(device.pairedAt)}`
                : '不是配对来的，是开发时用 USB 装进去的一把钥匙。它一样能开这台 Mac。'}
            </p>
          </div>
          {/* Two steps, because it cannot be undone without the phone in hand
              and a second pairing. The armed step says what is about to go. */}
          <div className="pk-device-action">
            {armed !== device.id && (
              <button className="button light pk-device-tune" disabled={busy} onClick={onCalibrate}>
                量一下距离
              </button>
            )}
            {armed === device.id ? (
              <>
                <p className="pk-device-warn">删掉钥匙之后，这部手机要重新配对一次才能再解锁。</p>
                <span className="pk-revoke-confirm">
                  <button className="text-button" onClick={() => onArm(null)}>算了</button>
                  <button className="text-button danger-text" disabled={busy} onClick={() => onRevoke(device.id)}>
                    删掉这把钥匙
                  </button>
                </span>
              </>
            ) : (
              <button className="text-button" disabled={busy} onClick={() => onArm(device.id)}>删掉这把钥匙</button>
            )}
          </div>
        </div>
      ) : (
        // 6.4: an empty list must answer what this is, not just offer a button.
        <div className="pk-device-empty">
          <p>配一部手机之后，它会出现在这里。配对要两边同时在场，在手机上点一下「一样」才算成功。</p>
          {onPair && <button className="button primary" disabled={busy} onClick={onPair}>配对手机</button>}
        </div>
      )}

      {device && (
        <p className="security-limit" style={{ marginTop: 16 }}>
          再配一部手机、以及给每部手机单独开关，还没做。
        </p>
      )}
    </section>
  )
}

/** "" when unknown, so the sentence simply ends instead of showing a fake date. */
function formatPairedAt(iso: string): string {
  if (!iso) return ''
  const t = new Date(iso)
  if (Number.isNaN(t.getTime())) return ''
  return ` · ${t.getFullYear()} 年 ${t.getMonth() + 1} 月 ${t.getDate()} 日`
}

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
// Four steps, not five: the design's ④ 校准 is not built yet, and a step a
// person cannot finish is worse than no steps at all (ui-conventions 6.3).
// It slots in here the day it exists.
const PAIR_STEPS: { key: PairingSession['stage']; label: string }[] = [
  { key: 'scanning', label: '找到手机' },
  { key: 'compare', label: '核对数字' },
  { key: 'waiting-for-phone', label: '手机确认' },
  { key: 'done', label: '锁屏验证' },
]

function PairSteps({ stage }: { stage: PairingSession['stage'] }) {
  const at = PAIR_STEPS.findIndex(s => s.key === stage)
  return (
    <ol className="pk-steps" aria-label={`第 ${at + 1} 步，共 ${PAIR_STEPS.length} 步`}>
      {PAIR_STEPS.map((s, i) => (
        <li
          key={s.key}
          className={i < at ? 'is-done' : i === at ? 'is-now' : ''}
          aria-current={i === at ? 'step' : undefined}
        >
          <span className="pk-step-dot">{i < at ? '✓' : i + 1}</span>
          <span className="pk-step-label">{s.label}</span>
        </li>
      ))}
    </ol>
  )
}

function PairingSheet(
  { session, onClose, onConfirm, onRetry, onTryLock, onCalibrate }:
  { session: PairingSession; onClose: () => void; onConfirm: () => void; onRetry: () => void
    onTryLock: () => void; onCalibrate: () => void },
) {
  const title = session.stage === 'done' ? '配好了，还差一次验证'
    : session.stage === 'failed' ? '配对没有完成'
    : session.stage === 'waiting-for-phone' ? '还差手机上那一下'
    : session.stage === 'compare' ? '核对这六位数字'
    : '正在找你的手机'

  return (
    <ModalShell label={title} onClose={onClose} className="phone-key-modal pk-pair-modal">
      <button className="modal-close icon-button" aria-label="关闭" onClick={onClose}><X size={21} /></button>
      <h2>{title}</h2>
      {/* Numbered because this really is a sequence, and it is the only
          numbered thing in the panel (ui-conventions 6.3). It exists because
          the flow makes you walk between two devices: without it there is no
          way to tell whether you are one step from done or halfway. */}
      {session.stage !== 'failed' && <PairSteps stage={session.stage} />}

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
          <p className="modal-intro">手机上现在也应该显示这串数字。两边都要点一下，先点哪边都行。</p>
          <p className="pk-pair-digits" aria-label={`配对数字 ${session.digits?.split('').join(' ')}`}>
            {session.digits}
          </p>
          <p className="pk-pair-hint">
            两边一样，就说明中间没有人冒充——这一眼是整个配对唯一的安全保障。
            不一样就按「不一样」，然后换个地方重新配一次。
          </p>
          <p className="pk-pair-hint">
            为什么两台都要点：任何「对方已确认」的消息都要走无线，而中间人能把它拆开重发。
            你的手指是两台设备之间唯一伪造不了的通道。
          </p>
          <div className="pk-modal-actions">
            <button className="button primary" onClick={onConfirm}>和手机上一样</button>
            <button className="button light" onClick={onClose}>不一样，停下</button>
          </div>
        </>
      )}

      {session.stage === 'waiting-for-phone' && (
        <>
          <p className="modal-intro">{session.detail ?? '这台 Mac 已经记下了。'}</p>
          <div className="pk-pair-waiting" role="status" aria-live="polite">
            <span className="pk-pair-dot" /><span className="pk-pair-dot" /><span className="pk-pair-dot" />
          </div>
          {/* Not a formality being waited out. The Mac cannot be told that the
              phone confirmed -- a man in the middle would simply say it did --
              so it waits to HEAR the phone sign something with the new key.
              That is why this screen exists instead of a tick. */}
          <p className="pk-pair-hint">
            在手机上点「一样，完成配对」。这里会在听到手机用上新钥匙时自己变成完成——
            <b>Mac 不会听信「手机已确认」这种消息</b>，中间人也能那么说。
          </p>
          <div className="pk-modal-actions">
            <button className="button light" onClick={onClose}>先关掉</button>
          </div>
        </>
      )}

      {session.stage === 'done' && (
        <>
          <p className="modal-intro">{session.detail ?? '这台 Mac 已经认得你的手机了。'}</p>
          {/* What pairing proved is that the two devices share a key. Whether
              macOS actually honours it on the lock screen is a different fact,
              and it has not been checked yet -- so this does not promise it.
              The old copy said 「以后锁屏时…直接按回车就能进」 before anything had
              tried. */}
          <p className="pk-pair-hint">
            还剩一件事：锁一次屏，确认 macOS 真的会放行。没试过之前，别把密码忘了。
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
            <button className="button light" onClick={onCalibrate}>先量一下距离</button>
            <button className="button primary" onClick={onTryLock}>现在锁屏试一次</button>
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
    <ModalShell label="把这台 Mac 改回原样" onClose={onClose} className="phone-key-modal">
      <button className="modal-close icon-button" aria-label="关闭" onClick={onClose}><X size={21} /></button>
      <h2>把这台 Mac 改回原样</h2>
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
