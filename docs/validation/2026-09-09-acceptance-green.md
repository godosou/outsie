# 验收测试首次通过（真实 macOS + 真实插件 + 模拟手机）

日期：2026-09-09 12:0x

```
[1/3] locked after 563ms
[2/3] stayed locked through a wake attempt while the phone was away
[3/3] unlocked 351ms after the phone returned
PASS  end-to-end unlock latency: 6035 ms (budget 8000 ms)
```

## 这证明了什么

完整链路第一次跑通：锁屏 → 手机离开 → 尝试解锁 → **保持锁定** → 手机返回 →
尝试解锁 → **解锁**。全部发生在真实 macOS 上，经由真实的 Authorization Plugin 和
真实的授权求值。唯一模拟的是「手机在不在」，用 ssh 写一个 permit 文件代替。

第 2 步不是空断言：它包含一次完整的解锁尝试，机制运行并 Deny，密码路径接管并拒绝。

## 这没有证明什么

**6035ms 这个数字不是产品延迟。** 它里面绝大部分是测试脚本自己的按键动作 ——
`vm-wake-submit.sh` 有约 3.5 秒的刻意延时和 24 次退格。真正的解锁判定是 **351ms**
（从 permit 出现到 `IOConsoleLocked` 翻转）。产品延迟要等接上真实 BLE 之后才有意义，
而那里的瓶颈已知是 macOS 的扫描窗口（p95 5.4 秒），不是这一段。

## 配置

| 项 | 值 |
|---|---|
| 客户机 | macOS 14.6.1 (23G93)，SIP enabled |
| 插件 | ad-hoc 签名，`ReposeSpike:permit`，非特权 |
| 规则 | `[ai.repose.spike, authenticate-session-owner-or-admin]`，k-of-n=1 |
| 唤醒动作 | 宿主机经 osascript 注入按键：关屏保 → 清空输入框 → 回车 |
| 存在源 | ssh `touch` / `rm` `/tmp/repose-permit` |

## 复现性：只绿过一次，原因是宿主机锁屏

首次通过之后立刻连跑三次，**三次全部失败**。诊断结果不是解锁机制的问题：

```
客户机输入空闲: 1230 秒     ← 20 分钟内没有任何输入到达 VM
插件日志:       (空)        ← 机制从未被调用
```

三次运行里 `vm-wake-submit.sh` 的按键**一个都没送进 VM**。

### 一次被自己抓住的错误归因

我最初把原因判为「`set frontmost` 对 tart 窗口静默无效、System Events 枚举不到该窗口」，
并据此写下「无人值守的 GUI 回归做不到」这个一般性结论。**那是错的。**

真实原因简单得多：**宿主机自己锁着屏**（`IOConsoleLocked = true`，宿主 HID 空闲 1382 秒）。
锁屏状态下没有可注入的图形会话，于是激活失败、窗口枚举为 0、点击不聚焦 —— 这些都是
同一个前提的后果，不是 macOS 的永久限制。

发现方式：截图全黑。此前几轮诊断都在盲测，没有先确认「屏幕上到底有没有东西」。

**所以「能否无人值守做 GUI 回归」这个问题目前仍是未知**，不能按上面那个错误结论排除。
唯一确定的是：宿主机锁屏时做不到。

### 但 harness 的缺陷是真的，且已修

无论根因是什么，它都把「输入没送达」报成了 `Mac did NOT unlock` —— 一句关于功能的断言，
而且是错的。真实发生的事情是「没有人敲门」。

已修：`vm-wake-submit.sh` 现在在动作前后各读一次客户机的 HID 空闲时间，没有下降就明确报
「输入没到达目标」并退出非零，绝不让投递失败被记成产品失败。同时在动作前校验前台窗口
确实是 VM，不是就直接拒绝。

## 下一步换掉什么

存在源从「ssh 写文件」换成真实 BLE，其余不动 —— 这正是当初把
`REPOSE_LEAVE_CMD` / `REPOSE_RETURN_CMD` 做成注入点的目的。
接口已经被这次运行验证过了。
