# E1：安装器实际生成的规则形态能否触发插件链

日期：2026-09-09 14:00

结论：**能。两个方向都验证通过。不需要修改 `use-login-window-ui`。**

## 为什么要做这个实验

A1 通过时用的规则是 `[ai.repose.spike, authenticate-session-owner-or-admin]`，
而那是**手工换过**的形态。安装器（`authdb-edit add-subrule`，前置插入并保留原条目）
实际生成的是 `[ai.repose.spike, use-login-window-ui]`。

**被验证的形态安装器不会生成，安装器生成的形态没被验证过。**
这个缺口是 A2 设计评审时由三份独立批评共同发现的。

而 A1 记录里「stock 规则下插件链根本不参与」那条结论，依据是 Apple 在规则 comment 里的
一句话加上三轮毫无动静的实验 —— 但那三轮全部发生在输入没有送达 VM 的时间段内，
没有提交过任何解锁尝试，对任何规则形态都不构成证据。

## 方法

卸载还原到 stock（`[use-login-window-ui]`，无 `k-of-n`），然后**原样运行安装器**，
不做任何手工修改。得到：

```
rule    = [ai.repose.spike, use-login-window-ui]
k-of-n  = 1
机制    = ReposeSpike:permit（非特权）
```

锁屏后由宿主机注入按键：关屏保 → 清空输入框 → 回车（空密码）。

## 结果

**permit 不在场：**

```
2026-09-09T14:00:48.928 pid=9622 uid=92 MechanismInvoke enter mode=permit
2026-09-09T14:00:50.561 pid=9622 uid=92 permit: /tmp/repose-permit absent after 1600ms, giving up
2026-09-09T14:00:50.561 pid=9622 uid=92 MechanismInvoke result=Deny
```
→ 保持锁定。

**permit 在场：**

```
2026-09-09T14:02:04.334 pid=9668 uid=92 permit: /tmp/repose-permit present after 0ms
2026-09-09T14:02:04.334 pid=9668 uid=92 MechanismInvoke result=Allow
```
→ 解锁（密码为空）。

## 对产品的影响

| | E1 之前的假设 | 实测 |
|---|---|---|
| 是否要改 `use-login-window-ui` | 必须改成 `authenticate-session-owner-or-admin` | **不需要** |
| 锁屏界面绘制方 | 从 loginwindow 换成 SecurityAgent，用户会察觉 | **不变** |
| 「谁的密码能解锁这台机」 | 语义可能扩大到任意管理员 | **不变** |
| 安装引导 | 需要一整屏披露该系统改动 | **该屏不存在** |

交互设计文档里为此准备的两条分支，确定走**形态 A**（无视觉变化、无语义变化）。
系统改动缩小为：往 screensaver 规则数组前面插一条自己的子规则，并在原本没有 `k-of-n`
的单条目规则上把它设为 1（「全部 1 条通过」与「1 条中通过 1 条」语义相同）。

## 仍未回答

- **E2 作废**：形态 B 不再需要，`authenticate-session-owner-or-admin` 的语义问题不存在了
- **E3 仍是发布门禁**：规则在、bundle 不在时，正确密码还能不能进
- E4（重启后登录窗口不受影响）、E5、E6、E7 见交互设计文档
