# 认证在场（`repose-presence-v1`）：**已验收**

日期：2026-09-10 实现 · **2026-09-11 真机验收通过**

## 验收结果

```
tests/e2e/impersonation_test.sh 25
  genuine : VALID=16  INVALID=0  NOKEY=0
  imposter: VALID=0   INVALID=24
  ok  the paired phone is accepted (16 verified beacons)
  ok  an unprovisioned device is refused (24 beacons, none verified)
  ok  the Mac can tell the two apart
  3 passed, 0 failed
```

走的是产品链路本身（`rssi-scan | presence-verify`，验证器以 root 读
`/var/db/repose-unlock/presence-key.1`），不是手工比对。

**冒充者不是没上天线**：24 个样本、格式完全正确、信号就在旁边，一个都验不过。
这正是 E13 那节「验收判据」要求的失败方式。

## 边界：这一条通过了，别的没有

- **没有验证完整解锁。** 证的是「Mac 能不能分辨这两台设备」。
  从 permit 写入 → 插件 → 屏幕真的开了，这一段还没有用真实 BLE 跑通。
- **没有验证配对安全。** `K` 仍然走 USB 下发，防不了中间人。
- **没有验证续航/Doze。** B4，本来就排在最后。

## 改了什么

在场判定从「看见某个公开 UUID」变成「看见一个只有配对密钥才能算出的标签」。

```
counter = floor(unix_seconds / 30)                    ← 不上天线，两端各自从时钟算
msg     = "repose-presence-v1 beacon" ‖ keyId(1) ‖ counter(8, 大端)
tag     = HMAC-SHA256(K, msg)[0..8)
广播     = 16-bit UUID FFF0 + service data: version(1) ‖ keyId(1) ‖ tag(8)
```

管线拆成三个进程，每个只能做一件事：

```
rssi-scan  |  sudo presence-verify  |  permit-bridge.sh
只测量         只认证                   只判远近
无密钥         无无线电                 两者都无
```

这个拆分不是洁癖：拿着 256 位密钥的进程不碰无线电，碰无线电的进程没有密钥可泄。

| 文件 | 变化 |
|---|---|
| `SpikeContract.kt` | 新增 `PRESENCE_*`（此前 0 处） |
| `PresenceKey.kt` | 新增。K 以不可导出形式存 AndroidKeyStore；签名在密钥库内完成 |
| `PresenceBeacon.kt` | 新增。每 30 秒重算标签并重启广播 |
| `BleSpikeService.kt` | 广播改为**不可连接**；GATT 服务端删除 |
| `PairingScreen.kt` | 改为显示真实密钥指纹，或「没有密钥」 |
| `rssi-scan.swift` | 删掉 connect→discover→read 整条路径 |
| `presence-verify.swift` | 新增。±1 窗口验证，常数时间比较 |
| `permit-bridge.sh` | 只在 `auth=VALID` **且** RSSI 够近时写 permit |
| `provision-dev-key.sh` | 新增。开发用密钥下发 |

## 已经证明的

| 项 | 怎么证的 |
|---|---|
| Swift 验证器的算法与独立实现一致 | 已知答案向量由 **OpenSSL** 生成，不是本项目代码。用自己的输出校验自己只能证明自洽 |
| 密钥不同 / keyId 不同 / 标签差一比特 → 拒绝 | `presence-verify --self-test`，无需密钥文件 |
| ±1 窗口接受，超出拒绝 | 同上。这条边界**就是**重放防护的全部 |
| 强信号的 INVALID 信标不写 permit | `permit-bridge-test.sh` 第 8 条 —— 冒充者站在 Mac 旁边，信号极好，仍然不放行 |
| NOKEY 是拒绝而不是放行 | 第 9 条。未配置密钥的 Mac 应当谁也解不开，而不是谁都能解 |
| 管线拼错（没接验证器）→ 不产生 permit | 第 10 条。fail closed |
| INVALID 洪水无法把 permit 撑着不过期 | 第 11 条。伪造不了标签的攻击者也不能靠一直发包把门顶住 |
| 认证开关没有关闭方式 | 第 12 条，grep 断言 |
| 两个 flavour 都能构建 | `assembleGenuineDebug assembleImposterDebug` |

`tests/run-all.sh` 全绿。

## 真机实测（RMX3888 + 本机 Mac，2026-09-10 12:04–12:15）

密钥用 `openssl rand -hex 32` 生成后推给手机，手机导入 Keystore 并删掉文件。
Mac 侧**不持有**这把密钥的文件（那一步要 root），所以下面的判定是用 OpenSSL
按同一份 pre-image 手工算出来再比对的。

| 项 | 结果 |
|---|---|
| 不可连接广播能否起来 | **能**。`mAdvertiseConnectable=false` 启动成功 |
| Mac 能否**不建连**读到载荷 | **能**。第一个 `didDiscover` 回调里就拿到了 `ver=1 keyId=1 tag=…` |
| 手机的标签和 OpenSSL 是否一致 | **一致**。`8258efdf4d291151` 落在 `c+0`，`c±1/±2` 全部不同 |
| 窗口是否轮换 | **是**。75 秒 4 个窗口、300 秒 11 个窗口，序列连续 |
| 地址是否随之变化 | 是，300 秒 11 个 peripheral id —— 现在无所谓，身份是密钥 |
| **冒充者**（同一份源码、换包名、无密钥） | 上了天线（21 个样本），标签 `936be9e4179d687d`，**对不上任何窗口** |

最后一行是这次最想看到的东西：冒充者**不是没广播**，它广播了、格式完全正确、
信号也够近，就是算不出正确的标签。这正是 E13 要求的那种失败方式。

