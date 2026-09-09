# 让手机钥匙在「自己的日常 Mac」上真正工作 · 可行性评估（A3 的宿主机前置）

日期：2026-09-09 · 分支 `feat/phone-unlock-walking-skeleton` · 状态：A3 实现前的宿主机决策

本文只回答一个问题：**把已验证的后端接进现有 App UI 之后，一个用户从「装好 App」到「手机解锁我的 Mac」，宿主机上到底要发生哪些事。** 它是 [2026-09-09-unlock-interaction-design.md](2026-09-09-unlock-interaction-design.md)（§6 后端契约、§8 发布门禁）和 [2026-09-09-permit-design.md](2026-09-09-permit-design.md) 的宿主机侧补充，不重复其中的状态机与文案。

> 一句话结论：**三块已验证的零件（插件 / 健康检查 / BLE→permit 桥）已经能在宿主机上拼起来。** 缺的不是新机制，是「让在场写入进程跑在 root、同时让 BLE 扫描拿到这台 Mac 的蓝牙 TCC 授权」这一处拓扑接线，外加配对（今天完全没有）与签名分发。把它们分成三段落地，第一段今天就能做，且零新 App 代码。

---

## 0. 现有零件盘点（都在本仓库，真实路径）

| 零件 | 路径 | 状态 | 特权 | 宿主机上谁来跑 |
|---|---|---|---|---|
| 授权插件 bundle | `native/macos/minimal-auth-plugin/`（`plugin.c` + `install.sh`/`uninstall.sh`） | ad-hoc 签名，**SIP 开启下实测能加载并听从**（[a1-plugin-load](../validation/2026-09-09-a1-plugin-load.md)） | 安装要 root | macOS `_securityagent`（uid 92）或 `authorizationhost`（root，`,privileged`）|
| 健康检查守护 | `minimal-auth-plugin/healthcheck.sh` + `ai.repose.spike.healthcheck.plist` | 真机验证，bundle 被删后约 4s 自动退回纯密码（[E12](../validation/2026-09-09-e12-healthcheck.md)） | root LaunchDaemon | 系统 |
| permit 守护（IPC #3） | `native/macos/permit-daemon/`（`repose-permitd.c` + `install-permitd.sh` + `.permitd.plist`） | 一次性消费 + 对端签名校验；**socket 替换文件读取的终局**，另一个 workflow 在推进 | root LaunchDaemon `ai.repose.spike.permitd` | 系统 |
| BLE 扫描 | `tools/ble-spike/mac/rssi-scan.swift`（已编译 `rssi-scan`） | CoreBluetooth central，实测真机读到手机 | **需要父进程的蓝牙 TCC 授权** | 用户会话进程 |
| 在场判定桥 | `tools/ble-spike/mac/permit-bridge.sh` | 迟滞 + 刷新 permit；`PERMIT_ON/OFF_CMD` 可注入 | touch/rm root-only 文件需 root | 见 §3 |
| 手机广播端 | `tools/ble-spike/android/` | 已装、工作 | — | Android |
| App（宿主） | Tauri v2 + React 19，`src-tauri/`、`src/App.tsx`、`src/tauriBridge.ts` | 托盘常驻；`macos.m:45` 已订阅 `com.apple.screenIsLocked/Unlocked` | 用户会话 | 用户登录态 |
| 「上一版 UI」 | `codex/phone-proximity-unlock` 分支的 `src/components/UnlockSettingsPanel.tsx` + `src/lib/unlock.ts` + `createUnlockBridge()` | **GateClosed 占位**：install 按钮禁用、hardcode `CLOSED_UNLOCK_SNAPSHOT`、文案写「Task 8 GATE CLOSED」 | — | 这就是「函数从未接线」的那版 UI |

