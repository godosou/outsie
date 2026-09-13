# E4 & E7 —— 两个焦点无关的收尾验证

日期：2026-09-09

## E4 —— 只改屏保规则，重启登录窗口不受影响（结构性确认）

状态：**结构性确认（CONFIRMED by construction）**。

「插件出任何问题，重启一定能进」这条兜底，靠的是我们**从不碰**重启登录用的
`system.login.console`。核实：

- `install.sh` 全程只有一个 `RIGHT="system.login.screensaver"`（第 16 行），加子规则、备份、还原都只针对它。
- VM 实测：`system.login.console` 里 `ai.repose.spike`、`ReposeSpike` **各 0 处**引用，仍是 stock
  `evaluate-mechanisms`；而 `system.login.screensaver` 有 1 处（对照正确）。

所以重启后的登录窗口用的是一条我们从未修改的规则，兜底成立。经验性的「重启并用密码登入」是
锦上添花的确认，但结构上已经足够硬。

## E7 —— `,privileged` 变体到底被不被调用？被调用，以 root 运行（**推翻旧结论**）

状态：**已解决（CONFIRMED）**，并**收回**此前「`,privileged` 从不被调用」的说法。

此前认为特权变体不被调用，那来自几次**输入被饿死**的 GUI 实验（宿主机锁屏 / 按键没落到客户机，
结果作废）——与本项目其它已收回结论同一类的仪器错误。

重测（把子规则机制临时换成 `ReposeSpike:permit,privileged`，放 root 新鲜 permit，`security authorize`）：

```
uid=0 AuthorizationPluginCreate hostVersion=4
uid=0 MechanismCreate id=permit mode=permit
uid=0 permit: /var/run/repose-spike/permit present, root-owned and fresh after 0ms
uid=0 MechanismInvoke result=Allow          -> security authorize: YES (0)
```

特权变体**确实被调用**，且以 **uid=0（root，运行在 `authorizationhost`）**。至此机制宿主模型完整：

| 变体 | 宿主 | uid |
|---|---|---|
| 非特权 `ReposeSpike:permit` | SecurityAgent | 92（真实锁屏）/ 调用者 uid（CLI）|
| 特权 `ReposeSpike:permit,privileged` | authorizationhost | **0（root）** |

（本次是 CLI 路径观测到 uid=0；真实锁屏 GUI 下特权变体是否同为 root 属另一次确认，但「被调用」已定。）

## 对 IPC 终局的直接影响（peer identity pin）

`docs/plans/2026-09-09-permit-design.md` 记过一个待定：`peer_identity.rs` 把对端 pin 成
`com.apple.authorizationhost`，而 A1 实测非特权变体是 `uid=92 _securityagent`——当时判为「pin 错了」。
E7 让这条有了清晰的二选一：

- **若产品用特权变体**：机制以 root 跑在 authorizationhost → 对端就是 authorizationhost，
  codex crate 原本的 `com.apple.authorizationhost` pin **本就正确**；IPC socket 可为 `0600 root`
  （root 客户端），最简单。
- **若产品用非特权变体**：对端是 `uid=92 _securityagent` → pin 要改成 SecurityAgent 的签名标识，
  socket 需允许 uid 92 连接（`0660` + 受限组）。

两条路安全性质不同（特权变体权限更大、影响面更大；非特权更小权限但守护要接受非 root 对端），
值得单独决策；但 E7 之后，**特权变体是可用的**，且让 IPC 对端识别最简单。

## 还原

E4 纯读；E7 用 `trap restore EXIT` 把子规则还原回 `["ReposeSpike:permit"]`（已确认）。VM 干净。
