// Phone Key (手机钥匙) state model — the front-end half of the §6 contract in
// docs/plans/2026-09-09-unlock-interaction-design.md.
//
// This is a clean rewrite (not a port of the codex-branch unlock.ts, whose
// schema — capability/authorizationGate — is incompatible with §6). It keeps
// three engineering handles the old one got right:
//   - normalizeUnlockSnapshot(): the native/IPC side is treated as UNTRUSTED
//     input; anything malformed or from a newer schema collapses to a safe
//     state rather than rendering a guessed one.
//   - a single-in-flight request reducer (begin/finish/fail).
//   - a revocation confirm/cancel reducer.
//
// Pure functions only — no React, no window — so it runs under `node --test`.

// ---- §6.1 error contract -------------------------------------------------

export type UnlockErrorCode =
  | 'preflight-rule-shape'      // F3
  | 'authorization-denied'     // F4
  | 'install-failed'           // F1 write failed
  | 'half-installed'           // F5
  | 'component-not-loaded'     // F1
  | 'rule-tampered'            // F2
  | 'backup-missing'           // F7
  | 'pairing-expired'
  | 'pairing-mismatch'
  | 'calibration-overlap'      // F18
  | 'key-mismatch'             // F17
  | 'bluetooth-off'
  | 'bluetooth-unauthorized'
  | 'phone-app-stopped'
  | 'phone-unseen'
  | 'daemon-unavailable'
  | 'drill-not-observed'       // a drill timing out is NOT a failure
  | 'unsupported'              // !window.repose / non-macOS

export type PhoneHint = 'bluetooth-off' | 'app-stopped' | 'battery' | 'unseen'

// §4.3 seven primitives + R0. `leave-it-alone` renders as a sentence, no button.
export type Remediation =
  | { kind: 'reinstall-component' }                 // R1
  | { kind: 'repair-rule' }                         // R2
  | { kind: 're-pair' }                             // R3
  | { kind: 're-calibrate' }                        // R4
  | { kind: 'fix-on-phone'; hint: PhoneHint }       // R5
  | { kind: 'revoke-device' }                       // R6
  | { kind: 'uninstall-and-restore' }               // R7
  | { kind: 'leave-it-alone' }                      // R0

export type Evidence = { expected: string; actual: string; readAt: string }

export type UnlockError = {
  code: UnlockErrorCode
  detail: string
  remediation?: Remediation
  evidence?: Evidence
}

// ---- §6.3 snapshot -------------------------------------------------------

export type UnlockState =
  | 'not-installed'
  | 'installing'
  | 'half-installed'
  | 'awaiting-password-drill'
  | 'awaiting-pairing'
  | 'awaiting-calibration'
  | 'awaiting-verification'
  | 'ready'
  | 'needs-repair'
  | 'paused'
  | 'uninstalling'

export type Presence = 'near' | 'away' | 'transport-unavailable'
export type RuleVariant = 'A' | 'B' | null
export type ComponentId = 'rule' | 'component' | 'daemon' | 'transport'
export type Health = 'ok' | 'degraded' | 'broken' | 'unknown'

export type UnlockComponent = {
  id: ComponentId
  health: Health
  detail: string
  evidence?: Evidence
  remediation?: Remediation
}

export type ComponentInvocation =
  | { kind: 'observed'; at: string }
  | { kind: 'never-observed' }

export type PairedDevice = {
  id: string
  name: string
  platform: string
  pairedAt: string
  lastSeenMs: number | null
}

export type LastFailure = {
  at: string
  cause: 'not-invoked' | 'phone-absent' | 'phone-slow' | 'allowed-but-locked'
  shown: boolean
}

export type UnlockSnapshot = {
  readAt: string
  state: UnlockState
  presence: Presence
  variant: RuleVariant
  components: UnlockComponent[]
  componentInvocation: ComponentInvocation
  device: PairedDevice | null
  stats: { unlocksToday: number; lastUnlockAt: string | null }
  lastFailure: LastFailure | null
  macosBuild: string
  componentVersion: string
}

// The safe state we collapse to whenever we cannot trust what the backend sent.
// "unsupported" is what !window.repose renders; a malformed snapshot from a real
// backend collapses to needs-repair (something is wrong, offer the password path,
// don't pretend it's ready).
export const UNSUPPORTED_SNAPSHOT: UnlockSnapshot = Object.freeze({
  readAt: '',
  state: 'not-installed',
  presence: 'transport-unavailable',
  variant: null,
  components: [],
  componentInvocation: { kind: 'never-observed' as const },
  device: null,
  stats: { unlocksToday: 0, lastUnlockAt: null },
  lastFailure: null,
  macosBuild: '',
  componentVersion: '',
})

