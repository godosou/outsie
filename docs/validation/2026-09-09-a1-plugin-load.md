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

**1. ~~stock 规则下插件链根本不参与~~ —— 这条结论不成立，已撤回。**

原本写的是：在 `[use-login-window-ui]` 形态下 loginwindow 自己处理解锁，插件链不被调用，
依据是 Apple 在该规则 comment 里的那句「set rule to `authenticate-session-owner-or-admin`
to enable SecurityAgent」，以及三轮毫无动静的实验。

**但那三轮实验全部发生在输入没有送达 VM 的时间段内**（客户机 HID 空闲时间单调增长到
369 秒，见本文件末尾的仪器故障记录）。它们没有提交过任何解锁尝试，因此对任何规则形态
都不构成证据。唯一在输入正常时验证过的配置是
`[ai.repose.spike, authenticate-session-owner-or-admin]` + 非特权机制。

一条相反方向的旁证：这台开发机上 OpenAI 的
`com.openai.sky.CUAService.AuthorizationPlugin.remote` 此刻就与 `use-login-window-ui`
并列在同一条规则里，`k-of-n=1`。

**所以「安装器实际生成的形态能不能加载插件」目前是未知的**，而这正是安装器会生成的形态
（`authdb-edit add-subrule` 是前置插入并保留原有条目）。**被验证的形态安装器不会生成，
安装器生成的形态没被验证过。** 这是 A3 的前置实验，见交互设计文档的 E1。

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

## 里程碑 B：条件门控 — 也通过

同一配置下换成 `ReposeSpike:permit` 机制，两个方向各测一次，**两次都输入错误密码**：

| permit 文件 | 插件判定 | 结果 |
|---|---|---|
| 不存在 | `absent after 1600ms, giving up` → `Deny` | **保持锁定**（回落到真实密码校验，错误密码被拒） |
| 存在 | `present after 0ms` → `Allow` | **解锁**（尽管密码是错的） |

```
2026-09-09T11:43:02.838 pid=7893 uid=92 permit: /tmp/repose-permit present after 0ms
2026-09-09T11:43:02.838 pid=7893 uid=92 MechanismInvoke result=Allow
```

从 `MechanismCreate` 到 `MechanismDestroy` 全程 3 毫秒。

**结论：这条技术路线成立。** 机制既能按条件放行，也能在条件不满足时干净地把控制权交还给
密码路径。剩下的是工程量，不再是可行性问题。

## 调用时机：两个实验定死了产品形态

**实验一：密码框出现时，机制不运行。** 锁屏后只按一下键让密码框出现、不提交，
等待 15 秒，插件日志为空。所以「唤醒后自动解锁、完全不碰键盘」在这条路线上做不到 ——
机制没有在后台运行的机会，也就无从用 `RequestInterrupt` 去打断密码界面。

**实验二：空密码回车即可触发，并解锁。** permit 在场时，清空输入框直接回车：

```
2026-09-09T11:47:45.406 permit: /tmp/repose-permit present after 0ms
2026-09-09T11:47:45.407 MechanismInvoke result=Allow
```

**因此可交付的产品形态是：**

> 唤醒 Mac → 密码框出现 → 按一下回车 → 进去

省掉的是输密码，不是那一次按键。文案必须这么写。这与 Apple Watch 解锁 Mac 的
「唤醒后自动进入」仍有一步之差，原因是那套机制在系统内部实现，不经由 Authorization Plugin。

## 安全语义：四个格子全部实测

最该问的问题是「手机不在旁边时，空密码能不能进去」。四种组合逐一测过：

| 手机在场（permit） | 输入的密码 | 结果 | 机制判定 |
|---|---|---|---|
| 在 | 错误 | **解锁** | `present after 0ms` → Allow |
| 在 | 空 | **解锁** | `present after 0ms` → Allow |
| 不在 | 错误 | 保持锁定 | `absent after 1600ms` → Deny |
| 不在 | **空** | **保持锁定** | `absent after 1600ms` → Deny |

最后一行是关键，日志证明机制**主动拒绝**而不是碰巧没被调用：

```
2026-09-09T11:53:23.555 permit: /tmp/repose-permit absent after 1600ms, giving up
2026-09-09T11:53:23.555 MechanismInvoke result=Deny
```

语义正确：**准入由手机在场决定；手机不在时密码路径完整生效，空密码进不去。**

测这一格时有一个过程教训值得记：第一次测「不在 + 空」得到的是「保持锁定但日志为空」，
看起来通过了，实际上是键盘残留了一个字符、提交的根本不是空密码，机制压根没运行。
截图确认输入框显示占位符之后重测，才拿到上面那条 Deny 记录。
**「结果符合预期」和「因为正确的原因符合预期」是两回事。**

## 尚未回答

- `,privileged` 变体为何没有动静（非特权版可用，特权版未成功，原因未查）
- permit 判定目前是「文件存在即放行」，且 `/tmp` 全局可写。产品化前必须改成
  「读一个带时效的签名 token 并在进程内验签」，路径也要移到只有 root 能写的目录

## 自动化能力（本轮新增）

宿主机已获得辅助功能与屏幕录制权限，现在可以：
- 用 `osascript` 向 VM 注入按键（实测有效，客户机 HID 空闲时间归零）
- 用 `screencapture` 看到 VM 画面，闭环确认界面状态而不是盲操作

这两项让后续实验不再需要人工按键。此前有三轮实验因为输入没进 VM 而完全空转。