**关键既有事实**（决定下面所有选择）：
- **ad-hoc 签名足够加载插件**（SIP 开启的 14.6.1 实测）。所以「个人自用现在就能跑」不需要 Developer ID。Developer ID / 公证是**分发**问题，不是**加载**问题。
- `/Library/Security/SecurityAgentPlugins/` 与 `security authorizationdb write` **不需要关 SIP**（观察到的第三方插件 `com.openai.sky.CUAService.AuthorizationPlugin.remote` 就装在日常 Mac 上）。VM + SIP-off 是 spike 的谨慎，不是宿主机的硬性前提。
- 当前真机验证打通的在场链路是**文件式**：桥 `touch /var/run/repose-spike/permit` → 插件读文件新鲜度。permitd 的 socket 是更安全的终局（门禁 G3），尚未替换文件。

---

## 1. 从「装好 App」到「回车解锁」要发生的完整链条

```
用户装 App
  │  ① 引导安装（一次管理员授权）
  ▼
root 执行：install-permitd.sh（先）+ install.sh（后）
  → 落 bundle 到 SecurityAgentPlugins、备份并改写 system.login.screensaver、
    装健康检查守护、装 permitd 守护
  │  ② 配对（无管理员授权）—— 今天缺这一环
  ▼
手机与 Mac 交换 P-256 密钥（codex 分支已有协议，需整合）
  │  ③ 校准「远」一次（无管理员授权）
  ▼
RSSI 阈值落到 root-only 持久层
  │  ④ 常驻服务起来
  ▼
BLE 扫描（用户会话，持蓝牙 TCC）──RSSI──▶ permit-bridge
    ──present──▶ 刷新 permit（root）──▶ permitd / 插件读到「在场」
  │  ⑤ 真实锁屏 + 回车（演练 B，进入就绪的唯一路径）
  ▼
锁屏提交 → macOS 调用插件 → 插件问 permitd →「在场」→ Allow → 解锁
```

每一环的宿主机落地见下面 §2–§5，对应的 UI 屏与契约见交互设计 §5 / §6。

---

## 2. ① 特权安装：App 怎么在 macOS 上拿到 root

三条路，按「本功能实际需要」排序：

| 方案 | 怎么拿 root | 适配度 | 代价 |
|---|---|---|---|
| **A. `osascript … with administrator privileges`** 包住现有 `install.sh`/`uninstall.sh` | Tauri 后端 shell 出 `osascript -e 'do shell script "…install.sh…" with administrator privileges'`，弹**系统自绘**的管理员授权框 | **推荐（v1）**。与交互设计 §5.5「那个窗口是 macOS 自己画的，不是 Repose 画的」逐字吻合；install/uninstall 是仅有的两次特权操作，各一次弹窗正合适 | 把一个 shell 脚本以 root 跑。**脚本及其目录必须 root 拥有、不可被非管理员改写、用绝对路径调用**，否则等于交出 root。因此脚本必须落在 App bundle 内（签名覆盖）或安装时落到 root-only 目录 |
| **B. SMAppService 特权 helper（旧 SMJobBless）** | 一次性 bless 一个 helper 到 `/Library/PrivilegedHelperTools`，XPC 以 root 驱动 | 仅当需要**反复**特权操作而不想反复弹窗时才值得 | 重：要求 Developer ID、App 与 helper 的 code-sign requirement 互相 pin、内嵌 launchd plist。本功能**用不上**——特权操作只有装 / 卸两次 |
| **C. 文档化的手工 `sudo ./install.sh`** | 用户自己在终端 sudo | **个人自用现在就能用**，零 App 代码 | 不是产品；但它是 Stage 0 的全部所需 |