const UNLOCK_STATES: readonly UnlockState[] = [
  'not-installed', 'installing', 'half-installed', 'awaiting-password-drill',
  'awaiting-pairing', 'awaiting-calibration', 'awaiting-verification',
  'ready', 'needs-repair', 'paused', 'uninstalling',
]
const PRESENCES: readonly Presence[] = ['near', 'away', 'transport-unavailable']
const HEALTHS: readonly Health[] = ['ok', 'degraded', 'broken', 'unknown']
const COMPONENT_IDS: readonly ComponentId[] = ['rule', 'component', 'daemon', 'transport']

function str(value: unknown, max = 4000): string | null {
  return typeof value === 'string' && value.length <= max ? value : null
}
function oneOf<T extends string>(value: unknown, allowed: readonly T[]): T | null {
  return typeof value === 'string' && (allowed as readonly string[]).includes(value) ? (value as T) : null
}

function normalizeEvidence(value: unknown): Evidence | undefined {
  if (!value || typeof value !== 'object') return undefined
  const e = value as Record<string, unknown>
  const expected = str(e.expected), actual = str(e.actual), readAt = str(e.readAt)
  if (expected === null || actual === null || readAt === null) return undefined
  return { expected, actual, readAt }
}

function normalizeComponent(value: unknown): UnlockComponent | null {
  if (!value || typeof value !== 'object') return null
  const c = value as Record<string, unknown>
  const id = oneOf<ComponentId>(c.id, COMPONENT_IDS)
  const health = oneOf<Health>(c.health, HEALTHS)
  const detail = str(c.detail)
  if (id === null || health === null || detail === null) return null
  return { id, health, detail, evidence: normalizeEvidence(c.evidence) }
}

/**
 * Treat the backend's snapshot as untrusted. A snapshot missing required fields,
 * carrying an unknown `state`, or otherwise from a schema we don't recognise
 * collapses to `fallback` (default: needs-repair) so the UI never renders a
 * guessed "ready". This is the front-end mirror of "only show what we read".
 */
export function normalizeUnlockSnapshot(
  value: unknown,
  fallback: UnlockState = 'needs-repair',
): UnlockSnapshot {
  if (!value || typeof value !== 'object') {
    return { ...UNSUPPORTED_SNAPSHOT, state: fallback }
  }
  const s = value as Record<string, unknown>
  const state = oneOf<UnlockState>(s.state, UNLOCK_STATES)
  const readAt = str(s.readAt)
  if (state === null || readAt === null) {
    return { ...UNSUPPORTED_SNAPSHOT, state: fallback }
  }

  const presence = oneOf<Presence>(s.presence, PRESENCES) ?? 'transport-unavailable'
  const variant = oneOf<Exclude<RuleVariant, null>>(s.variant, ['A', 'B'] as const) ?? null

  const components = Array.isArray(s.components)
    ? s.components.map(normalizeComponent).filter((c): c is UnlockComponent => c !== null)
    : []

  // componentInvocation: only `observed` with a timestamp counts. Anything else
  // — including a malformed observed — becomes never-observed (no green dot).
  let componentInvocation: ComponentInvocation = { kind: 'never-observed' }
  const ci = s.componentInvocation
  if (ci && typeof ci === 'object' && (ci as Record<string, unknown>).kind === 'observed') {
    const at = str((ci as Record<string, unknown>).at)
    if (at !== null) componentInvocation = { kind: 'observed', at }
  }

  let device: PairedDevice | null = null
  if (s.device && typeof s.device === 'object') {
    const d = s.device as Record<string, unknown>
    const id = str(d.id), name = str(d.name), platform = str(d.platform), pairedAt = str(d.pairedAt)
    if (id && name && platform && pairedAt) {
      const lastSeenMs = typeof d.lastSeenMs === 'number' && Number.isFinite(d.lastSeenMs) ? d.lastSeenMs : null
      device = { id, name, platform, pairedAt, lastSeenMs }
    }
  }

  const stats = {
    unlocksToday: typeof (s.stats as Record<string, unknown>)?.unlocksToday === 'number'
      ? Math.max(0, Math.floor((s.stats as { unlocksToday: number }).unlocksToday)) : 0,
    lastUnlockAt: str((s.stats as Record<string, unknown>)?.lastUnlockAt) ?? null,
  }

  let lastFailure: LastFailure | null = null
  if (s.lastFailure && typeof s.lastFailure === 'object') {
    const f = s.lastFailure as Record<string, unknown>
    const at = str(f.at)
    const cause = oneOf(f.cause, ['not-invoked', 'phone-absent', 'phone-slow', 'allowed-but-locked'] as const)
    if (at && cause) lastFailure = { at, cause, shown: f.shown === true }
  }

  return {
    readAt, state, presence, variant, components, componentInvocation, device,
    stats, lastFailure,
    macosBuild: str(s.macosBuild) ?? '',
    componentVersion: str(s.componentVersion) ?? '',
  }
}

