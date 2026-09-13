# E10 —— 机制向 context 注入凭据让 builtin:authenticate 静默通过：已在锁屏上证伪，收回

日期：2026-09-09

> **已收回 → 见 [E11](2026-09-09-e11-lockscreen-grant-model.md)。**
> 后来在真实锁屏上做了 A/B：手机在场时机制 `Allow` **本身就解锁**，`builtin:authenticate`
> 不会运行，注入正确与错误凭据结果相同。**E10 的前提（注入让 builtin 静默通过）在锁屏不成立，
> 无的放矢。** 本文以下为当时「部分完成」阶段的记录，保留以存证；结论以 E11 为准。

状态（历史）：**部分完成 → 已收回**。注入 API 本身可用（`SetContextValue` 返回成功），
但「让 `builtin:authenticate` 静默通过」这个目标在锁屏上不成立——见 E11。

## 背景

[E8](2026-09-09-e8-failopen-is-intrinsic.md) 得出：唯一能同时做到「手机在场免密」和
「插件缺失要密码（fail-closed）」的形态，是把我们的机制放进
`[ReposeSpike:credential, builtin:authenticate]` 这样的必经链，让机制在手机在场时
**向授权 context 注入用户名/口令**，使其后的 `builtin:authenticate` 静默通过。E10 就是
验证这条路能不能走通。

## 做了什么

给插件加了 `credential` 模式（`plugin.c`，与既有 `log`/`permit` 并存、向后兼容）：

- 手机在场（permit 在）且能读到凭据 → `SetContextValue` 注入
  `kAuthorizationEnvironmentUsername` / `kAuthorizationEnvironmentPassword`
  （`kAuthorizationContextFlagExtractable`），然后 `Allow`（在链里 = 继续下一机制）。
- 手机不在或无凭据 → 不注入，仍 `Allow`（让 `builtin:authenticate` 照常弹密码框）。
- **永不 Deny**：在必经链里 Deny 会掐断密码入口。缺失 bundle 被 authd 跳过、同样落到密码。

凭据来源在原型里是一个文件 `/tmp/repose-credential`（两行：用户名、口令）。**这是原型脚手架，
不是产品**——明文放在 world-readable 的 /tmp 恰恰是产品必须避免的；它替代的是「向持有
keychain 凭据的 root 守护进程发一次请求」。

## 命令行实测：注入通，消费无法判定

用 `security authorize system.login.screensaver`（无 `-C`/`-u`）在 E10 链上测，permit 在场：

| 注入内容 | 结果 | 用时 |
|---|---|---|
| 临时管理员 e10test 的**正确**口令 | `exit=142`（超时） | 8041ms |
| e10test 的**错误**口令 | `exit=142`（超时） | 8034ms |

插件日志两次都是 `credential: injected username(len=…) password(len=…) setctx u=0 p=0`
—— **`SetContextValue` 对用户名和口令都返回 0（成功）**，注入侧完全正常。

但**正确与错误口令结果一模一样**（都超时）。若 `builtin:authenticate` 读了注入的 context，
正确口令应秒回 `YES`、错误口令应秒回 `NO(-60005)`；两者无差别，说明它在**命令行路径下
根本没读 context**，而是试图弹 UI 直到超时。

（用临时管理员 e10test 是为了在**不知道 admin 真实口令**的情况下拿到一组已知正确的凭据；
测完即删。）

## 为什么命令行到此为止

两种解释，都指向「决定性验证只能在锁屏做」：

1. **`security authorize` 命令行路径本就不做静默认证**——它不跑 SecurityAgent 的完整 GUI 流程，
   `builtin:authenticate` 在这里总是要交互。日志里的 `uid=501`（我的 ssh 会话）而非锁屏的
   `uid=92`（`_securityagent`）也印证这是另一条上下文。
2. `builtin:authenticate` 读的是 **hints** 而非 context（可能性较低：context 注入是社区里
   记录为「在登录窗口有效」的标准做法）。

无论哪种，命令行都无法证明或证伪静默认证。**注入 context 是登录窗口上被记录为有效的标准手法**，
所以下一步是直接在真实锁屏验证，而那需要把 **admin 的真实口令**写进凭据文件。

## 决定性验证怎么做（待定，需真实口令 + 一次 GUI）

在真实锁屏上，permit 在场、凭据文件 = admin 的**正确**用户名+口令、规则为 E10 链，唤醒后观察：

- **不输入即解锁** → E10 成立，fail-closed 且免密的形态可行，G2 有解。
- 仍要求输入 → context 注入在锁屏也不被消费，需改用 hints 或换 builtin:authenticate 的变体
  （含 `,privileged`，但那受 E7 阻塞）。

口令应由使用者自己写入凭据文件，不必告诉自动化；自动化只负责摆好规则/permit 并触发唤醒。

## 已确定的事实（不受上面待定项影响）

1. **非特权 `ReposeSpike:credential` 机制会被调用并运行**（日志 `MechanismCreate id=credential`），
   不像 E7 里那个从不被调用的 `,privileged` 变体。
2. **`SetContextValue` 在该机制里返回成功**——注入 API 本身可用。
3. 机制「永不 Deny、只决定是否预填」的语义，与 E8b 的 fail-closed 链是自洽的。

## 待办

- **E10-决定性**：真实锁屏 + 真实口令下验证静默解锁（需一次 GUI 注入 + 口令由使用者写入）。
- 若 context 不被消费：试 `SetHintValue`，或含 `builtin:reset-password,privileged` 的完整链，
  或解开 E7（privileged 变体为何不被调用）后走特权注入。
- 凭据来源从明文文件换成向 root 守护进程发请求（与 permit 的 IPC 改造合并，见
  `docs/plans/2026-09-09-permit-design.md`）。