**ADR-1 安装特权通道**
- Context：宿主机上只有「安装」「卸载」两个特权动作；交互设计要求授权框必须是 macOS 原生、Repose 拿不到密码。
- Options：A osascript-admin / B SMAppService helper / C 手工 sudo。
- Trade-off：B 的持久 helper 是为「频繁特权 IPC」设计的，这里没有这个需求，它带来的 Developer-ID 强耦合反而拖慢个人自用；A 用系统原生授权框，一次一弹，正好覆盖两个动作，且能直接复用已写好、已带 stage-then-swap / 备份 / 校验的 `install.sh`；C 零代码但不是产品。
- Decision：**v1 用 A（osascript-admin 包 install.sh/uninstall.sh）；个人自用当下用 C。B 暂不做**，留给「守护进程注册」若确需从 App 内完成时再评估（见 §3 ADR-2）。
- Consequences：必须保证被 root 执行的脚本不可被非特权用户篡改（落在签名 bundle 内 / root-only 目录，绝对路径调用）；安装中断落入半状态 S9，靠健康检查 + 启动自检收敛。

---

## 3. ④ BLE 扫描常驻 + permit 桥作为服务：本评估最硬的一处拓扑

**矛盾**：在场写入（`touch` root-only 的 permit / 连 permitd）**需要 root**；而 CoreBluetooth 扫描**需要这台 Mac 的蓝牙 TCC 授权**，TCC 授权天然绑用户会话与 App 的签名身份，root LaunchDaemon 没有用户会话、难以弹出 TCC 框。两个需求不能塞进同一个进程角色。

三种接法：

| 接法 | BLE 扫描跑在 | 在场写入怎么拿 root | 问题 |
|---|---|---|---|
| 全塞 root LaunchDaemon | root | 天然是 root | **蓝牙 TCC 在无会话的 root 守护上不可靠**，现代 macOS 对守护进程用蓝牙也要 TCC，且没有交互弹框通道。脆 |
| **拆角色（推荐）** | **用户 LaunchAgent**（继承 / 弹出蓝牙 TCC，和 spike 里 Terminal 拿授权一模一样） | 桥只把「touch/rm」那一步 shell 给 root | 需要一条窄口子让用户态进程触达 root（见下）|
| 扫描=Agent，写入=permitd socket | 用户 LaunchAgent | **permitd 暴露「断言在场」的 socket 调用**，对端校验用户签名后接受 | 最干净的终局，但 permitd 现在只**读文件新鲜度**、不接受在场写入，需要扩协议 |

**窄口子怎么给**（拆角色方案里「touch 那一步拿 root」）：
- 个人自用：`permit-bridge.sh` 的 `REPOSE_PERMIT_ON_CMD` 已经是可注入命令，宿主机上把它从「ssh 进 VM」改成本地 `sudo touch /var/run/repose-spike/permit`，配一条**范围收窄到就这两个路径**的 NOPASSWD sudoers。丑，但当天可用，且**扫描仍在用户态拿 BT 授权、只有 touch 走 root**，职责是分开的。
- 产品：permitd 增加一个「在场断言」请求类型，`repose_peer_verify.c` 已经在取对端 uid / audit session / 代码签名，复用它校验「是 Repose 的 Agent 在断言」，permitd 自己维护在场新鲜度。**这样彻底不再有 root-only 文件被用户态 touch 的需要**，同时把 permit-design §4「world-writable 是新单点」的风险关死。

**R-P3 热扫描**：`src-tauri/native/macos.m:45` 已经订阅 `com.apple.screenIsLocked`。Agent 侧也应订阅同一个分布式通知，在锁屏瞬间把扫描拉满、保持缓存热，压低「唤醒后第一次回车撞上扫描冷启动」的失败率（交互设计 §2.4 R-P3、§6.5）。

**失效即锁**：Agent 崩溃 / 退出 / 蓝牙关 → permit 不再刷新 → 15s 内过期（`plugin.c:55 PERMIT_FRESHNESS_S`）→ fail-closed 回到密码。这正是 `permit-bridge.sh` 刷新式设计（非 write-once）要的性质，宿主机上原样成立。

