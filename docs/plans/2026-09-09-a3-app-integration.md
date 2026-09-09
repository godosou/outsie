# A3 · 把已验证的后端接进现有 App UI（手机钥匙落地实现计划）

日期：2026-09-09 · 分支 `feat/phone-unlock-walking-skeleton` · 状态：apply-ready 实现计划

## 这份文档解决的问题

用户的判断是对的：**「之前的 UI 是好的，只是功能从来没接上」。** 但要精确：

1. 现有工作分支 `feat/phone-unlock-walking-skeleton` 的 `src/App.tsx` / `src/tauriBridge.ts` 里**一行手机钥匙代码都没有**（grep `unlock|phone|proximity|ble|permit` 零命中）。
2. 「之前的 UI」在 `codex/phone-proximity-unlock`（commit `c960e27`）和 `codex/phone-work-console`（`.worktrees/phone-work-console/`）两条分支上，是一个自洽的设置页面板 `src/components/UnlockSettingsPanel.tsx` + 状态模型 `src/lib/unlock.ts`，已经写好、有测试、有 CSS，并且已经按 `bridge={window.repose?.unlock}` 的契约接线——**但它接的是一个占位后端**（`CLOSED_UNLOCK_SNAPSHOT`，就是交互设计里点名要替换的 `GateClosedBackend`）。
3. 本 session 验证通过的后端（`native/macos/minimal-auth-plugin/` + `tools/ble-spike/`）是 shell/C，**至今没有被任何 Rust 或前端代码引用**。A3 就是这座从没建过的桥。

所以 A3 = **①把旧面板的外壳搬到当前分支 → ②把它的状态模型升级到交互设计 §6 的新契约 → ③在 Rust 侧写一个真正 shell 进已验证脚本的后端替换 `GateClosedBackend` → ④用 sidecar + LaunchAgent 把 BLE 扫描跑起来 → ⑤把特权安装接成一次 macOS 原生授权。**

三份权威输入：
- 交互设计：`docs/plans/2026-09-09-unlock-interaction-design.md`（状态机 S0–S10、`UnlockError{code,detail,remediation,evidence}`、七原语 R1–R7+R0、门禁 G1–G7、前端约束 §7、后端契约 §6）。
- App 现状：`docs/plans/2026-09-09-repose-app-survey.md`。
- 宿主机可行性：`docs/plans/2026-09-09-host-mac-feasibility.md`（拓扑、分阶段路径、root-permit vs BT-TCC 的角色冲突）。

---

## 0. 先讲最大的那个决定（给用户）

**这个功能要在你的日常主力 Mac 上装一个 Authorization Plugin。** 不是在别的机器上、不是在服务里——是在你每天登录的这台机器的锁屏授权链里插一条规则、往 `/Library/Security/SecurityAgentPlugins/` 放一个 bundle。spike 只在一个可丢弃、可回滚的 VM 上验证过（`install.sh:8` 自己写着 "Throwaway experiment. Run only on a disposable, rollback-capable test VM"）。

它为什么可以做：A1 已验证 **ad-hoc 签名的第三方插件在 SIP 开启的 macOS 14.6.1 上能被加载**（`docs/validation/2026-09-09-a1-plugin-load.md`），SecurityAgentPlugins 不需要关 SIP。E12 健康检查守护已真机验证，bundle 消失后约 4 秒自动退回纯密码（`docs/validation/2026-09-09-e12-healthcheck.md`）。

它的真实风险（按严重度）：
- **E3 fail-open**：规则在、bundle 不在（拖进废纸篓 / 升级中途）时锁屏 **fail-open，空密码可绕过**（`docs/validation/2026-09-09-e3-fail-open.md`）。E8/E11 证明**没有任何规则形态能既免密又缺失即 fail-closed**——只能靠 E12 看门狗预防悬空引用。看门狗关闭的是「bundle 消失→修复」之间数秒的窗口，但**在那几秒里如果机器正好锁着，仍是敞开的**。
- 日常机没有 VM 快照，`uninstall.sh` + `/var/db/repose-unlock/` 离线还原栈 + E12 看门狗**就是全部安全网**。
- 当前 permit 走文件（`/var/run/repose-spike/permit`），任何本地进程只要能 `sudo touch` 就能越过锁屏（这是 G3 要求换成 IPC 的原因；`native/macos/permit-daemon/` 的 `permitd` 是另一条 workflow 在做）。

**决定项（详见 §7 开放问题 Q1）**：先在 VM 上把 A3 全链路跑绿（推荐），还是直接上宿主机做 Stage-1 个人版。本计划的构建顺序（§6）默认「先 VM 验证、再宿主机」，每一步都能在 VM 上验，只有最后的「真机演练 B」必须在目标机上做。

---

## 1. UI：复用什么、重写什么

