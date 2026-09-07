# 手机靠近解锁使用与恢复说明

日期：2026-09-08

状态：**工程原型 / GATE CLOSED**

## 先看当前状态

仓库已经实现并测试了距离校准、离开—返回状态机、密码学线协议、一次性许可、macOS
Authorization 原型、Flutter 管理界面以及 Android 原生能力的主要离线边界。但生产链路故意
保持关闭：没有修改这台 Mac 的 Authorization Services，没有把插件加载进
`authorizationhost`，Android BLE 角色没有根据 GT5 Pro 真机证据启用，iOS 后台实现也尚未在
受支持的 Xcode/iPhone 环境中完成。

因此，本页后面的“正常流程”描述的是门禁全部通过后的产品行为，不表示当前构建已经可以
自动解锁。

## 支持范围

目标能力只处理以下场景：

- Mac 已经有一个登录中的本地用户会话；
- 该会话因为锁屏或睡眠唤醒进入系统密码界面；
- 已配对手机在本次锁屏后先稳定离开，再稳定返回；
- 距离状态、会话绑定、短时挑战、签名和防重放计数器全部通过验证。

以下场景始终不自动解锁：

- 重启或关机后的首次登录；
- FileVault 开机解密；
- 注销、Guest、快速用户切换或远程登录；
- 没有经历本次锁屏后的“离开再返回”；
- 手机、蓝牙、后台服务、密钥、持久化状态或系统插件任一不可用。

Repose 不保存、读取或模拟输入 macOS 密码。系统密码必须始终作为独立 fallback 保留。

## 门禁通过后的正常流程

### 安装与配对

1. 管理员先在专用、可恢复的测试 Mac 上验证签名、公证、密码 fallback、修复和卸载。
2. Mac 在已解锁的本地会话中生成一次性、短时有效的配对二维码。
3. 用户在手机上扫描并确认 Mac；手机在 Android Keystore 或 Apple
   Secure Enclave/Keychain 中生成不可导出的 P-256 身份密钥。
4. 双方交换长期公钥与设备标识。配对秘密用后销毁，私钥不离开平台密钥存储。

当前设置页中的安装、配对和校准入口会在后端或验证证据不满足时明确显示关闭状态，不会
绕过门禁执行系统修改。

### 距离校准

每一对 Mac 和手机都需要单独校准：

1. 把手机放在日常携带位置，在期望的靠近边界停留并采集近距离 RSSI 样本。
2. 携带手机走远，再采集远距离样本。
3. 稳健统计计算 `nearThreshold` 与 `farThreshold`，中间保留迟滞区，防止边界抖动。
4. 如果两组分布重叠过多、样本不足或数据异常，校准失败并要求重做。

RSSI 会受到人体遮挡、口袋/背包、手机方向、墙体、射频拥塞和设备功率控制影响。校准得到
的是概率阈值，不是米制测距，也不是密码学距离证明；攻击者仍可能中继合法手机的无线交互。
后台自适应只能收紧范围，放宽范围必须由用户重新校准。高风险环境应关闭自动解锁并使用
系统密码。

### 日常解锁

Mac 锁屏后先进入未武装状态。只有稳定远离或可靠断连才进入 `ARMED`；之后稳定重新靠近才
会发起一次短时挑战。手机验证已配对 Mac 的签名后生成带整体签名、并包含 AEAD 加密证明的
响应；Mac 再验证手机签名、会话、锁屏代次和单调计数器。全部成功时，只生成绑定当前会话、
约三秒有效且只能消费一次的内存许可。

手机一直放在桌边不会因为锁屏而立即触发自动解锁。任何失败都只应让本次自动尝试失败，
用户仍可直接输入系统密码。

## Android 行为与 realme 注意事项

Android UI 使用 Flutter，后台 companion presence、BLE 和 Keystore 由原生 Kotlin 层负责，
不依赖 Dart isolate 常驻。目标首机是真我 GT5 Pro、realme UI 7.0、Android 16/API 36。