**Tauri sidecar 还是 LaunchAgent？**
- sidecar（`externalBin`）作为 App 子进程：TCC 归属 App（在 `Info.plist` 写 `NSBluetoothAlwaysUsageDescription`，App 弹一次蓝牙框），干净；但**只在 App 运行时在**。Repose 是托盘常驻 App，这点可接受。缺点：用户从托盘退出 App，手机钥匙就停了——而这正是交互设计 §5.16「托盘是它在 App 之外唯一可见面」想避免的脆弱。
- LaunchAgent（用户登录即起，独立于 App 生死）：更稳，登录态就在；TCC 归属该 Agent 二进制自身的签名身份。
- **ADR-2 扫描进程形态**：v1 用 **LaunchAgent**（登录常驻，不随 App 退出而停，符合「忘了装过它也还在工作」）；在场写入走 §3 的窄口子 / permitd。sidecar 仅在「只想个人快速验证、不在意退出即停」时用。

**TCC 与签名的耦合**：TCC 授权以 bundle id + 签名身份为键。**ad-hoc 每次重编 cdhash 都变 → 蓝牙授权每次重装都要重新弹框授权**。个人自用能忍；分发必须 Developer ID 才能让 TCC 授权稳定跨版本存活（见 §6）。

---

## 4. ② 配对特定手机：今天完全没有，这是安全门槛不是锦上添花

当前在场判定是「**任何**广播公共 UUID `7265706F-7365-0001-…` 的设备都算在场」——**没有认证**。宿主机上如实说就是：**任何装了广播 App 的 Android 手机靠近，都能回车解锁任何装了本插件的 Mac。** 个人单人 spike 可以接受（只有你的手机在跑），但**分发前这是硬门槛**，否则交互设计 §5.3「它认的是手机，不是你」这句承诺连「手机」都保证不了。

**已有资产**：`codex/phone-proximity-unlock` 分支的 `mobile/packages/repose_unlock_native/`（`ChallengeVerifier.kt`、`AndroidPhoneResponder.kt`、`P256SignatureCodec.kt`、`TrustedPairing.kt`、`AndroidKeyStoreSigner.kt`）+ Mac 侧 `native/macos/permit-daemon/repose_peer_verify.*` 已是 P-256 握手的形状，且带测试。permit-design 文档也点名这几个 crate 值得保留。

**落地 = 跨分支整合**，不是从零写：把握手协议接到「permitd 判定在场时要求一次签名应答」上。但要注意预算：发现 1.9s + GATT 连接读 1.5s 已超 3s 预算（交互设计 §9.1），所以握手不能放进每次解锁的热路径，只能放进**配对**；日常在场判定仍靠广播 + 滚动身份（rotating identity），这需要另设计，超出本评估范围，列为 A3 之后的依赖。

---

## 5. ③ 校准 + 持久层

- 校准只采「远」一次（交互设计 §5.8），产出一个可分离判定 + margin。阈值默认值**今天没有数据**（§9.5 / B3）。宿主机上先用 `permit-bridge.sh` 里那组保守占位（`NEAR=-72 / FAR=-85`，绑 MEDIUM tx power），宁可多输一次密码也不误开。
- **R-P7**：配对密钥、校准参数、安装时的 macOS build 与 cdhash **一律不进 localStorage**，要 Rust 侧新建 root-only / Keychain 持久层。这是宿主机上的新建工作（现 App 只有 `localStorage` 存偏好）。

---

## 6. 在日常 Mac 上装授权插件的风险与回退

### 6.1 风险（按严重度）

