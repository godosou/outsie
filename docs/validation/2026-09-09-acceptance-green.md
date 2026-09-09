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

## 下一步换掉什么

存在源从「ssh 写文件」换成真实 BLE，其余不动 —— 这正是当初把
`REPOSE_LEAVE_CMD` / `REPOSE_RETURN_CMD` 做成注入点的目的。
接口已经被这次运行验证过了。
