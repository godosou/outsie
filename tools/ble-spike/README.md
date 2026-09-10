# BLE spike

一次性实验，回答一个问题：**手机熄屏、拔掉充电、放在口袋里之后，蓝牙广播还连续吗？**

这不是产品代码。没有加密、没有状态机、没有配对、没有距离判定。只有：手机持续广播一个
固定 UUID，Mac 持续记录收到的信号强度。

## 为什么需要它

「走近电脑自动解锁」的前提是 Mac 能持续感知手机在不在。Mac 唯一的依据就是手机的 BLE
广播。而这个功能被用到的时刻，恰好是手机熄屏、在口袋里、靠电池 —— Android 省电机制最
积极的状态。

同类产品（车钥匙类应用）证明这条路走得通，做法是把应用放进不受限的耗电白名单。所以这里
要测的**不是「能不能活」，而是「活得有多稳」**：广播有没有间断、间断多长、RSSI 波动多大。
「三秒内解锁」这个目标要的是连续性和延迟，不只是存活。

`codex/phone-proximity-unlock` 分支写了 5 万行代码，全部建立在这个假设上，但从未测量过。

## 两端契约

> **2026-09-10 起，下面这张旧表已作废。** 那两个常量公开印在本仓库里，所以它们
> 谁也认不出来 —— 任何设备广播同一个 UUID 都会被当成「你的手机」（[E13](../../docs/validation/2026-09-09-e13-no-device-identity.md)）。
> GATT 服务端已删除，connect→read 那条路径已从判定中移除。

| | 值 |
|---|---|
| Service UUID | `0000FFF0-…`（16-bit **FFF0**） |
| Service Data | `version(1) ‖ keyId(1) ‖ HMAC-SHA256(K, msg)[0..8)` |
| pre-image | `"repose-presence-v1 beacon" ‖ keyId(1) ‖ counter(8, 大端)` |
| counter | `floor(unix_seconds / 30)`，**不上天线**，两端各自从时钟算 |
| 可连接性 | 否（`setConnectable(false)`） |

改任何一端都必须同步改另一端，而且这次改错**不会有任何报错**：pre-image 不一致的
症状是标签永远验不过，看起来和手机不在范围内完全一样。所以两端各自与 OpenSSL 对答案
（`presence-verify --self-test`、`presence-vectors.sh`），彼此之间靠
`tests/e2e/impersonation_test.sh` 在真机上对。

旧的 128-bit UUID 与 `repose-hello` 只保留在 `SpikeContract` 里作历史记录，不参与判定。

## 管线

```
rssi-scan  |  sudo presence-verify  |  permit-bridge.sh
只测量         只认证                   只判远近
无密钥         无无线电                 两者都无
```

拿着密钥的进程不碰无线电，碰无线电的进程没有密钥可泄。

下发一把开发密钥（**不是配对**，没有防中间人，见脚本头部说明）：

```bash
tools/ble-spike/provision-dev-key.sh          # 两端各自显示同一个指纹
tools/ble-spike/provision-dev-key.sh --show
tools/ble-spike/provision-dev-key.sh --revoke
```

## 手机端

```bash
cd android
./gradlew assembleDebug
adb install -r -t app/build/outputs/apk/debug/app-debug.apk
```

打开应用 → 授予蓝牙权限 → 点 START ADVERTISING → 界面应显示
`advertising: yes` / `gatt server: open`。

**如果停在 `advertising: no`**：这台手机的芯片/系统不支持 BLE 外围模式，整个「手机当外围」
的方案不成立，必须翻转角色（Mac 当外围、手机当中心）。这是最先要确认的分叉点。

### realme / ColorOS 必做设置

不做这一步基本必被系统冻结：

1. 设置 → 应用管理 → 找到 BLE spike → **耗电管理** → 允许后台运行、关闭「智能省电」
2. 设置 → 电池 → 更多设置 → 关闭针对该应用的省电优化
3. 把应用锁在最近任务列表里（下拉卡片点锁图标）

权限被拒两次后 Android 会永久拒绝，之后点 START ADVERTISING 会**静默无反应**。
遇到这种情况去应用信息里手动开权限。

## Mac 端

```bash
cd mac
./rssi-log.sh 300        # 5 分钟
./rssi-log.sh 28800      # 8 小时（默认）
./analyze-rssi.py logs/rssi-<时间戳>.csv
```

脚本会在源码更新时自动重编，并用 `caffeinate -i` 阻止 Mac 空闲休眠 —— 否则 Mac 一睡，
CSV 里会出现几小时的空白，看起来和「手机不广播了」一模一样。

首次运行会弹 macOS 蓝牙权限，授权的是**父终端应用**（Terminal / iTerm），不是这个二进制。
每换一个终端都要重新授权一次；一旦点过拒绝就不再弹，程序直接 `exit(2)`。

CSV 格式：`unix_ms,rssi,peripheral_id_prefix,version,key_id,tag_hex`，无表头。
经 `presence-verify` 后追加一列 `auth`（`VALID` / `INVALID` / `NOKEY` / `MALFORMED`）。
没有载荷的广播也照样输出（后三列为 `-`）—— 「看见了但用不了」是信息，不是该丢掉的行。

## 分级验证，不许跳级

| 级别 | 内容 | 通过判据 |
|---|---|---|
| **B-1** | 桌面明屏 5 分钟，相距 1 米 | CSV 持续出行且 `auth=VALID`；`gaps > 10s` 为 0 |
| **B-2** | 锁屏 10 分钟，**不插 USB** | 区分「一锁屏就停」和「进深度 Doze 才停」 |
| **B-3** | 整夜 8 小时 | 只有 B-1、B-2 都干净才开始 |

旧版这里有一条「`peripherals` 只有一个 id」的判据，现在**必须删掉**：地址每几分钟自己
轮换一次，而且每 30 秒重算标签时会停止再启动广播，也会换一个地址。所以一次整夜观测里
出现几百个 id 是正常的。身份是密钥，不是地址 —— 把地址当身份正是 E13 的成因之一。

同样作废的是「读完之后 CSV 仍在出行」：那条判据针对的是「建立连接后广播自动停止」这个
坑，而现在的信标不可连接，没有连接可建。

## 观测期间的硬性要求

- **必须拔掉 USB。** 充电状态下 Android 根本不进 Doze，插着测等于没测。
- 因此观测期间**不抓 logcat**（抓 logcat 要插线）。只看 Mac 侧 CSV。
- Mac 不关机、不合盖。

## 解读数据时的注意事项

- **TX power 是 MEDIUM 不是 HIGH**（`BleSpikeService.kt` 的 `startAdvertising`）。
  这批 RSSI 的绝对值绑定这个功率档。将来产品代码必须锁同一档，否则据此标定的距离阈值
  全部作废。
- **心跳缺失 ≠ 广播中断。** 应用的 heartbeat 用 `uptimeMillis`，深度睡眠时不推进；而广播
  由蓝牙控制器 offload，CPU 挂起时照常发。别用心跳判断存活，只看 Mac 侧收到了什么。
- **分析器出现 `WARNING: more than one peripheral`** 时，聚合的 gap 数字不可信 —— Android
  每约 15 分钟轮换一次 BLE 地址，多个 id 交织会互相填补空档，把真实中断掩盖掉。以
  per-peripheral 那段为准。
- **出现 `samples arrived out of order`** 说明运行期间系统校过时，所有时间统计不可信，
  该次数据作废重跑。