| # | 风险 | 后果 | 缓解 | 残余 |
|---|---|---|---|---|
| 1 | **E3 fail-open** | bundle 在规则仍引用时消失（拖进废纸篓 / 升级中断 / 迁移），锁屏**空密码即可绕过**（[E3](../validation/2026-09-09-e3-fail-open.md)）。E8/E11 证明没有任何规则形态既免密又缺失即 fail-closed | 健康检查守护（E12）：开机 + `WatchPaths` + 慢定时，发现悬空约 4s 退回纯密码 | **机器已锁着时 bundle 消失，没有任何东西能在解锁前修复**——这是「便利功能反而让 Mac 更开」的唯一窗口。日常机无 VM 快照，这条权重更高 |
| 2 | 把自己关在门外 | 规则写坏导致锁屏解不开 | `k-of-n=1` 保住密码路径；备份落 `/var/db`（跨重启存活）；E4 重启后的 `system.login.console` 不受影响 → **重启必能进**；`uninstall.sh` + 离线 `RESTORE.txt` | 无快照时，兜底只剩「重启 + 离线卸载 + 健康检查」三条，栈更浅 |
| 3 | **permit 是全局可写开关** | `/tmp/repose-permit`（旧 spike 路径）1777，任意本地 uid `touch` 即越过锁屏 | 已移到 `/var/run/repose-spike/`（root-only）+ root 属主 + 15s 新鲜度；终局换 permitd socket（G3） | **绝不能分发 `/tmp` 版本**；socket 必须 `0600`/`0660`，装后由健康检查断言权限位（R-P5） |
| 4 | 给 root shell 的篡改面 | osascript-admin 跑的脚本若可被非管理员改写 = 提权 | 脚本落签名 bundle / root-only 目录，绝对路径调用 | 随方案 A 引入，必须在实现里守住 |
| 5 | macOS 升级后不再加载 | 插件失效 | 状态降级回「还没试过」(S5)，主动提示锁屏重验（交互设计 S6→S5） | 升级当天静默失效，靠演练 B 肌肉记忆兜底 |
| 6 | 与其他授权插件共存 | 规则里已有第三方条目（本机就有 CUAService） | 安装 preflight 要求 `class=rule` 且 `k-of-n=1`，只 prepend、不动别人条目；`authdb-edit` 外科式增删 | F8 信息披露 |

### 6.2 回退故事（已成体系，且**不依赖 App 还活着**）

1. `uninstall.sh`：按校验过的备份**逐字还原** `system.login.screensaver`、删子规则、删 bundle、删两个守护、清 permit。备份用 `-s` 判空、`authdb-edit validate` 校验，拒绝用零字节备份覆盖。
2. 备份丢失（F7）：外科式只摘自己那条；形态 B 且无他人条目时按**硬编码 stock 常量**换回 `use-login-window-ui`（交互设计 §5.12）。
3. **离线路径（G7）**：`uninstall.sh` + `RESTORE.txt`（含安装前规则全文与 stock 常量）+ right 的 `comment` 键都落 `/var/db/repose-unlock/`，**删掉 App 不影响**。`sudo /var/db/repose-unlock/uninstall.sh` 一条命令可撤。
4. 健康检查守护：悬空引用自动退回纯密码，先停守护再动 bundle（uninstall 的 ordering）避免自己和守护抢写。
5. 日常机兜底栈（无快照）：健康检查（秒级）→ 重启进 console 登录（E4）→ 离线 `uninstall.sh`。

---

## 7. 分段落地路线

| Stage | 目标 | 签名 | App 代码 | 配对 | permit | 谁能用 |
|---|---|---|---|---|---|---|
| **0 今天 · 个人 · 零 App 改动** | 在自己 Mac 上跑通整条链 | ad-hoc（本地 `make`） | 无 | 无（只你的手机在广播） | 文件式 | 只有你，手工 |
| **1 A3 核心 · 个人 · 接进 App** | 引导安装 + 实时状态 + 开关 + 配对入口，接进现有 UI | ad-hoc | 有（见 §8） | 最小 / 占位 | 文件式，permitd 并行推进 | 你，图形化 |
| **2 分发** | 公开可下载安装 | **Developer ID + 公证** | 完整 | **P-256 握手整合（跨分支）** | **permitd socket（G3）** | 任何人 |