// ---- derived view --------------------------------------------------------

export type StatusTone = 'ready' | 'neutral' | 'attention'

export type UnlockView = {
  // The single primary action for this state (§3.4 invariant 3: exactly one,
  // named as a verb). null when the state has no primary action (e.g. ready).
  primaryAction: { verb: string; command: PanelCommand } | null
  // The panel-top status line (§5.10 axis C). Never a warning colour for `away`.
  status: { tone: StatusTone; main: string; sub?: string }
  // Whether a green "ready" dot is allowed. False unless truly observed-ready.
  showReadyDot: boolean
  // Human label for the header badge state.
  headline: string
}

// Commands the panel can ask the container to run (mapped to bridge calls).
export type PanelCommand =
  | 'install' | 'repair-rule' | 'reinstall-component' | 'uninstall'
  | 'start-password-drill' | 'start-phone-drill' | 'begin-pairing' | 'calibrate'
  | 'resume' | 'open-bluetooth-settings'

export function deriveUnlockView(snapshot: UnlockSnapshot): UnlockView {
  switch (snapshot.state) {
    case 'not-installed':
      return {
        primaryAction: { verb: '开始设置', command: 'install' },
        status: { tone: 'neutral', main: '还没开启', sub: '手机在身边时，回车就是你的密码。' },
        showReadyDot: false,
        headline: '未开启',
      }
    case 'installing':
      return { primaryAction: null, status: { tone: 'neutral', main: '正在安装…' }, showReadyDot: false, headline: '安装中' }
    case 'half-installed':
      return {
        primaryAction: { verb: '继续完成', command: 'install' },
        status: { tone: 'attention', main: '上次的安装没有完成' },
        showReadyDot: false, headline: '安装未完成',
      }
    case 'awaiting-password-drill':
      return {
        primaryAction: { verb: '锁屏，我用密码进来', command: 'start-password-drill' },
        status: { tone: 'neutral', main: '先用密码进来一次', sub: '确认退路是通的，再试手机。' },
        showReadyDot: false, headline: '待演练退路',
      }
    case 'awaiting-pairing':
      return {
        primaryAction: { verb: '配对手机', command: 'begin-pairing' },
        status: { tone: 'neutral', main: '让这台 Mac 认识你的手机' },
        showReadyDot: false, headline: '待配对',
      }
    case 'awaiting-calibration':
      return {
        primaryAction: { verb: '开始采样', command: 'calibrate' },
        status: { tone: 'neutral', main: '教它分辨「在身边」和「不在」' },
        showReadyDot: false, headline: '待校准',
      }
    case 'awaiting-verification':
      return {
        primaryAction: { verb: '锁屏，试一次', command: 'start-phone-drill' },
        status: { tone: 'attention', main: '还没试过', sub: '锁一次屏，确认 macOS 真的在用它。' },
        showReadyDot: false, headline: '待验证',
      }
    case 'ready':
      return deriveReadyView(snapshot)
    case 'needs-repair':
      return {
        primaryAction: deriveRepairAction(snapshot),
        status: { tone: 'attention', main: '需要修复', sub: '密码始终照常可用。' },
        showReadyDot: false, headline: '需要修复',
      }
    case 'paused':
      return {
        primaryAction: { verb: '恢复', command: 'resume' },
        status: { tone: 'neutral', main: '已暂停', sub: '锁屏回到输密码。配对还留着，随手就能再打开。' },
        showReadyDot: false, headline: '已暂停',
      }
    case 'uninstalling':
      return { primaryAction: null, status: { tone: 'neutral', main: '正在移除…' }, showReadyDot: false, headline: '移除中' }
  }
}

