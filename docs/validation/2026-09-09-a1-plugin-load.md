# A1：macOS 会加载并听从 ad-hoc 签名的第三方 Authorization Plugin

日期：2026-09-09 11:29

结论：**通过。** 这是本项目最大的未知，现在有了肯定的答案。

## 环境

| 项 | 值 |
|---|---|
| 客户机 | Tart VM，macOS 14.6.1 (23G93)，与宿主同 build |
| SIP | **enabled** |
| 签名 | **ad-hoc**（`codesign -s -`），无 Developer ID，无公证 |
| 机制 | `ReposeSpike:log`，**不带 `,privileged`** |
| 规则 | `system.login.screensaver` = `[ai.repose.spike, authenticate-session-owner-or-admin]`，`k-of-n=1` |

## 证据

用户在锁屏界面输入了一个**错误的密码**并回车，屏幕**解锁了**。插件日志：

```
2026-09-09T11:29:39.348 pid=6051 uid=92 AuthorizationPluginCreate hostVersion=4
2026-09-09T11:29:39.348 pid=6051 uid=92 MechanismCreate id=log mode=log
2026-09-09T11:29:39.350 pid=6051 uid=92 MechanismInvoke enter mode=log
2026-09-09T11:29:39.350 pid=6051 uid=92 MechanismInvoke result=Allow
2026-09-09T11:29:39.351 pid=6051 uid=92 MechanismDestroy
```

`uid=92` 是 `_securityagent`。全链路耗时 3 毫秒。

## 三个必须记住的发现

**1. stock 规则下插件链根本不参与。** 全新 macOS 的 `system.login.screensaver` 是
`[use-login-window-ui]`，且**没有 `k-of-n` 键**。Apple 自己在该规则的 comment 里写着：
「set rule to `authenticate-session-owner-or-admin` to enable SecurityAgent」。
在 `use-login-window-ui` 形态下，loginwindow 自己处理解锁，SecurityAgent 与插件链不被调用。
先前三轮实验毫无动静，部分原因就在这里。

**2. 授权求值发生在「提交解锁尝试」时，不是唤醒时。** 按键唤醒画面后
`SecurityAgent` 并未启动；只有输入密码并回车才触发求值。这对产品是硬约束：
「走近就自动解锁」需要机制在后台等待并主动打断密码界面（`RequestInterrupt`），
而不是被动等待被调用。B 里程碑要验证这一点。

**3. 引擎没有发 `MechanismDeactivate`。** 日志里 `MechanismInvoke` 之后直接是
`MechanismDestroy`。这印证了先前查证的契约：`DidDeactivate` 是对 Deactivate 请求的应答，
在 `MechanismInvoke` 里主动调用它等于应答一个从未发出的请求。原实现是错的，已修正。

## 过程中被证伪的三个「结论」

这次实验有三轮结果是仪器故障而非事实，记录下来以免重演：

| 假象 | 真相 |
|---|---|
| 「插件 NOT LOADED」 | `log show` 在该 VM 里对任何查询返回 0 行，证据通道本身是坏的 |
| 「按键了但无反应」 | HID 空闲时间单调增长到 369 秒，**输入根本没进 VM**，三次实验全是空转 |
| 「日志 0 行」 | 文件属主是 `_securityagent`、权限 0600，`admin` 读不了。**它一直有内容** |

最后一条尤其值得记住：那次「0 行」藏住的是一个成功。

## 尚未回答

- **里程碑 B**：机制能否按条件放行/拒绝（`permit` 模式 + 验收测试）
- `,privileged` 变体为何没有动静（非特权版可用，特权版未成功，原因未查）
- 产品形态是否必须用 `RequestInterrupt` 才能做到「唤醒即免密」