### 1.1 结论：搬外壳，换内核

「之前的 UI」= `codex/phone-proximity-unlock` 分支的两个文件（这是最干净、聚焦解锁、放在设置页内的版本；`codex/phone-work-console` 那版把面板拆成独立 `phoneKey` 页并混入了无关的 WorkConsole 功能，不采用）：

| 文件（源分支 `codex/phone-proximity-unlock`） | 目标位置（当前分支） | 处理方式 |
|---|---|---|
| `src/components/UnlockSettingsPanel.tsx` | 同路径 | **搬外壳，重写内核**（见下） |
| `src/components/UnlockSettingsPanel.test.tsx` | 同路径 | 随内核重写 |
| `src/lib/unlock.ts` | 同路径 | **整体重写到 §6 契约**（旧的不兼容，见 1.3） |
| `src/lib/unlock.test.ts` | 同路径 | 随之重写 |
| `src/styles.css` 里 `.unlock-*` 规则 | **新文件 `src/phone-key.css`** | 搬运并按交互设计 §7.3 改成多行格式，补 `[data-theme=dark]` |

### 1.2 可以原样保留的外壳（旧面板做对的部分）

这些是「UI 是好的」为真的部分，直接复用：
- **集成点**：`import { UnlockSettingsPanel }`，在 `src/App.tsx` 设置页 `security-panel` 的 `</section>`（当前分支 `App.tsx:392`）之后、「提醒与声音」面板之前渲染 `<UnlockSettingsPanel bridge={window.repose?.unlock} />`。位置与交互设计 §5.2 完全一致。
- **容器生命周期**：单 in-flight 请求守卫（`requestSequence` ref）、挂载即拉快照、`bridge` 为 `undefined` 时全 disable —— 这正是交互设计 §7.6 的 `!window.repose` 只读降级。
- **撤销确认**：`unlock-revoke-cancel` / `unlock-revoke-confirm` 的 `alertdialog` 展开-确认交互（交互设计 §5.13）。
- **面板骨架**：header + 徽章 + 状态条 + 「查看组件状态」折叠 + 设备卡 + 底部 outline 按钮的布局。
- **安全限制文案区**（`unlock-safety-copy`）→ 映射交互设计 §5.3 的「它挡不住什么」与 §5.10 的常驻警告。

### 1.3 必须重写的内核：旧 `unlock.ts` 契约与交互设计 §6 不兼容

这是**「UI 是好的，只是没接上」这句话里唯一不成立的部分**，必须让用户知道：旧面板接的不只是占位后端，它接的是一套**旧 schema**。两套契约的形状根本不同：

| | 旧 `src/lib/unlock.ts`（codex 分支） | 交互设计 §6 新契约（本次目标） |
|---|---|---|
| 顶层态 | `capability`(4) + `authorizationGate`(3) | `state`(11 路)：`not-installed`…`ready`…`needs-repair`/`paused`/`uninstalling` |
| 在场轴 | 无 | `presence: near\|away\|transport-unavailable`（永不上横幅） |
| 规则形态 | 无 | `variant: 'A'\|'B'\|null`（E1 决定，驱动 §5.3 第 02 块文案） |
| 组件健康 | 4 个固定字段 `policy/plugin/service/transport` | `components[]`，每项带 `health/detail/evidence?/remediation?` |
| 「被系统调用过吗」 | 无 | `componentInvocation: observed\|never-observed`（只能来自守护进程实测，never 时禁止绿点） |
| 错误 | `UnlockErrorCode`(6 个泛型码) | `UnlockError{code(~20),detail,remediation(R1–R7/R0),evidence}` |
| 事后卡 | 无 | `lastFailure{cause: not-invoked\|phone-absent\|phone-slow\|allowed-but-locked}`（§5.14 分流） |
| 安装按钮 | `installEnabled: false` **硬编码** | 由 `unlock_preflight` 真实判定，安装是真的会跑 |

**决定：重写 `unlock.ts` 到 §6 契约。** 保留旧文件的工程手法（`normalizeUnlockSnapshot` 把原生 IPC 当不可信输入、非法/未来 schema 塌回安全态；`begin/finish/failUnlockRequest` 的单请求 reducer；`reduceRevocationConfirmation`）——这些直接复用，只是字段换成新形状。新增：
- `UnlockErrorCode`（§6.1 的枚举，`kebab-case`）+ `Remediation`（R1–R7 + `leave-it-alone`）+ `Evidence{expected,actual,readAt}`。
- `deriveUnlockView` 重写：`state` → 每态唯一主按钮（动词）；`presence` → 状态行（中性灰，不上横幅，交互设计 §3.4 不变量 2/4）；`componentInvocation.never-observed` → 「还没有观察到被调用」而非绿点（§5.10 实现要求）。
- 一个 `deriveGlobalBanner(snapshot)`：只在 F1/F2/F5/F6/F17/F24 与 F13>24h 时返回横幅（§4.4），复用现有 `.security-alert`。

