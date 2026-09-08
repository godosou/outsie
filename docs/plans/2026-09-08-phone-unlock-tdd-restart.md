# 手机靠近解锁：按风险重排的 TDD 计划

日期：2026-09-08

分支：`feat/phone-unlock-walking-skeleton`（从 `main` 分出）

取代：`docs/plans/2026-09-07-phone-proximity-unlock.md` 的任务顺序（该文档的技术设计仍然有效）

## 为什么要重排

`codex/phone-proximity-unlock` 分支写了 314 个文件、52,449 行，所有测试全绿
（JS 46/46、Flutter 64/64、Android JVM 94/94、Rust workspace 全过），但真实的
「锁屏 → 离开 → 靠近 → 解锁」一次都没有发生过。核查结论：

| 事实 | 证据 |
|---|---|
| 传输层完全不存在 | 全分支没有 `BluetoothGattServer` / `BluetoothLeAdvertiser` / `BluetoothLeScanner` / `connectGatt`；`CoreBluetooth` 只出现在一份 markdown 里；`src-tauri/Cargo.toml` 无任何 BLE 依赖 |
| 手机侧生产角色写死关闭 | `BleRoleStrategy.kt:14` — `fun productionRole(): BleRole = BleRole.Disabled` |
| Mac 侧命令全部返回不可用 | `src-tauri/src/unlock.rs:342` — `ProductionUnlockCommandService = UnlockCommandService<GateClosedBackend>`；该 backend 的配对/校准命令一律返回 `BackendUnavailable` |
| 安装通道硬关闭 | `repose-unlockctl/src/main.rs:68` — `apply-gate=closed-pending-task-8` |
| 解锁机制从未加载 | `docs/validation/macos-authorization-results.md`：没有读过 authorizationdb，没装过 launchd，没锁过屏 |

根因是任务顺序把风险排反了。原计划 14 个任务里，Task 1–7（校准、状态机、协议、
密码学、防重放、许可、安装事务）在动手之前就确定能做成；而这个功能真正的两个未知：

1. macOS 在无人操作时能不能自己顶掉密码框（`authorizationhost` 是否加载插件、
   `RequestInterrupt` 是否有效）；
2. realme UI 7.0 熄屏 / Doze 下 BLE 后台能不能活；

分别被排到 Task 8（标注 BLOCKED）和 Task 11 Step 4–5（NOT RUN），且 Task 8 的前置条件
（专用可擦除 Mac、二次管理员、远程恢复、FileVault 恢复演练、备份还原、Developer ID
签名）在当前这台日常开发机上无法满足。于是每一轮都合规地重写一遍 `GATE CLOSED`，
然后回去继续增加纯逻辑测试。

**最关键的一点：那 200 多个测试里没有一个会因为「Mac 真的解锁了」而由红转绿。**
测试全绿与功能零可用同时成立，TDD 的反馈回路完全脱靶。

## 开发机上已被实际安装的组件（2026-09-08 已清理）

排查过程中发现，`repose-unlockctl install --apply` 曾在这台日常开发机上真实执行过，
时间戳 2026-09-08 15:23。这与旧分支 `docs/validation/verification-summary.md` 中
「没有读取或修改真实 authorizationdb，没有安装/加载 Authorization Plugin 或 launchd
服务，没有执行任何 `--apply` 命令」的记载直接矛盾。**该分支的 "GATE CLOSED / NOT RUN"
记载不能再作为机器实际状态的证据。**

发现时的实际状态：

| 组件 | 状态 |
|---|---|
| `/Library/PrivilegedHelperTools/ai.repose.unlockd` | 以 root 运行中（PID 65212），`RunAtLoad` + `KeepAlive` |
| `/private/var/run/ai.repose.unlock-control.sock` | `srw-rw-rw-`，即 0666，**任何本地进程可写** |
| `/var/run/ai.repose.unlockd/consume.sock` | 0600（同一 plist 里这个是对的） |
| `ai.repose.unlock` | 已写入 authorizationdb，并被 `system.login.screensaver` 引用 |
| `ReposeUnlock.bundle` | 已装入 `/Library/Security/SecurityAgentPlugins/` |

当时没有发生自动解锁。**此处最初的解释是错的，现更正**：我曾认为规则里的
`cdhash H"38ea648b..."` 是一个失效的 pin（与实际 bundle 的 `a7c8340c...` 不符），
因而插件不被信任。查阅 Apple 开源的 authd 源码后确认并非如此 ——
`OSX/authd/rule.c` 的 `rule_sql_commit` 会**无条件用写入者进程的 csreq 覆盖**
requirement 字段，而 `engine.c` 的求值路径从不读取该字段。

