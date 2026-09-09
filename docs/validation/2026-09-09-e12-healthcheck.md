# E12 —— 健康检查守护进程：真机验证，fail-open 窗口自动关闭

日期：2026-09-09

状态：**已实现并在真机验证（CONFIRMED）**。这是发布门禁 G2 的解法。

## 背景

E3/E8/E11 得出：`system.login.screensaver` 引用我们的机制、而 bundle 缺失时，锁屏 fail-open
（空密码可绕过）；且**没有任何规则形态**能同时做到免密与「缺失即 fail-closed」。唯一的防线是
**预防悬空引用**。install.sh / uninstall.sh 已保证脚本路径不留悬空引用；E12 守护未脚本化的路径：
用户拖废纸篓、升级中断、系统迁移清空 `SecurityAgentPlugins`。

## 构成

| 文件 | 作用 |
|---|---|
| `healthcheck.sh` | 读规则；若引用了 `ai.repose.spike` 而其机制 bundle 缺失或 `codesign` 不过 → 用 `authdb-edit remove-subrule` 把子规则从 authdb 移除，退回纯密码。**只在确认悬空/失效时才改；读不到 authdb 就什么都不做。** |
| `ai.repose.spike.healthcheck.plist` | LaunchDaemon：`RunAtLoad`（开机查一次）、`WatchPaths=/Library/Security/SecurityAgentPlugins`（目录一变就触发）、`StartInterval=3600`（慢兜底）。 |
| install.sh / uninstall.sh | 安装期把两个脚本装到 `/Library/Application Support/ReposeSpike/`（root:wheel）、plist 装到 LaunchDaemons、`launchctl bootstrap`；卸载期**先** bootout（避免与卸载抢着改规则）再清除。 |

修复复用安装器同一个 `authdb-edit`，它**拒绝**把规则改到没有密码兜底——健康检查即使误判也不会把人锁死。

## 真机端到端验证（一次性 VM，全程 ssh）

用**真实的 install.sh**（非 ad-hoc 脚本）安装，然后：

1. **安装**：`install.sh` 装好 bundle + 规则 + 守护进程；`launchctl print` 显示守护已加载、`RunAtLoad`。
2. **WatchPaths 自动触发**（关键）：`sudo rm -rf …/ReposeSpike.bundle`（模拟拖废纸篓），**不手动跑任何东西**。
   - 起点规则 `["ai.repose.spike","use-login-window-ui"]`；
   - **约 4 秒后**规则自动变成 `["use-login-window-ui"]`；
   - 守护日志：`referenced bundle is MISSING` → `REPAIRED: … screensaver is now password-only`。
3. **VULN→SAFE**（此前用手动跑健康检查也验过）：bundle 缺失时 `security authorize system.login.screensaver`
   无凭据 = `YES (0)`（fail-open）；健康检查修复后 = `NO (-60007)`（要密码）。
4. **卸载**：`uninstall.sh` bootout 守护、删 plist 与支持目录、从备份还原规则到出厂
   `["use-login-window-ui"]`、移除 `ai.repose.spike` right、删 bundle。核对全部「已移除」。
5. **重装**：回到守护运行、报 healthy 的可演示状态。

顺带确认：`/var/db/repose-spike` 的备份是**纯净的装前规则**（`["use-login-window-ui"]`），
之前对备份缺失的存疑就此消除。

## 覆盖的失败模式（本地沙箱测试 `tests/healthcheck_test.sh`，8/8）

- 健康安装不动；bundle 缺失 → 移除子规则、保留密码路径；未引用则不动；
- **读不到 authdb → 什么都不写**（绝不猜）；
- bundle 在但 `codesign` 不过 → 视为不可加载、修复；
- 子规则被引用但其机制读不出 → 判悬空、修复；
- **移除会导致无密码兜底时，`authdb-edit` 拒绝、健康检查中止并报错**（绝不锁死）；
- `bundle_names` 正确排除 `builtin:`/`loginwindow:` 等系统机制宿主。

`tests/install_invariants_test.sh` 增加 7 项：install 引导守护、装 healthcheck.sh、uninstall
bootout、**bootout 早于删 bundle**（避免竞态）、健康检查委托 authdb-edit、不依赖 python3、
读不到规则就不动。全套 `make test`：契约 + authdb-edit + invariants(29) + healthcheck(8) 全绿。

## 局限（诚实记录）

- 守护**无法**在「机器已锁 + 引用刚悬空」的那一瞬补救——机器已经锁了，任何进程都来不及。
  它的价值是把「bundle 消失」到「规则修复」之间的**窗口**压到最小（WatchPaths 实测约数秒）。
  开机期 `RunAtLoad` 覆盖「关机状态下 bundle 被删、开机即处于悬空」的情形。
- `codesign` 校验对「bundle 在但签名失效」偏保守：会退回纯密码。这是**安全方向**的误动作
  （最坏是本该免密却要了密码），可接受。**E9 已测**：给二进制追加垃圾字节使 `codesign --verify`
  报 “main executable failed strict validation”，但 SecurityAgent **仍加载并运行了机制**
  （permit 不在 → Deny → authorize `NO`）——即「签名坏但仍可加载」**不** fail-open。fail-open
  只发生在 bundle **缺失**（机制无法实例化）。因此健康检查的「存在性 + codesign」是所有
  fail-open 诱发态的**超集**（缺失由存在性挡，真正不可加载的损坏过不了 codesign），不漏；代价是
  对「坏签名但仍可加载」偶有保守误动作。未覆盖的边角：bundle 在、codesign 通过、但
  `AuthorizationPluginCreate` 运行期失败——属运行期而非加载期问题，另议。
- WatchPaths 有约几秒延迟，且极端情况下可能漏事件——`StartInterval` 兜底。

## 结论

G2 的解法从「用规则结构关闭 fail-open」（E11 证明不存在）改为「预防悬空引用 + 运行期自动修复」，
现已实现并在真机验证：拖废纸篓后 fail-open 窗口在数秒内自动关闭，且卸载不留残留、不锁死。

## 待办

- 开机竞态的进一步收紧（是否有比 RunAtLoad 更早的挂载点）——目前认为窗口已足够小。
- ~~把 `credential` 死路模式从插件里清掉~~ **已移除**（2026-09-09）。
- 运行期失败（bundle 在、codesign 过、但 `AuthorizationPluginCreate` 失败）是否 fail-open——
  属运行期而非加载期，单独验。