### 1.4 从 `codex/phone-work-console` 采收的配对 UX（不采其架构）

`codex/phone-work-console` 的面板加了：`react-qr-code` 二维码、一次性配对码倒计时（`pairingSecondsRemaining` / `formatPairingCountdown`）、`PairingPollScheduler` 轮询配对状态。这三样正好对应交互设计 §5.7 的配对屏（二维码 + 3 分钟有效期 + 「等待手机…/已连上」状态行），**采收进新面板**。但不采它把面板拆成独立 `phoneKey` 页、不引入 WorkConsole——交互设计 §7.1 明确要求装进现有 `Modal`（`App.tsx:64-86`），不新建窗口（`capabilities/default.json` 只给 `main`/`break-*` 权限）。

### 1.5 前端硬约束（交互设计 §7，逐条落地）

- 全流程装进现有 `Modal`，安装在同一 Modal 内换 step，**不新建窗口**。
- **不引入 stepper / 进度条**；安装期五行打勾是「动作日志」（哪一步失败），不是进度条。
- CSS 写 `src/phone-key.css`（多行格式），深色模式逐条补 `[data-theme=dark]`。
- **不引入 destructive red**；撤销/移除用暖棕警告色（`#f7efe2`/`#ecdfc8`/`#aa8b58`）+ outline。
- `!window.repose` → 面板只读介绍态 + 「桌面版专属」徽章，Toggle 灰态。
- **点开关不立刻翻绿**（§5.2）：弹安装 Modal，只有安装真成功才翻。
- `break.html` 不做任何手机钥匙展示。

---

## 2. 后端：用什么替换 `GateClosedBackend`

### 2.1 落点与形态

在 `codex/phone-work-console` 里，Rust 侧已有 `UnlockBackend` trait + `GateClosedBackend` 占位（`crates/repose-unlock-core`，经 `build_unlock_backend.rs` 选择器）+ 6 个 `#[tauri::command]`。**当前分支没有这些**——`src-tauri/src/lib.rs` 的 `generate_handler!`（`lib.rs:876-884`）只有 7 个非解锁命令。

A3 在当前分支**新建 `src-tauri/src/unlock.rs`**，定义：
- `struct UnlockError { code, detail, remediation, evidence }`（`#[derive(Serialize)]`，§6.1 形状）。
- 一个 `trait UnlockBackend`（便于测试可注入假后端），一个真实实现 `HostMacBackend`——它**不再是占位，而是 shell 进已验证脚本**。
- 所有命令签名 `-> Result<T, UnlockError>`，参数走单个 `value` 对象（沿用现有惯例，见 `notify_user(value: NotificationValue)` `lib.rs:696`）。
- 命令注册进 `generate_handler!`。

**Command 的先例已经在仓库里**：`open_security_settings` 用 `Command::new("/usr/bin/open")`（`lib.rs:708-712`），idle-lock 用 `Command::new("/usr/bin/osascript")`（`lib.rs:803-805`）。**`std::process::Command` 从 Tauri 进程 shell 出去不需要任何 entitlement，不需要 shell 插件，不需要额外 ACL**（Tauri v2 自动放行 app 自定义命令）。这是 A3 最省的一条缝。

### 2.2 命令清单 → shell 目标 → UnlockError 码映射

脚本通过 `bundle.resources` 打进 `.app`，运行时用 `app.path().resource_dir()` 解析绝对路径（见 §4）。