当前尚未完成该设备的连接与授权实测，因此以下内容仍需真机确认：熄屏、应用 UI 关闭、
Doze、系统回收、蓝牙切换、重启、口袋/背包校准、30 次离开/返回循环、延迟、耗电和误判。
用户对应用执行“强行停止”后，Android 不应再被认为能够自动响应；在用户重新打开应用并恢复
系统允许的运行状态前，Mac 必须退回密码解锁。OEM 电池策略可能进一步限制后台行为，不能
依靠关闭系统安全机制来掩盖这种限制。

同一签名 APK、同一 Android UID 内的原生依赖属于手机端可信计算基。当前 Java/Pigeon 边界
阻止普通跨包调用构造签名能力，但不声称能隔离已经进入同一 UID 的恶意原生代码；更强隔离
需要单独 UID/进程的受保护服务。

## iOS 行为

iOS 目标设计使用 Flutter 共享界面、AccessorySetupKit 做前台用户授权配对、Core Bluetooth
state restoration 处理后台传输，并把身份密钥保存在 Secure Enclave/Keychain。用户强制退出
应用、蓝牙关闭、重启后首次解锁前或系统拒绝后台恢复时，自动响应必须不可用并退回密码。

当前选中的 Apple 开发工具链是 macOS Command Line Tools，无法定位 `iphoneos`/iOS 26 SDK；
是否在其他位置另装 Xcode 未作为门禁证据。因此没有提交无法编译验证的生产 Swift 实现，
iOS Release/Archive 门禁继续关闭。

## 撤销、丢失手机与重新配对

- 手机丢失、被盗、重装应用、身份密钥损坏或计数器状态无法恢复时，在已解锁 Mac 上撤销该
  精确设备，并使用系统密码；不要等待“距离变远”自动解决。
- 撤销会使该配对代次及更旧代次失效。重新配对必须创建严格更新的代次，不能静默沿用旧
  公钥或旧计数器。
- 已解锁手机被盗并带回 Mac 附近仍是残余风险；撤销和正常设备锁定是必要的处置手段。

## 故障、修复与卸载

任何组件健康检查失败时，自动解锁应保持关闭，系统密码界面继续工作。不要通过删除系统
文件、直接编辑 authorization 数据库或恢复过期 plist 来“修复”。设计中的管理工具只允许
固定范围的只读计划或事务化操作：

```text
repose-unlockctl status
repose-unlockctl plan-install --artifacts <absolute-dir>
repose-unlockctl plan-uninstall
repose-unlockctl install --artifacts <absolute-dir> --apply
repose-unlockctl uninstall --apply
repose-unlockctl repair --apply --backup <absolute-path>
```

后三个命令目前在接触文件系统、launchd 或 Authorization Services 前硬关闭；它们不是当前
可用的恢复手段。未来只有签名、公证、专用测试 Mac、密码 fallback、断电恢复和卸载矩阵全部
通过后才可开放。正常卸载必须先精确移除 Repose 自己的授权候选并确认密码 fallback，绝不以
旧备份覆盖其他软件后来写入的机制。

## 日志与隐私

诊断可以记录组件状态、阶段、错误分类、计数和脱敏时序，但不得记录：

- macOS 密码、配对秘密或任何私钥；
- 会话密钥、ECDH 原始秘密、AEAD 密钥或派生 nonce；
- 完整挑战、完整响应、完整随机数或可重放签名；
- FileVault 恢复密钥或其他恢复凭据。

距离样本和解锁事件按设计保留在本机/手机，不依赖云端作为授权条件。

当前原型没有生产日志 sink、日志文件、保留周期或导出功能。设置页的
`open_unlock_diagnostics` 仅返回门禁关闭的脱敏状态；未来任何日志位置、权限、轮换、保留和
导出方案都必须先通过安全评审。

## 相关证据

- [安全威胁模型](security/phone-unlock-threat-model.md)
- [验证状态与发布门禁](validation/verification-summary.md)
- [macOS Authorization 实机门禁](validation/macos-authorization-results.md)
- [Android GT5 Pro 结果](validation/android-gt5-pro-results.md)
- [iOS 后台结果](validation/ios-background-results.md)
- [协议 v1](protocol/repose-unlock-v1.md)