所以那串 cdhash 极可能就是 `repose-unlockctl` 自己的签名标识，由 authd 自动记录，
不是任何人写上去的 pin；拿它与插件的 cdhash 比较本身就是在比较两个不同的二进制。
同机 OpenAI 规则里的 `identifier "com.apple.security" and anchor apple` 是同一现象
的佐证 —— 那是 `/usr/bin/security` 的身份，因为其安装流程调用了该命令。

更可能的真实原因是：插件被正常加载并调用，但因 permit 条件不满足而主动 Deny，
按 `k-of-n=1` 回落到密码路径。**这意味着「ad-hoc 签名插件能否在 macOS 14.6.1 上被
加载」可能已经有过一次肯定的实例**，只是当时没有日志留存，无法据以定论 —— A1 仍需实测。

两件相关的产物已据此修正：`install.sh` 不再写 `requirement` 键（它是无效的安全剧场），
但仍从已安装的 bundle 读取 cdhash 记入日志，作为「装的到底是哪个二进制」的凭据。

清理由 `tools/uninstall-legacy-unlock/` 的两个脚本完成，已执行并独立复核：

- 守护进程、plist、二进制、两个套接字全部移除，无残留进程；
- `ai.repose.unlock` 从 screensaver 规则中外科式摘除，命名规则本身删除，bundle 删除；
- 无关的 `com.openai.sky.CUAService.AuthorizationPlugin.remote` 原样保留；
- 清理后锁屏往返验证 **PASS**：锁屏 1142ms 生效，密码解锁 4469ms 完成。

回滚备份保留在 `/var/db/repose-unlock-cleanup/`。

## 唯一的执行规则

> 只有 `tests/e2e/unlock_acceptance.sh` 由红转绿，才算完成一步。
> 单元测试变绿不算进度。

每天收工时问一句：今天有没有一条「真实系统行为」第一次被证明？如果答案是
「写了 N 个通过的单元测试」，那今天是 0 进度。

## 保留什么

`codex/phone-proximity-unlock` 原封不动保留。它的密码学、状态机、防重放、许可 broker
和 authorizationdb 变换都是有价值的、已测的资产 —— 只是接线顺序错了。A4 与 B3 会把它们
合并进来。本分支从 `main` 起步，是为了让 A1 的切片保持真正的最薄；带着 729 行的
`plugin.c` 会把工作重新拽回原来的顺序。

## 先拆掉环境卡点

**用 macOS 虚拟机当「专用测试机」。** Apple Silicon 上用 Tart 或 UTM
（原生 Virtualization.framework）起一个 macOS VM：

- 快照/回滚是秒级的 —— 「erase/restore 演练」「备份还原」「断电恢复」这些前置一次性满足；
- VM 里可以关 SIP，ad-hoc 签名的 Authorization Plugin 即可加载，不需要 Developer ID
  （当前 `security find-identity` 是 0 个身份）；
- 装崩了回滚快照，日常机永远不会被锁在门外。

这把 Task 8 从「永久阻塞」变成「半小时可做」。VM 没有蓝牙直通，但 A1 不需要蓝牙，
第一阶段全程用模拟存在源。

### Developer ID 是个假门禁

旧分支把两件不同的事混成了一件：

1. **Developer ID + 公证 = 分发要求。** 只有把插件装到别人的 Mac 上、需要绕过 Gatekeeper
   时才需要（$99/年）。与「机制在自己机器上灵不灵」无关。
2. **能否被 `authorizationhost` 加载 = 签名形式 + library validation 的问题。**
   ad-hoc 签名（`codesign -s -`）很可能就够。

`production/attestation.rs` 把 (1) 变成了 (2) 的前置条件：

```rust
pub fn verify_production_mutation_gate() -> Result<(), ArtifactError> {
    Err(ArtifactError::ProductionGateClosed)      // 无条件拒绝
}
...
SignaturePolicy::PinnedDeveloperId => Err(ArtifactError::ProductionGateClosed),
```

第二行 pin 了一个 Developer ID 要求，又硬编码说该要求永远不满足。唯一返回 `Ok` 的
`DevelopmentAdHoc` 分支只用于校验开发包，不通向安装。**所以拦住安装的不是 macOS，是这段
代码本身**；即使购买了 Developer ID 证书，该分支也不会修改任何系统文件。

