# 认证在场（`repose-presence-v1`）：已实现，**尚未在真机上验证**

日期：2026-09-10

状态：**代码已落地并接入，但决定性的那个测试没跑成。**

先把结论放在最前面，因为这份文档最容易被误读成「E13 修好了」：

> **本次没有任何一次真实无线电证据。** 手机在实现过程中从 adb 掉线，
> `tests/e2e/impersonation_test.sh` 一次都没有跑过。按本项目自己定的标准 ——
> 「修复落地而这个测试没有翻绿，就只是断言，不是证明」—— 这次交付的是断言。

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

## **没有**证明的（这一节比上一节重要）

1. **真机上冒充者会被拒。** 没跑过。`impersonation_test.sh` 已按两条腿重写
   （真机必须被接受 **且** 冒充者必须被拒绝），但一次都没执行。
2. **手机和 Mac 对同一段 pre-image 的理解一致。** 两端各自与 OpenSSL 对得上，
   仍可能彼此对不上 —— 比如计数器端序理解不同。这种错误的症状是
   **手机永远解不开锁**，看起来和信号差一模一样，所以只有真机能排除。
3. **不可连接广播在这台 realme 上能起来。** `setConnectable(false)` 是新的。
4. **每 30 秒停止再启动广播**在 OEM 限流下的行为。
5. Doze、耗电、延迟 —— B4 的内容，本来就排在最后。

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