function deriveReadyView(snapshot: UnlockSnapshot): UnlockView {
  const name = snapshot.device?.name ?? '你的手机'
  // §3.4 invariant 2: only "near" is a green dot. "away" is neutral, not a warning.
  if (snapshot.presence === 'near') {
    return {
      primaryAction: null,
      status: { tone: 'ready', main: '可以用了', sub: `刚刚看到 ${name}` },
      // green dot only if the mechanism has actually been observed running.
      showReadyDot: snapshot.componentInvocation.kind === 'observed',
      headline: '可以用了',
    }
  }
  if (snapshot.presence === 'away') {
    return {
      primaryAction: null,
      status: { tone: 'neutral', main: '手机不在附近', sub: '现在解锁需要密码，这是正常的。手机回来会自动恢复。' },
      showReadyDot: false, headline: '手机不在',
    }
  }
  // transport-unavailable — bluetooth off / no permission. Offer the fix.
  return {
    primaryAction: { verb: '打开蓝牙', command: 'open-bluetooth-settings' },
    status: { tone: 'neutral', main: '这台 Mac 的蓝牙关着' },
    showReadyDot: false, headline: '传输不可用',
  }
}

function deriveRepairAction(snapshot: UnlockSnapshot): UnlockView['primaryAction'] {
  // Priority per §3.5: fix the lower layer first (rule > component > daemon).
  const broken = COMPONENT_IDS
    .map(id => snapshot.components.find(c => c.id === id))
    .find(c => c && (c.health === 'broken' || c.health === 'degraded'))
  if (broken?.id === 'rule') return { verb: '修复规则', command: 'repair-rule' }
  if (broken?.id === 'component') return { verb: '重新安装组件', command: 'reinstall-component' }
  return { verb: '重新安装组件', command: 'reinstall-component' }
}

// ---- global banner (§4.4 / §5.15) ----------------------------------------
// The banner is deliberately RARE: only for "pressing Return will actually fail
// and you need to act". Never for `away`, never for `paused`, never for a
// healthy `ready`. This is the intentional divergence from the idle-lock banner.

export type GlobalBanner = {
  tone: 'attention' | 'danger'
  title: string
  body: string
  action: { label: string; command: PanelCommand }
}

export function deriveGlobalBanner(snapshot: UnlockSnapshot): GlobalBanner | null {
  // The one place red is allowed: a dangling reference (rule present, component
  // gone) is the fail-open emergency.
  //
  // This used to be titled 「检测到异常，已自动恢复到纯密码解锁」 and to tell the
  // user 「Outsie 的后台守护已把规则改回只认密码，一切安全」. That was false, and
  // false in the worst possible direction. This snapshot is the app looking at
  // the rule *right now* and finding it still pointing at a component that is
  // not there — which is precisely the state the daemon would have removed had
  // it repaired anything. Claiming the repair as done, at the one moment the Mac
  // opens for anybody, told the user to relax exactly when they should not.
  //
  // The daemon may also simply not be running; that is a state this panel now
  // reports separately. So the daemon is mentioned only when it is loaded, and
  // then only as something that should act shortly — never as something that
  // already has.
  const ruleBroken = snapshot.components.find(c => c.id === 'rule' && c.health === 'broken')
  const componentGone = snapshot.components.find(c => c.id === 'component' && c.health === 'broken')
  if (componentGone && snapshot.state === 'needs-repair') {
    const daemon = snapshot.components.find(c => c.id === 'daemon')
    const guarded = daemon?.health === 'ok'
    return {
      tone: 'danger',
      title: '现在这台 Mac 可能不用密码就能进',
      body: guarded
        ? '解锁组件不在了，而锁屏规则还指着它 —— macOS 会把装不上的这一步当作已通过。'
          + '后台守护正在运行，应该很快会把规则改回只认密码，但此刻还没有改回来。'
          + '现在就修复，或者先卸载。'
        : '解锁组件不在了，而锁屏规则还指着它 —— macOS 会把装不上的这一步当作已通过。'
          + '本该自动修复的后台守护没有在运行，所以不会有人替你改回来。请立即修复或卸载。',
      action: { label: '立即修复', command: 'reinstall-component' },
    }
  }

  switch (snapshot.state) {
    case 'needs-repair':
      return {
        tone: 'attention',
        title: '手机钥匙需要修复',
        body: ruleBroken
          ? '锁屏授权规则被别的程序改动了。现在解锁需要输密码，其他一切正常。'
          : 'macOS 没有加载解锁组件。现在解锁需要输密码，其他一切正常。',
        action: { label: '前往设置', command: 'reinstall-component' },
      }
    case 'awaiting-verification':
      return {
        tone: 'attention',
        title: '手机钥匙还没试过',
        body: '组件已经装好，但还没确认 macOS 真的在用它。锁一次屏就能知道。',
        action: { label: '现在试一次', command: 'start-phone-drill' },
      }
    case 'awaiting-password-drill':
      return {
        tone: 'attention',
        title: '手机钥匙还没有完成安装',
        body: '组件已经装好，但还没有演练过密码解锁。在完成这一步之前，我们不会开启它。',
        action: { label: '继续安装', command: 'start-password-drill' },
      }
    case 'half-installed':
      return {
        tone: 'attention',
        title: '上次的安装没有完成',
        body: '手机钥匙的安装中途停下了。可以继续完成，也可以清理干净。',
        action: { label: '前往设置', command: 'install' },
      }
    default:
      return null
  }
}