真实的签名要求未知，且不应靠猜。Jamf Connect、NoMAD Login 等第三方 Authorization Plugin
确实可用，因此 `authorizationhost` 必定没有启用 library validation。A1 在 VM 里按下列
梯度实测，哪一级通过就说明真实要求是什么：

1. ad-hoc 签名 + SIP 开启（最严，先试这个）
2. ad-hoc + 关闭 library validation
3. ad-hoc + `csrutil disable`

在拿到这个答案之前，不要购买 Developer ID，也不要把签名写成任何前置条件。

其余两项：**iOS 砍出本期**（Android-first，当前工具链没有 iOS SDK）；Android 环境是通的
（RMX3888 上 instrumentation 已 5/5 跑过）。

---

## 执行顺序（按风险与依赖重排）

早先两版计划各犯过一个变体的错误：第一版把 8 小时耐久测量排在骨架跑通之前；第二版只
规划到「技术链路打通」，没把功能交付到可用。正确的分界是 **功能完整交付（含 App 内引导
安装、配对、校准、故障恢复）→ 再做耐久测试**。

交付标准：**先自己用**（ad-hoc 签名，不买 Developer ID），但安装器、错误处理、健康检查
按可分发的质量写，只把签名这一层留作后续接入。交互流程重新设计，旧分支的**状态模型**
值得复用，界面与流程不沿用。

排期约束：手机会被带走，因此按「需不需要手机」分成两段。Mac 侧的全部工作都不依赖手机，
只要把「手机在不在」抽象成一个可替换的存在源。

---

# 第一阶段：不需要手机

## A0 阻塞项修复 — 已完成

| 项 | 状态 |
|---|---|
| `uninstall.sh` 备份路径与 install.sh 不一致，导致卸载永远不还原规则 | ✅ 已修，并新增无备份时的外科式移除路径 |
| `lockstate.sh` 只能读本机 | ✅ 加 `REPOSE_LOCKSTATE_CMD` 注入点 |
| `unlock_acceptance.sh` 锁屏动作写死本机 | ✅ 加 `REPOSE_LOCK_CMD` 注入点 |
| SSH 握手开销污染 3 秒延迟测量 | ✅ `vm-env.sh` 用 ControlMaster 复用连接 |

## A1 证明 macOS 会加载并听从插件 — 待 VM

**最大的未知。** 纯 VM 内进行，只用本地 `/tmp`，零共享目录零 SSH，先把这个未知单独隔离
回答。

1. `tart run repose-spike`，按 `tools/vm-spike/FIRST-BOOT.md` 人工过一遍初始设置。
2. 打干净快照 `repose-spike-clean`，之后随时可回滚。
3. **里程碑 A**：`sudo ./install.sh log` → 锁屏 → `cat /tmp/repose-plugin.log`。
   有行即证明 `authorizationhost` 会加载并调用 ad-hoc 签名的第三方插件。
   `log` 模式无条件放行，配合 `k-of-n=1` 会**直接无密码解锁**，这正是测试目的。
4. **里程碑 B**：`uninstall.sh && install.sh permit`，文件触发跑验收测试。
5. A 不过就按 `docs/product-tech-research/2026-09-08-securityagent-plugin-loading.md`
   的梯度下探（ad-hoc + SIP 开 → 关 library validation → 关 SIP）。

**产出不只是「行/不行」，而是「安装需要用户做哪几步」** —— 这是 A2 设计的直接输入。三种
结果对应三种完全不同的引导形态，最后一种（必须关 SIP）甚至要重新评估这个功能值不值得做。

`tools/vm-spike/run-experiment.sh` 会在建议安装之前，先把合约测试拷进 VM 里跑一遍。
这样万一里程碑 A 一片沉默，可以确定不是我们的 bundle 在传输中损坏或行为异常。

## A2 交互流程设计 — 待 A1 结论

重新设计。要覆盖的真实状态：未安装 / 安装中 / 已安装未配对 / 已配对未校准 / 正常 /
组件异常 / 已撤销，以及每个失败态的**出路**（不是只显示错误）。

复用旧分支的状态模型（`src-tauri/src/unlock.rs` 的 snapshot 结构：capability、
components{policy,plugin,service,transport}、devices、pending_pairing、calibration、
limitations）。

## A3 引导安装器 + 真实后端

