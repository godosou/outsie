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

## 唯一的执行规则

> 只有 `tests/e2e/unlock_acceptance.sh` 由红转绿，才算完成一步。
> 单元测试变绿不算进度。

每天收工时问一句：今天有没有一条「真实系统行为」第一次被证明？如果答案是
「写了 N 个通过的单元测试」，那今天是 0 进度。

## 保留什么

`codex/phone-proximity-unlock` 原封不动保留。它的密码学、状态机、防重放、许可 broker
和 authorizationdb 变换都是有价值的、已测的资产 —— 只是接线顺序错了。Step 4 会把它们
合并进来。本分支从 `main` 起步，是为了让 Step 2 的切片保持真正的最薄；带着 729 行的
`plugin.c` 会把工作重新拽回原来的顺序。

## 先拆掉环境卡点

**用 macOS 虚拟机当「专用测试机」。** Apple Silicon 上用 Tart 或 UTM
（原生 Virtualization.framework）起一个 macOS VM：

- 快照/回滚是秒级的 —— 「erase/restore 演练」「备份还原」「断电恢复」这些前置一次性满足；
- VM 里可以关 SIP，ad-hoc 签名的 Authorization Plugin 即可加载，不需要 Developer ID
  （当前 `security find-identity` 是 0 个身份）；
- 装崩了回滚快照，日常机永远不会被锁在门外。

这把 Task 8 从「永久阻塞」变成「半小时可做」。VM 没有蓝牙直通，但 Step 2 不需要蓝牙。

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
确实可用，因此 `authorizationhost` 必定没有启用 library validation。Step 2 在 VM 里按下列
梯度实测，哪一级通过就说明真实要求是什么：

1. ad-hoc 签名 + SIP 开启（最严，先试这个）
2. ad-hoc + 关闭 library validation
3. ad-hoc + `csrutil disable`

在拿到这个答案之前，不要购买 Developer ID，也不要把签名写成任何前置条件。

其余两项：**iOS 砍出本期**（Android-first，当前工具链没有 iOS SDK）；Android 环境是通的
（RMX3888 上 instrumentation 已 5/5 跑过）。

---

## Step 0：造一个能 assert 的 oracle — 已完成

没有可执行的验收判据就没有 TDD。

**文件：**

- 新增：`tests/e2e/lockstate.sh`
- 新增：`tests/e2e/unlock_acceptance.sh`
- 新增：`tests/e2e/harness_selftest.sh`

锁屏状态从 IOKit 读，零依赖（不需要 PyObjC / Swift / 签名）：

```bash
ioreg -n Root -d1 -a | plutil -extract IOConsoleLocked raw -o - -
```

**已验证（2026-09-08，MacBook-Pro-2 / macOS 14.6.1 23G93）：**

- oracle 在 unlocked 状态下四种模式全部正确；
- harness 的轮询/超时逻辑自测 7/7 通过；
- `--dry-run` 与两道安全闸（未设 `REPOSE_LEAVE_CMD` / 未设 `REPOSE_E2E_ALLOW_LOCK=1`）
  行为正确；
- **真实锁屏往返**：`open -a ScreenSaverEngine` 后 1038ms 观察到 `IOConsoleLocked=false
  → true`，用户输入密码后观察到 `true → false`。oracle 两个方向都成立，且本机锁屏密码
  确认为「立即」。

macOS 14 已经读不到 `defaults -currentHost read com.apple.screensaver askForPassword`，
所以「锁屏密码是否立即」只能这样经验性地验证。测试 VM 上要重做一次这个往返，再开始
Step 2。

## Step 1：让北极星测试红起来

`tests/e2e/unlock_acceptance.sh` 断言的就是产品承诺本身：

```
锁屏 → 手机离开 → Mac 保持锁定 → 手机返回 → 3 秒内解锁
```

「离开」和「返回」是注入的（`REPOSE_LEAVE_CMD` / `REPOSE_RETURN_CMD`），所以同一套断言
能活过后面每一步而不必重写。「手机离开后必须保持锁定」这条负向断言不能省 —— 少了它，
一个无条件解锁的插件也能通过测试。

**完成判据：** 在测试 VM 上跑一次，确认它因为「没有在 3 秒内解锁」而失败，而不是因为
锁屏没触发、oracle 读不出来或者 hook 报错而失败。红得对，才是有效的红。

同时确认 VM 的「系统设置 → 锁定屏幕 → 在屏幕保护程序开始后要求输入密码」设为「立即」，
否则根本没有可解的锁。

```bash
tests/e2e/harness_selftest.sh
REPOSE_LEAVE_CMD='rm -f /tmp/repose-permit' \
REPOSE_RETURN_CMD='touch /tmp/repose-permit' \
  tests/e2e/unlock_acceptance.sh --dry-run
```

## Step 2：最小垂直切片 —— 文件触发解锁（第一次绿）

在 VM 里装一个只做一件事的 Authorization Plugin：**看到 `/tmp/repose-permit` 就放行**。

没有密码学、没有 BLE、没有状态机、没有 IPC 协议、没有 launchd。目标只有一个：搞清楚
macOS 到底让不让你干这件事。

**完成判据：**

```bash
REPOSE_LEAVE_CMD='rm -f /tmp/repose-permit' \
REPOSE_RETURN_CMD='touch /tmp/repose-permit' \
REPOSE_E2E_ALLOW_LOCK=1 tests/e2e/unlock_acceptance.sh
```

由红转绿，且未触发时密码输入完全正常。

**这一步一旦绿，最大的未知就死了。** 如果做不成，整个方案要换路子 —— 这个答案值得用
一天换，不值得用一个月换。

## Step 3：把文件触发换成真实 BLE（第二次红转绿）

写今天完全不存在的代码：Android 侧 GATT server + advertiser，Mac 侧 central
（Rust 用 `btleplug`，或先写个几十行的 Swift CoreBluetooth 小进程更快）。
**仍然不加密、不做状态机**，只传一个明文 hello 并读 RSSI。

前置实验：手机熄屏放口袋，Mac 侧连续记 8 小时 RSSI 日志，看 realme UI 会不会在 Doze
里把连接掐掉。这份数据决定 BLE 角色怎么选 —— 也只有这份数据能让
`BleRoleSelector.productionRole()` 有资格不再是 `Disabled`。

**完成判据：** 手机关蓝牙 → 保持锁定；开蓝牙走近 → 解锁。

## Step 4：给那 52k 行通电

按 permit → 状态机 → 协议/签名 → 防重放 → 校准 的顺序，从
`codex/phone-proximity-unlock` 逐层合并进真实链路。每接一层跑同一条验收测试，保持绿。
既有单元测试从这一步开始才真正发挥防回归的作用，而不是充当进度的假象。

## Step 5：场景矩阵逐条变成测试

Doze、force-stop、重启、蓝牙切换、30 次离开/返回循环、p95 延迟、20 次密码 fallback。
每条先写进验收 harness 成为一个 case，红了再修。这一步做完才谈得上
`docs/validation/verification-summary.md` 里的发布门禁。

## 相关文档

- [原技术设计](2026-09-07-phone-proximity-unlock-design.md)（在 `codex/phone-proximity-unlock` 分支）
- [安全威胁模型](../security/phone-unlock-threat-model.md)（同上）
- [验证状态与发布门禁](../validation/verification-summary.md)（同上）