**Stage 0 的全部步骤**（宿主机，已全部有脚本）：
1. `cd native/macos/minimal-auth-plugin && make`（ad-hoc 签）
2. `cd native/macos/permit-daemon && ./build.sh && sudo ./install-permitd.sh`（socket 终局；或先跳过用纯文件式）
3. `sudo ./install.sh`（装 bundle + 改规则 + 健康检查）
4. 终端里 `./rssi-scan | permit-bridge.sh`，其中把 `REPOSE_PERMIT_ON_CMD` 指到本地 `sudo touch /var/run/repose-spike/permit`、`REPOSE_PERMIT_OFF_CMD` 指到本地 `sudo rm -f …`（授予终端蓝牙权限）
5. 手机开广播；锁屏 → 回车。
6. 撤：`sudo ./uninstall.sh` + `sudo launchctl bootout system/ai.repose.spike.permitd`

**Stage 2 为什么非 Developer ID 不可**（而 Stage 0/1 不需要）：
- **加载**插件：ad-hoc 在 SIP 开启下实测够（A1）。
- **分发**：下载的 .app 带 quarantine 属性，Gatekeeper 要求公证；ad-hoc 的 quarantine 包里的插件会被拦。
- **TCC 稳定性**：蓝牙授权以签名身份为键，ad-hoc 每次重编 cdhash 变 → 每次重装重新授权；Developer ID 让授权跨版本存活。
- **守护注册**：若想从 App 内用 SMAppService 注册守护（替代手工 `launchctl bootstrap`），SMAppService 要求 Developer ID。

---

## 8. 让它在宿主机上成真的「最小改动」

**纯机制层（Stage 0，今天）**：上面 §7 的第 4 步就是全部缺口——**把 `permit-bridge.sh` 的在场写入命令从「ssh 进 VM」改成本地 root 的 touch/rm，并让扫描进程拿到这台 Mac 的蓝牙授权。** 三块已验证零件（插件 / 健康检查 / BLE→桥）立刻在宿主机上合拢，不需要任何新机制。

**接进 App（Stage 1，A3 核心）最小增量**，按「先只读、再特权、再常驻」排：
1. **只读快照命令**（无特权，风险最低，价值最高）：Rust 侧实现 `unlock_get_snapshot`（交互设计 §6.2）——读 `security authorizationdb read`、bundle 存在 + cdhash、socket 权限位、守护 `launchctl print`、蓝牙/TCC 状态，带 `readAt`。**它把「上一版 UI 的 GateClosed 占位」换成真实数据**，让面板从「安装尚不可用」变成活的状态行（交互设计 §5.10）。这一步不改系统、不拿 root，是 A3 最安全的起点。
2. **引导安装 / 卸载**：`unlock_install` / `unlock_uninstall` 经 §2 方案 A 的 osascript-admin 包 `install.sh`/`uninstall.sh`，一次授权内完成，完成后**当场重读**回填 `UninstallReport`（§6.2）。
3. **常驻服务**：App 安装时落一个用户 LaunchAgent 跑 `rssi-scan | permit-bridge`（§3 ADR-2），在场写入走窄口子 / permitd。
4. **桥接层对齐**：`src/tauriBridge.ts` 要把新的 `repose-command` 取值（`unlock-drill-passed` 等）**同时**加进 `DesktopCommandName` 和 `commandNames` Set，否则白名单静默丢弃（交互设计 §6.4 的显式警告）；`window.repose` 扩出 unlock 命令族。
5. `src/App.tsx` 把 `codex` 分支的 `UnlockSettingsPanel` 接进来，但**契约要重对齐**——见下面的未决。

**注意：两版后端契约不一致，必须先收敛（否则接不上）。** `codex/phone-proximity-unlock` 的 `src/lib/unlock.ts` 是 `UnlockSnapshot{ capability, authorizationGate, components{policy,plugin,service,transport} }` + `UnlockErrorCode` 六值；交互设计 §6 是另一套 `UnlockSnapshot{ state(11 态), presence, components[], componentInvocation, lastFailure }` + `UnlockError{code,detail,remediation,evidence}` + `Remediation` 八原语。**「上一版 UI 美观、只是没接线」为真，但它接的是旧契约的占位数据；A3 要么把 UI 升级到新契约，要么让后端同时发旧契约——前者正确。** 这是 A3 第一个要做的决定，不在本评估内定案。