| command（§6.2） | shell 进 / 调用 | 成功返回 | 失败 → `UnlockErrorCode` |
|---|---|---|---|
| `unlock_get_snapshot` | **只读**：`security authorizationdb read system.login.screensaver` + `codesign -dvvv` 读 bundle cdhash + `stat` permit 权限 + `launchctl print system/…healthcheck` + 读守护进程只读状态文件（R-P6）+ 传输状态 | `UnlockSnapshot`（带 `readAt`） | 读不到任一项 → 对应 `components[].health='unknown'`；整体不 throw |
| `unlock_preflight` | 只读：读当前规则，判 `class==rule`（`install.sh:47-54` 的逻辑）、k-of-n、条目集，判形态 A/B | `PreflightReport{variant, canInstall}` | 形态不符预期 → `preflight-rule-shape`（F3，唯一安全动作是停手） |
| `unlock_install` | `osascript -e 'do shell script "<resource>/install.sh permit" with administrator privileges'`（一次原生授权内：备份→装 bundle→落 uninstall.sh+RESTORE.txt→写 right(带 comment)→prepend 子规则→装 E12 daemon→重读校验） | `UnlockSnapshot` | 授权取消/密码错 → `authorization-denied`(F4)；写入失败(脚本已自回滚) → `install-failed`(F1)；中断半态 → `half-installed`(F5) |
| `unlock_repair` | `{target: Rule\|Component\|Daemon}` → 重跑 `install.sh` 的对应片段（修规则时**只加回我们那一条**，`authdb-edit add-subrule` 已是 prepend-保留语义，`install.sh:181`） | `UnlockSnapshot` | 同 install |
| `unlock_uninstall` | `osascript … with administrator privileges` 包 `uninstall.sh`（还原备份→删 right→删 bundle+`/var/db/repose-unlock/`→删配对密钥） | `UninstallReport`（重读后的 `ruleNow`/`diffAgainstBackup`/`residual`，§6.2） | 备份丢失 → 仍执行，`residual[]` 列出没还原的；码 `backup-missing`(F7) |
| `unlock_pair_begin` | 生成配对码 + qrPayload + 3 分钟 expiry（当前**无 crypto，见 §5 DEFERRED**） | `PairingSession` | — |
| `unlock_pair_cancel` | 作废当前 session | `()` | — |
| `unlock_calibrate_sample` | `{kind: Near\|Far}`：采一窗 RSSI（读 sidecar 输出），算可分离性 | `CalibrationReport{separable,marginDb}` | 远近重叠 → `calibration-overlap`(F18) |
| `unlock_revoke_device` | `{deviceId}` 删本机配对密钥，**不要求手机在场** | `UnlockSnapshot` | 找不到设备 → `device-not-found` |
| `unlock_set_enabled` | `{enabled}` 暂停/恢复：**停/起 LaunchAgent（BLE 桥），不动 authorizationdb** | `UnlockSnapshot` | — |
| `unlock_pause_for` | `{minutes}` 托盘用，定时恢复 | `UnlockSnapshot` | — |
| `unlock_drill_start` | `{kind: PasswordDrill\|PhoneDrill}`：PasswordDrill 先令守护进程对下一次求值强制 Deny（清 permit + 抑制刷新）再锁屏；PhoneDrill 直接锁屏 | `()` | — |
| `unlock_open_bluetooth_settings` | `Command::new("/usr/bin/open").arg("x-apple.systempreferences:…Bluetooth")`（沿用 `open_security_settings` 模式） | `()` | — |
| `unlock_copy_diagnostics` | 只读拼纯文本：实读规则 + cdhash + 守护进程最近记录 + macOS build | `String` | — |
| `unlock_export_manifest` | 拼含离线卸载命令的纯文本（§5.3「把这些改动保存一份」） | `String` | — |

**锁屏动作**（drill / idle-lock 的实际锁屏）沿用现有 `osascript … key code 12 using {control down, command down}`（`lib.rs:803-805`）。

### 2.3 事件（Rust→FE，§6.4）

现有事件经 `app.emit` 发、FE 用 `listen()` 收（`tauriBridge.ts:112-119`）。新增：
- `repose-unlock-snapshot`（`UnlockSnapshot`，低频，状态变化时）→ 面板 + 全局横幅。
- `repose-unlock-presence`（`{presence,lastSeenMs}`，秒级）→ **只驱动状态行，绝不驱动横幅**。
- `repose-command` 新增取值 `unlock-drill-passed`/`unlock-drill-failed`/`unlock-drill-not-observed`。**必须同时改 `tauriBridge.ts` 的 `DesktopCommandName` 联合类型和 `commandNames` Set**，否则前端白名单静默丢弃（交互设计 §6.4 警告）。

### 2.4 bridge 接线（`src/tauriBridge.ts`）

在 `window.repose` 上挂 `unlock: UnlockDesktopBridge`（沿用 codex 分支 `createUnlockBridge()` 模式，但方法名对齐 §6.2），每个方法 `invoke('unlock_get_snapshot')` 等。`!isTauri()` 时 `window.repose` 为 `undefined` → 面板降级。

---

## 3. BLE 在产品里怎么跑

### 3.1 结论：sidecar 扫描 + 用户 LaunchAgent，不走进程内 CoreBluetooth

有两条路。**选 sidecar**，因为它复用的正是本 session 验证过的东西（`rssi-scan.swift` + `permit-bridge.sh`，真机双向验证，`docs/validation/2026-09-09-b2-real-ble.md`），而进程内 CoreBluetooth（codex 分支的选择）在当前分支需要新链 `framework=CoreBluetooth`、新建 `Info.plist`、且从未接过真实 BLE。

### 3.2 root-permit vs BT-TCC 的角色冲突（这是 BLE 部分最硬的一点）

宿主机可行性文档（§7）点明：**写 permit 需要 root，但 CoreBluetooth 扫描需要 App 的蓝牙 TCC 授权（用户会话）——这两个角色不能塞进同一个进程。** 解法：