// ---- single-in-flight request reducer ------------------------------------
// Only one backend request runs at a time; the panel disables its controls
// while `pending` is set. `error` carries the last UnlockError for display.

export type RequestState = {
  pending: PanelCommand | null
  sequence: number
  error: UnlockError | null
}

export const INITIAL_REQUEST_STATE: RequestState = { pending: null, sequence: 0, error: null }

export function beginRequest(state: RequestState, command: PanelCommand): RequestState {
  return { pending: command, sequence: state.sequence + 1, error: null }
}
export function finishRequest(state: RequestState, sequence: number): RequestState {
  if (sequence !== state.sequence) return state // a newer request superseded this one
  return { ...state, pending: null }
}
export function failRequest(state: RequestState, sequence: number, error: UnlockError): RequestState {
  if (sequence !== state.sequence) return state
  return { ...state, pending: null, error }
}
export function canIssue(state: RequestState): boolean {
  return state.pending === null
}

// ---- revocation confirmation reducer -------------------------------------

export type RevocationState = { armedDeviceId: string | null }
export const INITIAL_REVOCATION: RevocationState = { armedDeviceId: null }

export function reduceRevocationConfirmation(
  _state: RevocationState,
  action: { type: 'arm'; deviceId: string } | { type: 'cancel' } | { type: 'confirm' },
): RevocationState {
  switch (action.type) {
    case 'arm': return { armedDeviceId: action.deviceId }
    case 'cancel': return { armedDeviceId: null }
    case 'confirm': return { armedDeviceId: null }
  }
}

// ---- the desktop bridge interface ----------------------------------------
// What window.repose.unlock exposes (implemented in tauriBridge.ts). Each method
// maps to a Tauri command in src-tauri/src/unlock.rs (§6.2); returns the fresh
// snapshot (as unknown — the panel re-normalizes it) or throws an UnlockError.

export type UnlockDesktopBridge = {
  getSnapshot: () => Promise<unknown>
  preflight: () => Promise<unknown>
  /** Start or stop presence monitoring. Raises one administrator prompt on start. */
  setPresenceRunning: (value: { enabled: boolean }) => Promise<unknown>
  install: (value: { variant: 'A' | 'B' | null }) => Promise<unknown>
  repair: (value: { target: 'rule' | 'component' | 'daemon' }) => Promise<unknown>
  uninstall: () => Promise<unknown>
  setEnabled: (value: { enabled: boolean }) => Promise<unknown>
  revokeDevice: (value: { deviceId: string }) => Promise<unknown>
  beginPairing: () => Promise<unknown>
  /** Where the live exchange has got to. Polled while the sheet is open. */
  pollPairing: () => Promise<unknown>
  /** The human says the six digits match. The only path that writes a key. */
  confirmPairing: () => Promise<unknown>
  cancelPairing: () => Promise<void>
  calibrateSample: (value: { kind: 'near' | 'far' }) => Promise<unknown>
  startDrill: (value: { kind: 'password-drill' | 'phone-drill' }) => Promise<unknown>
  openBluetoothSettings: () => Promise<void>
  onSnapshot: (cb: (snapshot: unknown) => void) => () => void
  onPresence: (cb: (presence: unknown) => void) => () => void
}