---

## 9. 待定（宿主机相关，需单独回答）

- **ADR-Pending-1 `,privileged` 与对端 pin**：E7 已确认特权变体以 root（authorizationhost）被调用；非特权变体以 `_securityagent` uid=92 被调用。两条路 socket 权限与 `repose_peer_verify.c` 的签名 pin 不同，安全性质不同（交互设计 §9.7 / permit-design）。**需决策走哪条**再定 permitd 的接入细节。
- **在场判定的旋转身份**：日常判定不能做 GATT 握手（超 3s 预算），又必须认证（否则任意手机可解）。配对用 P-256，但**日常广播怎么携带可轮换的签名身份**尚无设计（交互设计 §9.1）。这是配对整合的真正难点。
- **校准默认阈值**（B3 / §9.5）：宿主机多机型采样前，只有保守占位。
- **唤醒按键是否落进密码框**（§9.8）：手机不在场时会变成一次失败密码尝试，影响「不输任何字符」这句承诺。写进 README 前需十分钟手工验证。
- **LaunchAgent vs permitd 的在场写入协议**：若选 permitd 扩「在场断言」请求，需定义 wire 格式与对端校验，复用 `repose_permit_wire.h` / `repose_peer_verify.c`。

---

## 附：本评估引用的真实符号 / 路径

- 插件判据与超时：`native/macos/minimal-auth-plugin/plugin.c:49`（`PERMIT_PATH`）、`:55`（`PERMIT_FRESHNESS_S 15`）、`:68`（`PERMIT_TIMEOUT_MS 1500`，交互设计 R-P1 要求压到 250ms）
- 安装 / 回退：`install.sh`（stage-then-swap、`/var/db/repose-spike` 备份、健康检查装载）、`uninstall.sh`（`backup_usable` 校验、先停守护再动 bundle）
- 在场桥：`tools/ble-spike/mac/permit-bridge.sh`（`REPOSE_PERMIT_ON_CMD`/`OFF_CMD` 注入点、`NEAR_DBM`/`FAR_DBM`/`STALE_S`/`REFRESH_S`）
- BLE：`tools/ble-spike/mac/rssi-scan.swift`（`serviceUUID`、`STATE unauthorized` 提示把蓝牙授权给父终端）
- permitd：`native/macos/permit-daemon/repose-permitd.c`、`repose_peer_verify.c`、`ai.repose.spike.permitd.plist`（root、`KeepAlive`、自建 socket `0660 root:_securityagent`）
- 锁屏信号：`src-tauri/native/macos.m:45`（`com.apple.screenIsLocked`）→ R-P3 热扫描
- App 桥：`src/tauriBridge.ts`（`window.repose`、`DesktopCommandName`/`commandNames` 白名单）、`src/App.tsx:113,117,291,387-390`（`window.repose` 降级、现有安全锁屏面板）
- 上一版 UI：`codex/phone-proximity-unlock:src/components/UnlockSettingsPanel.tsx`、`src/lib/unlock.ts`（`CLOSED_UNLOCK_SNAPSHOT`、`createUnlockBridge`）
- 权威背景：[unlock-interaction-design](2026-09-09-unlock-interaction-design.md) §6/§8、[permit-design](2026-09-09-permit-design.md)、验证记录 [A1](../validation/2026-09-09-a1-plugin-load.md) / [E3](../validation/2026-09-09-e3-fail-open.md) / [E11](../validation/2026-09-09-e11-lockscreen-grant-model.md) / [E12](../validation/2026-09-09-e12-healthcheck.md)