- **BLE 扫描跑成用户 LaunchAgent**（不是 root daemon），这样它像 spike 里的 Terminal 一样拿到蓝牙 TCC 弹窗（F11）。Agent 里跑 `rssi-scan | permit-bridge.sh`。
- **只有「刷新 permit」这一步走窄 root 路径**。`permit-bridge.sh` 的 `REPOSE_PERMIT_ON_CMD`/`_OFF_CMD` 是**可注入的**（`permit-bridge.sh:47-48`）：
  - **个人版（Stage 1）**：指向本机一条 scoped-NOPASSWD `sudo touch /var/run/repose-spike/permit`。
  - **产品版（Stage 2）**：扩展 `native/macos/permit-daemon/` 的 `permitd`，接受一次经认证的「在场断言」（复用 `repose_peer_verify.c`），**彻底去掉 root 文件 touch**——这也顺带满足 G3（permit 换 IPC）。
- **谁管 Agent 的生命周期**：App 通过 `unlock_set_enabled` / `unlock_pause_for` `launchctl bootout/bootstrap` 这个用户 Agent。`macos.m:44-67` 已订阅 `com.apple.screenIsLocked`，用于 R-P3 锁屏即热扫描。

### 3.3 打包与 Info.plist / entitlements 改动（当前分支缺，必须加）

1. **`NSBluetoothAlwaysUsageDescription`**：当前 `src-tauri/` **没有 Info.plist**，`tauri.conf.json` 没有 `bundle.macOS` plist-merge。要么给 sidecar 自己的 bundle 一份用途说明，要么（若扫描器作为 App 的 helper）给 App 加。字符串可从 `.worktrees/phone-work-console/src-tauri/Info.plist` 抄。
2. **`bundle.resources`**（当前**完全缺失**）：把 `install.sh`/`uninstall.sh`/`healthcheck.sh`/`authdb-edit`/`*.healthcheck.plist`/`ReposeSpike.bundle`/`rssi-scan`(编译后二进制)/`permit-bridge.sh` 打进 `.app`；运行时 `app.path().resource_dir()` 解析。**当前它们在 repo 根的 `native/`+`tools/`，装出来的 App 找不到。**
3. **sidecar 二进制**：`rssi-scan.swift` 编译成 `rssi-scan`（已在 `tools/ble-spike/mac/rssi-scan`），作为 `bundle.externalBin` 或普通 resource 由 `Command` 起。
4. **代码签名 / hardened runtime**：当前 `tauri.conf.json` `bundle.macOS` 只有 `minimumSystemVersion: 14.0`。Stage 1（个人、ad-hoc）够用——ad-hoc 在 SIP 开启下能加载（A1 已证），BT TCC 在 ad-hoc 下每次 rebuild 可能重弹但可接受。Stage 2 分发才必须 Developer ID + notarization（Gatekeeper/quarantine、稳定 BT TCC、SMAppService 注册）。

---

## 4. 特权安装路径与回滚

### 4.1 安装：一次 macOS 原生授权

```
osascript -e 'do shell script "\"<resource_dir>/install.sh\" permit" with administrator privileges'
```

- `with administrator privileges` 触发 **macOS 自己画的**授权框（交互设计 §5.5 逐字承诺「那个窗口是 macOS 自己画的，不是 Repose 画的」），payload 以 root 跑，一次安装/卸载各一次密码。
- `install.sh` 已具备：`class==rule` 前置检查（`install.sh:47-54`，对应 F3 停手）、备份到 `/var/db/repose-spike/`（`install.sh:126-145`，先写 `.partial` 校验再 mv）、stage-then-swap 装 bundle（`install.sh:109-117`，避免「删了旧的、新的没拷上」的悬空态）、写 right、prepend 子规则、装 E12 看门狗（`install.sh:185-198`）。
- **`install.sh` 三条硬要求需补齐到产品形态**（§6.2）：① right 的 plist 写 `comment` 键（`Installed by Repose … To remove: sudo /var/db/…/uninstall.sh`，当前脚本未写 comment，需加）；② `uninstall.sh` + `RESTORE.txt` 落 `/var/db/`（`uninstall.sh` 已在，`RESTORE.txt` 需生成）；③ `RESTORE.txt` 含安装前规则全文 + 形态 A/B 的 stock 常量。
- **命名**：spike 用 `ReposeSpike` / `ai.repose.spike` / `/var/db/repose-spike`；产品应改 `ai.repose.unlock` / `/var/db/repose-unlock`（交互设计全文用后者）。这是 A3 里对脚本的一处重命名改动。

**untamperable 前提（宿主机文档的 load-bearing caveat）**：root 跑的脚本必须不可被非 root 篡改——绝对路径、root-only 目录、bundle 内。`install.sh` 已把 helper 装到 root-only 的 `/Library/Application Support/ReposeSpike/`（`install.sh:189-192`）。