// The button-label mapping for each Remediation primitive (§4.3). Exactly one
// per variant — a closed set, no eighth button. `leave-it-alone` has no button.
export function remediationLabel(remediation: Remediation): string | null {
  switch (remediation.kind) {
    case 'reinstall-component': return '重新安装组件'
    case 'repair-rule': return '修复规则'
    case 're-pair': return '重新配对'
    case 're-calibrate': return '重做校准'
    case 'fix-on-phone': return '在手机上打开'
    case 'revoke-device': return '撤销这台设备'
    case 'uninstall-and-restore': return '移除手机钥匙并还原系统设置'
    case 'leave-it-alone': return null
  }
}


/**
 * The CSS class a component's health renders as.
 *
 * Lives here rather than in the panel so a test can check the other half of the
 * contract: every class this can return must have a rule in phone-key.css. It
 * returned 'bad' for a broken component from the start, and nothing styled it,
 * so the row that means "this Mac may open with no password right now" was
 * drawn in the same colour as "已就位".
 */
export function healthClass(health: string): string {
  return health === 'ok' ? 'good' : health === 'broken' ? 'bad' : health === 'degraded' ? 'warn' : ''
}


// ---- preflight -----------------------------------------------------------

export type PreflightReport = {
  variant: RuleVariant
  canInstall: boolean
  ruleNow: string
  /**
   * Third-party mechanisms already in this Mac's lock-screen rule.
   *
   * Empty on an untouched machine. Non-empty means someone else's authorization
   * plugin is already in the unlock path -- with k-of-n = 1, each entry can grant
   * an unlock on its own, so adding ours adds one more door rather than a second
   * lock. That is a fact about the user's Mac, not about Outsie, and the install
   * sheet has to say it before they agree to anything.
   */
  foreign: string[]
}

/** Untrusted like everything else from the backend: anything odd becomes "cannot install". */
export function normalizePreflight(value: unknown): PreflightReport {
  const safe: PreflightReport = { variant: null, canInstall: false, ruleNow: '', foreign: [] }
  if (!value || typeof value !== 'object') return safe
  const p = value as Record<string, unknown>
  return {
    variant: oneOf<Exclude<RuleVariant, null>>(p.variant, ['A', 'B'] as const) ?? null,
    canInstall: p.canInstall === true,
    ruleNow: str(p.ruleNow) ?? '',
    // Cap the list: it is rendered, and a backend returning thousands of entries
    // should not become an unclosable modal.
    foreign: Array.isArray(p.foreign)
      ? p.foreign.filter((e): e is string => typeof e === 'string' && e.length <= 200).slice(0, 12)
      : [],
  }
}

// ---- Pairing (repose-pair-v2) ----------------------------------------------

export type PairingStage = 'idle' | 'scanning' | 'compare' | 'done' | 'failed'

export type PairingSession = {
  stage: PairingStage
  /** The six SAS digits. Only ever present in `compare`. */
  digits: string | null
  /** The paired key's short fingerprint. Only in `done`. Behind 技术细节. */
  fingerprint: string | null
  /** What the phone calls itself. Cosmetic — see the Rust side. */
  peerName: string | null
  detail: string | null
}

export const IDLE_PAIRING: PairingSession = {
  stage: 'idle', digits: null, fingerprint: null, peerName: null, detail: null,
}

/**
 * Normalize a pairing status from the backend.
 *
 * Stricter than the other normalizers on one point: digits are only kept when
 * they are exactly six ASCII digits AND the stage is `compare`. Those digits
 * are the entire man-in-the-middle defence — a person compares them against
 * their phone and presses 一样 — so anything the panel is not certain of must
 * not be rendered as them. Unparseable input becomes a failure, never a
 * comparison the user might answer.
 */