## 顺带实测出一个会让人骂街的缺陷（已修，见 [扫描节奏](2026-09-10-scan-cadence.md)）

同一批数据里，把「相邻两次广播之间隔多久」拉出来看（300 秒 342 个样本）：

| p50 | p90 | p95 | p99 | max |
|---|---|---|---|---|
| 0.13s | 2.99s | 3.61s | 6.67s | **8.98s** |

而 `permit-bridge.sh` 原来的 `STALE_S=8`。也就是说，**手机一动不动放在桌上，
平均每 2.5 分钟就会被判定为「已离开」一次**，屏幕跟着要密码。5 分钟里踩中 2 次。

这段沉默不是手机的问题。我一开始把它归因为「CoreBluetooth 全局占空比」，
**后来的对照实验证明那个解释是错的**：同一次不过滤扫描里，230 台其他设备的包
最坏间隔只有 0.57 秒，电台一刻没停。空档是**单个广播者**才有的现象。
不管归因如何，结论不变：**阈值设在这条尾巴底下，只会产生误报。**

改成 **45 秒**。这个数不是调出来的，是两类错误不对称：太长只是让已经走开的人
多算一会儿在场；太短是**坐在工位上的人被反复要求输密码**。

后续三组对照（换地址策略、发射功率、原地换载荷）证明这条尾巴是 macOS 的扫描节奏，
手机侧够不着 —— 细节见 [扫描节奏](2026-09-10-scan-cadence.md)。

45 秒不额外花安全预算：验证通过的信标本来就在当前 ±1 个窗口内有效，
捕获的信标最长可用 2×WINDOW = 60 秒，所以 60 秒以内的取值**不会带来设计尚未承认的暴露**。

也不影响兜底：桥认为在场期间每 5 秒重打一次 permit，所以这些空档不会让 permit 过期；
桥要是死了，没人刷新，permit 在插件的 `PERMIT_FRESHNESS_S`（15 秒）自己失效，与
`STALE_S` 无关。我上一轮写的注释声称 `STALE_S` 必须小于这个 15 秒 —— **那是错的**，
而且正是那个错误的上界把默认值钉在了噪声底下。

`permit-bridge-test.sh` 两条断言分别钉住这两件事，而且两个上界分别从
`presence-verify.swift` 和 `plugin.c` 里读出来。

## **没有**证明的（这一节比上一节重要）

1. ~~`impersonation_test.sh` 本身没跑过。~~ **2026-09-11 已跑通,见开头。**

   值得记下它为什么必须跑：推迟到两条腿都采完再验证，
   导致验证器拿当前时钟去 judge 60~90 秒前的采样，**16 个正确铸造的信标全被判 INVALID**。
   手工用 OpenSSL 比对完全看不出这个问题 —— 因为那段算法本来就是对的，
   错的是产品代码里我刚写的那一行。**「同一段算法我算对了」不能替
   「产品代码在产品路径上算对了」签字。**
2. **permit 有没有真的因此被写/不写。** 验收证的是标签判定，还没有把
   `rssi-scan | presence-verify | permit-bridge.sh` 整条管线接起来跑一遍。
3. **45 秒这个阈值够不够。** 尾巴只采了几分钟；一整个工作日几乎肯定更差。
   上界很清楚（2×WINDOW=60 秒），但真正合适的值要等长时间观测。
4. Doze、耗电、锁屏后的存活 —— B4 的内容，本来就排在最后。

## 为什么冒充测试必须有两条腿

「冒充者被拒绝」这句话，拔掉天线也能满足。所以同一次运行里，同样的二进制、
同样的密钥、相隔几十秒的条件下，真机必须被接受。要测的是**区分能力**，不是拒绝能力，
只有两条腿放在一起才说得出这句话。

测试在下列任一情况下**拒绝给出结论**（退出码 2，不是通过）：真机零样本、
冒充者零样本、冒充者没拿到蓝牙权限、Mac 上没有密钥、验证器自检失败。

## 这次仍然不是「配对」

`K` 通过 `adb push` 下发到 app 私有目录，app 导入 Keystore 后删除文件。
这等于默认「拿着数据线的人就是机主」。设计文档 §1 里那套两端比对 6 位数字、
能防中间人的 SAS 交换**没有实现**。

所以本次关掉的是一个洞：**陌生设备不能再冒充你的手机**。
没关掉的是引导过程本身。脚本叫 `provision-dev-key.sh` 而不是 `pair`，
界面上写的是「开发预览：密钥通过 USB 直接写入」，都是刻意的 ——
一个叫 pair 的脚本干这件事，就会成为本项目第四份「描述了代码没有的保护」的产物。

## 顺带修掉的第四处「不存在的兜底」

设计文档 §4.1 和 §6、issue 0002、原型 README 里都有同一句话：
「真正的解锁仍需不可重放的身份挑战响应」。**这个挑战响应不存在，也不在这条管线里。**
验证通过的信标写下的 permit，就是让空密码回车生效的那个东西。

因此如实改写为：窗口内重放、或实时中继，**买到的就是一次解锁**。
窗口长度和 RSSI 是仅有的两道边界，后面没有第三道。

## 下一步

```bash
sudo -v                                  # 验证器要读 root 专有密钥文件
tools/ble-spike/provision-dev-key.sh     # 两端下发同一把 K，比对指纹
tests/e2e/impersonation_test.sh          # 必须两条腿都过
```

在这条命令给出 `2 passed` 之前，E13 的状态是「已实现」，不是「已修复」。

## 相关

- 设计：[authenticated-presence-design](../plans/2026-09-10-authenticated-presence-design.md)
- 缺陷记录：[E13](2026-09-09-e13-no-device-identity.md)
- 跟踪：[issue 0002](../issues/0002-authenticated-presence.md)