### 4.2 回滚故事（日常机的全部安全网）

1. **安装内自回滚**：`install.sh` 任一步失败即撤销已做的步骤；FE 在 §5.5「没装成，已经还原」屏**当场重读**规则并与备份逐行 diff（`unlock_install` 失败返回 `evidence{expected,actual,readAt}`）。
2. **E12 健康检查守护**：`ai.repose.spike.healthcheck.plist` + `healthcheck.sh`，开机 + WatchPaths + 慢定时断言「规则引用的 bundle 存在且 codesign 通过」，悬空即从 authdb 移除子规则退回纯密码。真机验证 rm bundle 后约 4 秒生效（`docs/validation/2026-09-09-e12-healthcheck.md`）。**这是 G2 唯一的解法**（E8/E11 证明规则结构关不掉 fail-open）。
3. **离线卸载**：`sudo /var/db/repose-unlock/uninstall.sh` + `RESTORE.txt`，**删掉 App 也不影响**（G7、F25）。`uninstall.sh` 还原备份、删 right、删 bundle、删看门狗。
4. **无备份分支（F7）**：stock 规则是常量不是用户数据，硬编码进 `RESTORE.txt` 与 `uninstall.sh`；形态 A 换回 `use-login-window-ui`，形态 B 且规则里有别人条目时如实告知「回不到逐字原样」（交互设计 §5.12）。

---

## 5. 宿主 vs VM 决定 · 分阶段路径 · 明确 DEFERRED

### 5.1 分阶段（来自宿主机可行性文档，压缩）

- **Stage 0（今天，个人，零 App 代码，ad-hoc）**：命令行 6 步（VM 或本机手动 `sudo install.sh` + 手动跑 BLE 桥 + 手动 permit）。用于继续验证，不是产品。
- **Stage 1（A3 核心，个人版，ad-hoc 签名）**：本计划的目标。顺序见 §6。ad-hoc 足够——SecurityAgentPlugins 在 SIP 开启下加载 ad-hoc 插件已验证。第一步先做**零特权的 `unlock_get_snapshot`**（用真实数据替换 `CLOSED_UNLOCK_SNAPSHOT`），风险最低、价值最高。
- **Stage 2（分发）**：Developer ID + notarization 变强制；permit 换 IPC（`permitd`）；crypto 配对必须补齐。

### 5.2 明确 DEFERRED（不在「A3 让功能能用」的定义内，但必须写清是缺口）

- **crypto 配对（DEFERRED）**：当前**任何**广播公开 UUID 的 Android 都算「在场」——没有认证。P-256 握手协议资产在 `codex/phone-proximity-unlock`（`docs/protocol/repose-unlock-v1.md`、`mobile/packages/repose_unlock_native/`），是跨分支集成；且「每日轮换身份广播（不做 >3s GATT 握手也能认证）」尚未设计。**Stage 1 个人版可先不做（单用户自己的手机），但产品分发前是硬门禁。** `unlock_pair_begin` 在 Stage 1 只生成配对码占位。
- **RSSI 校准默认值（B3，DEFERRED）**：`permit-bridge.sh:35-43` 的阈值是「保守占位，不要发布这些数字」。出厂默认需在多机型采样得出。`unlock_calibrate_sample` 的可分离性判定逻辑要实现，但默认阈值待定（交互设计 §9.5）。
- **permit IPC #3（另一条 workflow 在做）**：`native/macos/permit-daemon/permitd`。Stage 1 用文件 permit + scoped-NOPASSWD sudo 顶着（G3 未达成，属分发门禁）。
- **iOS**：首发只承诺 Android（交互设计 §9.1）。任何 UI/文案/官网不出现 iPhone。
- **E1（规则形态 A/B）**：`unlock_preflight` 的形态判定要写，但形态 A 是否真能触发插件链需 E1 定论（G1）。在 E1 出结论前，`variant` 判定按 `docs/validation/2026-09-09-e1-stock-rule.md` 的现有证据。

---

## 6. 构建顺序（每步带验证，终点＝真机 App UI 能用）

每步都可在 VM 上验，只有 Step 9 的真机演练 B 必须在目标机。建议按序，前 3 步零特权、可先合。

**Step 1 · 打包 seam（无行为改动）**
把 `native/macos/minimal-auth-plugin/` + `tools/ble-spike/mac/` 的脚本与 `rssi-scan`/`ReposeSpike.bundle` 加进 `tauri.conf.json` 的 `bundle.resources`；加 `NSBluetoothAlwaysUsageDescription`（新建 `src-tauri/Info.plist` 或 `bundle.macOS` plist-merge）。
*验证*：`tauri build` 后在 `.app/Contents/Resources/` 里能 `ls` 到全部脚本；`app.path().resource_dir()` 打印出的路径下文件存在。

