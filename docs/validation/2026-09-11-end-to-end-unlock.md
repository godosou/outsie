# 第一次真正解锁一块屏幕

日期：2026-09-11 07:43–07:48

**真手机 → 真 BLE → HMAC 认证 → permit → macOS 授权插件 → 屏幕打开。**
这条链路此前从未完整跑通过一次；`codex/phone-proximity-unlock` 分支写了 5 万行，
也没有发生过一次。

## 四个观测，双向都要

| # | 条件 | 动作 | 插件判定 | 屏幕 |
|---|---|---|---|---|
| 1 | 手机在旁边 | 锁屏 → **空密码**回车 | `result=Allow` | **打开** |
| 2 | 手机离开（信标停止） | 等 46 秒 → 空密码回车 | `result=Deny` | **保持锁定** |
| 3 | 手机不在 | 输**真密码** | `result=Deny` | **打开** |
| 4 | 手机回来 | 锁屏 → 空密码回车 | `result=Allow` | **打开** |

第 3 行和第 1 行一样重要：我们的机制拒绝时，`k-of-n=1` 让密码那条路照常生效。
**手机没电、蓝牙关了、插件坏了，都不会把人锁在门外。**

第 2 行是第 1 行的意义所在 —— 没有它，「能解锁」只等于「总是能解锁」。

插件日志（VM 内 `/tmp/repose-plugin.log`）：

```
07:44:10.947  AuthorizationPluginCreate hostVersion=4
07:44:10.947  MechanismCreate id=permit mode=permit
07:44:10.948  permit: /var/run/repose-spike/permit present, root-owned and fresh after 0ms
07:44:10.948  MechanismInvoke result=Allow          ← 手机在

07:46:14.966  permit: /var/run/repose-spike/permit absent after 1600ms, giving up
07:46:14.966  MechanismInvoke result=Deny           ← 手机走了
```

## 管线长什么样

```
rssi-scan          |  presence-verify        |  permit-bridge.sh
你的 uid，有蓝牙       root，有密钥               你的 uid，两者都没有
                                              ↓ ssh
                                            VM: /var/run/repose-spike/permit
                                              ↓
                                            ReposeSpike.bundle（授权插件）
```

三段privilege 不同，所以写不成一条管道：扫描器**必须**非特权（蓝牙 TCC 绑父应用，
以 root 跑会报 `STATE unauthorized`），验证器**必须** root（密钥是 root:wheel 0600，
谁能读 K 谁就能伪造在场），而桥**必须**留在用户身份下（它用你的 ssh 密钥连 VM）。
`presence-pipeline.sh` 用两个普通文件把三段接起来。

FIFO 是第一版，是个陷阱：打开一端会阻塞到另一端打开，于是三段的启动顺序变成隐式依赖，
而授权对话框正好卡在中间；再加上 `do shell script` 返回时会回收进程组，
用 nohup detach 的验证器时有时无。普通文件没有顺序可搞错。

## 这次花掉最多时间的错误，和它的解药

**有几轮我一直在测错的屏幕。**

VM 刚开机时停在**开机登录窗**，它走的是 `system.login.console`；
而我们的机制装在 `system.login.screensaver` 上。所以插件根本不会被问到 ——
日志文件不存在、屏幕不开、`authorizationhost` 一次没起。

每一个症状读起来都像「macOS 拒绝加载这个插件」。我查了规则、查了 `k-of-n`、
查了 codesign、还 `killall` 了 SecurityAgent。**全都没问题，是我敲错了门。**

两块屏幕长得几乎一样，这正是它昂贵的原因。区分它们只需要一行：
没人登录时 `/dev/console` 属于 root。

现在 `repose_vm_check` 有一行 `gui session`，没人登录就明说
「那是开机登录窗，不走屏保那个 right，机制在那里永远不会运行」。

## 这次**没有**覆盖的（不要读成更多）

- **不是你的日常 Mac。** BLE 和手机是真的，插件和屏幕在 VM 里。
  是否搬到日常机是交付之后的独立决定。
- **没有验证距离判定。** 手机全程没动过；「离开」是用停止广播模拟的。
  RSSI 阈值仍然是未标定的占位值（B3）。
- **没有验证 Doze / 续航 / 8 小时。** B4，本来就排在最后。
- **配对仍然不安全。** `K` 走 USB 下发，防不了中间人（issue 0002 未关）。
- **App 界面还没接这条链路。** 现在跑的是 shell 脚本，不是 Mac App。
  「只用界面完成安装 → 解锁 → 卸载」这条判据仍未满足。

## 相关

- [认证在场验收](2026-09-10-authenticated-presence.md)
- [扫描节奏与超时](2026-09-10-scan-cadence.md)
- [E13](2026-09-09-e13-no-device-identity.md)