export function normalizePairing(value: unknown): PairingSession {
  if (!value || typeof value !== 'object') {
    return { stage: 'failed', digits: null, fingerprint: null, peerName: null, detail: '配对没有完成。' }
  }
  const p = value as Record<string, unknown>
  const stage = oneOf<PairingStage>(p.stage, ['idle', 'scanning', 'compare', 'done', 'failed'] as const)
  if (!stage) {
    return { stage: 'failed', digits: null, fingerprint: null, peerName: null, detail: '配对没有完成。' }
  }
  const rawDigits = str(p.digits)
  const digits = stage === 'compare' && rawDigits && /^\d{6}$/.test(rawDigits) ? rawDigits : null
  // A comparison stage with nothing to compare is not a comparison.
  if (stage === 'compare' && !digits) {
    return { stage: 'failed', digits: null, fingerprint: null, peerName: null, detail: '没有读到要核对的数字，请重新配对。' }
  }
  const fingerprint = str(p.fingerprint)
  return {
    stage,
    digits,
    fingerprint: stage === 'done' && fingerprint && /^[0-9A-F]{8}$/.test(fingerprint) ? fingerprint : null,
    // Rendered as a device name, so it is stripped of anything that is not one.
    // It arrives from a stranger over a radio and identifies nobody.
    peerName: str(p.peerName, 60)?.replace(/[\u0000-\u001f]/g, '').trim() || null,
    detail: str(p.detail)?.slice(0, 400) ?? null,
  }
}

// ---- Uninstall ------------------------------------------------------------

/**
 * What a removal actually read back off the machine.
 *
 * Every field here was once a literal `true` in the Rust, which is how this
 * project once reported `keys_removed: true` over a key that was still on
 * disk. They are readings now, and the panel shows them rather than
 * summarising — "已移除" is a claim; a list of what is and is not still there
 * is evidence.
 */
export type UninstallSummary = {
  ruleNow: string
  backupUsed: boolean
  rightRemoved: boolean
  bundleRemoved: boolean
  keysRemoved: boolean
  residual: string[]
}

export function normalizeUninstall(value: unknown): UninstallSummary {
  const safe: UninstallSummary = {
    ruleNow: '', backupUsed: false, rightRemoved: false,
    bundleRemoved: false, keysRemoved: false, residual: [],
  }
  if (!value || typeof value !== 'object') return safe
  const r = value as Record<string, unknown>
  return {
    ruleNow: str(r.ruleNow)?.slice(0, 400) ?? '',
    backupUsed: r.backupUsed === true,
    rightRemoved: r.rightRemoved === true,
    bundleRemoved: r.bundleRemoved === true,
    keysRemoved: r.keysRemoved === true,
    residual: Array.isArray(r.residual)
      ? r.residual.filter((e): e is string => typeof e === 'string' && e.length <= 200).slice(0, 12)
      : [],
  }
}

// ---- Panel modes ----------------------------------------------------------

/**
 * Which of three jobs the panel is doing right now.
 *
 * The panel used to do all three at once, in one flat list at one weight:
 * a setup sequence you run once, a status you glance at daily, and diagnostics
 * you only want when something is broken. Rendering them together is what made
 * it impossible to tell what you were supposed to look at — and it is the
 * shared cause behind a button that could only fail, a state with no way out,
 * and two controls for one decision.
 */
export type PanelMode = 'setup' | 'daily' | 'repair'

export function panelMode(state: UnlockState): PanelMode {
  switch (state) {
    case 'ready':
    case 'paused':
      return 'daily'
    case 'needs-repair':
    // Removal is a system-changing operation, not part of getting set up, and
    // its panel shows the same two things a repair does: what is happening to
    // the machine, and what that means for getting in.
    case 'uninstalling':
      return 'repair'
    default:
      return 'setup'
  }
}

/**
 * Which step of the setup a state sits at, or null when setup is over.
 *
 * Setup is the only genuinely ordered part of this feature, which is why it is
 * the only place in the panel that carries numbers. Numbering anything else
 * would be decoration claiming to be structure.
 */
export function setupStep(state: UnlockState): 1 | 2 | 3 | null {
  switch (state) {
    case 'not-installed':
    case 'installing':
    case 'half-installed':
      return 1
    case 'awaiting-pairing':
      return 2
    case 'awaiting-password-drill':
    case 'awaiting-calibration':
    case 'awaiting-verification':
      return 3
    default:
      return null
  }
}

export const SETUP_STEPS = ['安装', '配对', '试一次'] as const

/**
 * Whether the switch should exist at all.
 *
 * A control for something that is not installed is a second way to start an
 * installation, sitting next to the button that already does that. Both opened
 * the same disclosure, so a reader had to stop and work out whether they
 * differed. They did not.
 */
export function showsSwitch(state: UnlockState): boolean {
  return state !== 'not-installed' && state !== 'installing'
}