**Step 2 · 前端外壳搬迁（接占位后端）**
从 `codex/phone-proximity-unlock` 搬 `UnlockSettingsPanel.tsx`，CSS 抽到 `src/phone-key.css`，在 `App.tsx:392` 后集成。**先接旧 `CLOSED_UNLOCK_SNAPSHOT`**，确认渲染与降级正常。
*验证*：`npm run dev`（浏览器，`!window.repose`）面板渲染为只读降级态；无 TS 错误；旧测试或占位测试绿。

**Step 3 · 契约升级 + 只读 `unlock_get_snapshot`（零特权，最高价值第一步）**
重写 `src/lib/unlock.ts` 到 §6 契约；新建 `src-tauri/src/unlock.rs` 实现 `unlock_get_snapshot`（只读 `security authorizationdb read` + `codesign` + `stat` + `launchctl print`）；注册命令；`tauriBridge.ts` 挂 `window.repose.unlock.unlockStatus()`。
*验证*：在**已用 Stage-0 命令行装好 spike 的 VM** 上跑 App，面板显示真实规则/cdhash/看门狗状态；`componentInvocation` 在没观测到时显示「还没有观察到被调用」而非绿点；`unlock.test.ts` 覆盖 `normalizeUnlockSnapshot` 对畸形 IPC 塌回安全态。

**Step 4 · `unlock_preflight` + 安装 Modal 文案分支**
实现形态 A/B 判定；FE §5.3 第 02 块按 `variant` 选文案；F3 停手屏（`preflight-rule-shape`）。
*验证*：VM 上 stock 规则 → preflight 返回可安装 + variant；人为改成异常形态 → F3 屏、系统零改动。

**Step 5 · `unlock_install` / `unlock_uninstall`（特权，含回滚）**
接 `osascript … with administrator privileges` 包 `install.sh`/`uninstall.sh`；补脚本的 `comment` 键、`RESTORE.txt`、`ai.repose.unlock` 重命名；实现失败自回滚后**重读** diff（`evidence`）；`UninstallReport` 只渲染真实读数。
*验证*（对应 G6/G7）：VM 上装→卸载，有备份/无备份两条分支各跑一次，还原报告 diff 为真实读数；`security authorizationdb read` 确认逐字还原；删掉 App 后 `sudo /var/db/repose-unlock/uninstall.sh` 仍能卸干净。

**Step 6 · E12 看门狗在 App 流程里确认（G2）**
安装后确认看门狗随 `install.sh` 装上；`unlock_get_snapshot` 能读到它的状态。
*验证*：装好后 `rm` bundle，≤~5 秒规则自动退回纯密码，`security authorize` 从 YES 翻 NO（复现 `docs/validation/2026-09-09-e12-healthcheck.md`）；面板升红色横幅（悬空引用 = 安全紧急态，交互设计 §3.3 末行）。

**Step 7 · BLE LaunchAgent + presence 事件**
把 `rssi-scan | permit-bridge.sh` 装成用户 LaunchAgent，`REPOSE_PERMIT_ON_CMD` 指向本机 scoped-NOPASSWD sudo；`unlock_set_enabled`/`unlock_pause_for` 管 Agent 生命周期；发 `repose-unlock-presence`。
*验证*：真机上带手机靠近 → 面板状态行「刚刚看到 <设备>」，permit 被刷新；拿远 → 8 秒内变「手机不在附近」并清 permit（复现 B2）；presence **不触发横幅**。

**Step 8 · 配对 + 校准（占位 crypto / 占位阈值）**
`unlock_pair_begin`/`_cancel` + 二维码/倒计时（采收自 work-console）；`unlock_calibrate_sample` 采「远」+ 可分离性判定。
*验证*：配对码 3 分钟过期屏；校准远近重叠 → F18；采「近」在配对成功时自动触发一次。

**Step 9 · 演练 A / 演练 B（真机，进入就绪的唯一路径，G4）**
`unlock_drill_start{PasswordDrill}`：令守护进程对下一次求值强制 Deny 再锁屏（演练 A，验证「机制 Deny 时密码路径完好」——产品依赖但实验室没测过的那一格）；`{PhoneDrill}` 真实锁屏 + 回车（演练 B，唯一 S5→S6 路径）。守护进程写 F22 证据（R-P6），驱动 §5.14 事后卡四路分流。
*验证*（**必须在目标真机**）：演练 A——机制 Deny 下用密码进得来（「退路是通的」）；演练 B——手机在场按回车进入，面板转 `ready`，`componentInvocation` 变 `observed`。**到这一步「功能从 App UI 在真机上能用」成立。**

