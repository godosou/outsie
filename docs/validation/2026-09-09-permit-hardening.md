# permit 加固（文件式中间态）—— 真机验证

日期：2026-09-09

状态：**已实现并真机验证（CONFIRMED）**。这是 permit 的**中间态**，关掉四个缺口里的三个；
第四个（绑定到具体解锁尝试）留给 IPC 终局（见 `docs/plans/2026-09-09-permit-design.md`）。

## 改了什么

早期 walking skeleton 的 permit 是 `/tmp/repose-permit`：只要文件存在就解锁。`/private/tmp` 是
1777，**任何本地 uid `touch` 一下就越过锁屏**。加固后：

- 路径移到 **`/var/run/repose-spike/`**，目录 `root:wheel 0755` —— 只有 root 能在其中建文件；
  `/var/run` 开机清空，permit 不跨重启残留。
- 插件（`plugin.c` 的 `wait_for_permit`）要求 permit：是**常规文件**、**root 属主**（`st_uid==0`）、
  且 **mtime 新鲜**（≤ `PERMIT_FRESHNESS_S=15s`，容忍 `PERMIT_SKEW_S=5s` 时钟偏差）。
- 写入方（BLE 桥 / 测试用 ssh sudo）必须以 root **持续刷新** permit；停刷（崩溃/丢手机）→ 过期 → 拒绝。
- 插件读 `REPOSE_PERMIT_PATH`（若设）——仅作测试缝，生产默认走 root-only 路径；SecurityAgent 的
  环境由系统设定，非用户可控，且默认比原来的 /tmp 严格，override 与否都不更弱。

## 真机验证矩阵（一次性 VM，命令行、焦点无关）

`security authorize system.login.screensaver` 无凭据，k-of-n=1 下：机制 Allow→YES，Deny→落到密码→NO。

| 场景 | permit | 结果 | 日志 |
|---|---|---|---|
| A | root 属主 + 新鲜 | **YES** | present, root-owned and fresh |
| B | 非 root 尝试创建 | 建不了 | `Permission denied`（目录 0755 root）|
| C | 非 root 属主（root 建后 chown admin）| **NO** | `not root-owned (uid=501)` |
| D | root 属主但过期（-600s）| **NO** | `stale (600s old > 15s)` |
| E | 无 permit | **NO** | absent after … |

## 关掉了哪些缺口（对照 permit-design.md 的四条）

| # | 缺口 | 状态 |
|---|---|---|
| 1 | `/tmp` 世界可写，任何 uid 越锁 | **已关**：非 root 在 root-only 目录建不了文件（场景 B）；且即使有非 root 文件也被拒（场景 C）|
| 2 | 无时效，permit 永久有效 | **已关**：过期即拒（场景 D）|
| 4 | fail-open 方向：源崩溃残留=一直放行 | **已关**：停刷 → 过期 → 拒绝 = fail-closed 方向 |
| 3 | 不绑定本次解锁尝试，可重放 | **未关**：时效窗内、同机可重放。需 IPC 请求-响应终局 |

## 单元覆盖

`tests/plugin_contract_test.c`（`make test`，非 root 26 项全绿）新增：**非 root 属主 permit → 拒绝**、
**过期 permit → 拒绝**；「root 属主新鲜 → 放行」在非 root 下跳过（真机验收覆盖），root 下运行会执行。
契约测试通过 `REPOSE_PERMIT_PATH` 指到可写临时路径来在无 root 下驱动这些用例。

## 与 BLE 桥（B2）如何衔接

这个「root 持续刷新 permit」正是真实 BLE 桥的形状：桥以 root 跑、持 BLE，手机在近场时每隔几秒
`touch /var/run/repose-spike/permit`（或经 ssh 写进 VM），手机离开或桥崩溃则停刷 → 过期 → 锁定。
所以加固与 B2 天然对齐。

## 终局（未做）

`docs/plans/2026-09-09-permit-design.md` 的 IPC：插件不读文件，改向 root 守护进程发一次
请求-响应，把「一次响应绑定一次解锁尝试」做成原子消费，关掉第 3 条。复用
`codex/phone-proximity-unlock` 的 `repose-unlock-ipc` / `permit_broker.rs` / `peer_identity.rs`
（后者的对端签名 pin 需按 A1 实测的 `uid=92 _securityagent` 改）。