App 内引导：说明将要修改什么 → 管理员授权 → 安装组件 → 健康检查 → 出错时给出修复或
干净卸载。用真实实现替换 `ProductionUnlockCommandService = UnlockCommandService<GateClosedBackend>`。

签名留口子：复用 `repose-unlockctl` 已有的 `SignaturePolicy::{DevelopmentAdHoc,
PinnedDeveloperId}` 这个缝 —— 让 `DevelopmentAdHoc` 真正走通，`PinnedDeveloperId` 返回
「未配置」，而不是旧代码里那种永久关闭的 `ProductionGateClosed`。

## A4 用模拟存在源跑通完整链路

把「手机在不在」抽象成存在源接口，第一个实现是模拟源（ssh 写 permit 文件），第二个才是
真实 BLE。验收测试的 `REPOSE_LEAVE_CMD` / `REPOSE_RETURN_CMD` 已经是这个抽象。

**接真实 permit 之前必须修**：`plugin.c` 用 `access(path, F_OK)` 判定 —— 文件存在即放行，
不校验新鲜度。一个忘删的 permit 文件等于永久免密。改成带时间戳并校验有效期。

**第一阶段完成判据**：干净快照上从「没装过」开始，**只用 App 界面**完成安装 → 健康检查 →
模拟离开/返回触发自动解锁 → 卸载并确认系统完全还原，全程不碰命令行。

---

# 第二阶段：需要手机

## B1 BLE bring-up（约 15 分钟）

装 APK、授权、加 realme 耗电白名单、启动广播；宿主 `./rssi-log.sh 300`。
判据：读到 `repose-hello` 且**读完后 CSV 仍持续出行**，`peripherals` 单一 id。

**分叉点**：若 `advertising: no`，这台手机当外围走不通，必须翻转角色。影响范围仅限 BLE
层 —— 安装器、UI、解锁链路不受影响，这正是先做第一阶段的好处。

## B2 把模拟源换成真实 BLE

新增 `permit-bridge.sh`：读 CSV 流做带迟滞的近/远判定，驱动同一个存在源接口。

传 permit 走 SSH 而非共享目录：`plugin.c` 的 `PERMIT_PATH` 是硬编码宏（改它就换 cdhash，
得重新 make + 重装），而 virtiofs 上 `access()` 走真实 uid、attr 缓存会让删除延迟可见，
直接打乱「离开后必须保持锁定」那条断言。走 SSH 则 `plugin.c` 一行不用改。

## B3 配对与校准

接入旧分支已测的密码学资产（P-256、Keystore/Secure Enclave、防重放计数器、一次性许可）。
校准要能失败并要求重做。

## B4 耐久测试

8 小时 Doze 观测、force-stop、重启、蓝牙切换、30 次离开/返回循环、p50/p95 延迟、误近率、
耗电、20 次密码 fallback。解读陷阱见 `tools/ble-spike/README.md`。

---

## 当前状态

| 项 | 状态 |
|---|---|
| 锁屏 oracle | ✅ 本机双向实测（`false→true→false`），远端注入点已测 |
| 验收测试 | ✅ 已写，按设计红着；`REPOSE_LOCK_CMD` / `REPOSE_LOCKSTATE_CMD` 就位 |
| 最小 Authorization Plugin | ✅ 编译 + ad-hoc 签名；**22 项合约测试通过**；**从未被系统加载** |
| 授权规则变换 | ✅ 抽出为 `authdb-edit.py`，15 项测试，含 4 类畸形输入的拒绝 |
| install/uninstall 不变量 | ✅ 15 项静态断言，经变异测试验证有效 |
| BLE 两端 | ✅ 编译通过，6 个会伪造结论的 bug 已修；**从未真机通信** |
| Tart VM | ⏳ IPSW 下载中（`UniversalMac_14.6.1_23G93`，与宿主同 build） |
| 手机 RMX3888 | 已确认可连（Android 16 / API 36），当前不在手边 |

测试总数：52 项（插件合约 22 + 规则变换 15 + 脚本不变量 15），加上验收 harness 自测 17 项。

## 相关文档

- [VM 首次启动手册](../../tools/vm-spike/FIRST-BOOT.md)
- [BLE spike 说明](../../tools/ble-spike/README.md)
- [Authorization Plugin 签名要求调研](../product-tech-research/2026-09-08-securityagent-plugin-loading.md)
- 旧分支 `codex/phone-proximity-unlock` 的设计与威胁模型仍可参考，但其 validation 文档
  已被证明与机器实际状态不符，不可作为证据使用。