**Step 10 · 托盘 + 全局横幅 + 事后卡 + 文案同步**
托盘加「手机钥匙 · <状态>」「暂停一小时」（§5.16）；`deriveGlobalBanner` 接横幅（§5.15）；事后解释卡（§5.14）；README「不包含在安装包中」那行 + `website/` 中英文同步（§7.8）。
*验证*：托盘状态词随快照变；横幅只在 F1/F2/F5/F6/F17/F24/F13>24h 出现，不为「手机不在」出现。

---

## 7. 需要用户拍板的开放问题

**Q1（最大）· 拓扑与同意：先 VM 全绿再上宿主机，还是直接宿主机做 Stage-1？**
本计划默认「VM 验证 Step 1–8、真机只做 Step 9」。但功能的真实价值只有在日常机上才兑现，而日常机没有 VM 快照兜底，安全网只剩 `uninstall.sh` + E12 看门狗。需要用户明确：接受在主力机装 Authorization Plugin 吗？还是先接受一台专门的测试 Mac？

**Q2 · crypto 配对算不算「fully working」的一部分？**
Stage-1 个人版没有配对认证——任何广播公开 UUID 的 Android 都能解你的锁屏。对「只有你自己一部手机」的个人自用，这可接受；但如果用户对「fully working」的定义包含「只有我配对的那部手机能解」，则必须先做跨分支集成 `codex/phone-proximity-unlock` 的 P-256 握手 + 设计每日轮换身份广播——这会显著扩大 A3 范围。

**Q3 · UI 内核重写的确认。**
「之前的 UI 是好的」为真，但它的状态模型（`unlock.ts`）接的是旧占位 schema，与交互设计 §6 不兼容（§1.3 对照表）。A3 会**保留外壳、重写内核**。需要用户确认接受这个「UI 外观基本不变、但底层契约换掉」的做法，而不是原样 port 旧 `unlock.ts`。

**Q4 · 形态 A/B 未定（依赖 E1，G1）。**
`unlock_preflight` 和安装 Modal 第 02 块的文案分支依赖 E1 的结论（`[ours, use-login-window-ui]` k-of-n=1 能否触发插件链）。E1 未出结论前，是先按现有证据（`docs/validation/2026-09-09-e1-stock-rule.md`）走形态判定、把 E1 作为发布门禁，还是先补 E1 再动 UI 文案？

**Q5 · 命名与 permit 路径迁移。**
spike 用 `ReposeSpike`/`ai.repose.spike`/`/var/db/repose-spike`；产品文案统一用 `ai.repose.unlock`/`/var/db/repose-unlock`。确认在 A3 里做这次重命名（会牵动脚本、看门狗 label、备份目录）。

---

## 关键文件索引

- 现有 App 后端：`src-tauri/src/lib.rs`（命令注册 `876-884`；`Command` 先例 `708-712` / `803-805`）、`src-tauri/tauri.conf.json`、`src-tauri/capabilities/default.json`、`src-tauri/build.rs`。
- 现有 FE bridge：`src/tauriBridge.ts`（`window.repose` `121-148`）、`src/App.tsx`（集成点 `392`，`Modal` `64-86`，设置页 `385-393`）。
- 要搬的旧 UI（源 `codex/phone-proximity-unlock`）：`src/components/UnlockSettingsPanel.tsx`、`src/lib/unlock.ts`（旧契约，将重写）、`src/styles.css` 的 `.unlock-*`。
- 配对 UX 采收（源 `codex/phone-work-console`，`.worktrees/phone-work-console/`）：`src/components/UnlockSettingsPanel.tsx`（QR/倒计时/轮询）；Rust 占位面：`src-tauri/src/unlock.rs`、`build_unlock_backend.rs`、`crates/repose-unlock-core/`、`Info.plist`（`NSBluetoothAlwaysUsageDescription`）。
- 已验证后端（要 shell 进）：`native/macos/minimal-auth-plugin/{install.sh,uninstall.sh,healthcheck.sh,ai.repose.spike.healthcheck.plist,authdb-edit,plugin.c}`、`tools/ble-spike/mac/{rssi-scan.swift,rssi-scan,permit-bridge.sh}`。
- permit IPC（另一 workflow）：`native/macos/permit-daemon/{repose-permitd.c,repose_peer_verify.c,repose_permit_client.c}`。
- 权威输入：`docs/plans/2026-09-09-unlock-interaction-design.md`（§3 状态机 / §4 失败态 / §6 后端契约 / §7 前端约束 / §8 门禁）、`docs/plans/2026-09-09-host-mac-feasibility.md`、`docs/plans/2026-09-09-repose-app-survey.md`。
- 证据：`docs/validation/{2026-09-09-a1-plugin-load.md, 2026-09-09-e3-fail-open.md, 2026-09-09-e11-lockscreen-grant-model.md, 2026-09-09-e12-healthcheck.md, 2026-09-09-b2-real-ble.md, 2026-09-09-e1-stock-rule.md}`。
