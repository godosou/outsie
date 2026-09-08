# 手机 UI、权限与 Debug 配对验证

日期：2026-09-08

状态：**AUTOMATED PASS；API 35 EMULATOR UX PASS；GATT E2E NOT OBSERVED；RELEASE GATE CLOSED**

本文是纯验证记录，只区分已经观察到的结果和仍未运行的项目。它不把 Debug 配对、自动化
测试或 APK 编译外推为 macOS 系统自动解锁已经可用。

## 结论

Flutter 手机端已经具备与 Mac 一致的暖白／森林绿色品牌、中英文与系统深色外观、权限和
后台说明，以及以相机扫描 Mac 一次性二维码为唯一用户入口的配对流程。相机权限会先解释、
后请求；普通拒绝留在应用内，永久拒绝提供系统设置入口。手工粘贴配对码入口已移除。

当前 Debug 拓扑是 Mac CoreBluetooth peripheral、Android CDM + GATT central。Mac 开发机的
`bluetoothd` 观察到精确本地名/service UUID 开始广播，并在二维码 120 秒到期后停止。API 35
`emulator-5554` 已安装当前 APK，并走通附近设备授权、CDM association `id=1`、`READY TO
PAIR` 和真实相机扫描页。模拟器无法使用 Mac 主机蓝牙，因而没有 GATT control/status 或
`ACCEPTED` 观察；realme 真机仍为 ADB `unauthorized`，本轮未安装。macOS Authorization、
production presence/unlock role 和手机钥匙 release 路径仍关闭。

## 当前实现边界

| 区域 | 当前实现 | 已有证据 | 没有证据 |
|---|---|---|---|
| 品牌与布局 | Mac 同款色彩、Manrope、四瓣花、手机钥匙式首页；中英文、亮/暗模式 | Flutter widget/controller 回归 | 人工视觉走查截图、完整无障碍设备矩阵 |
| 相机扫码 | `mobile_scanner` 扫描页；无粘贴入口；同步防重复提交 | 聚焦测试；API 35 上说明 → 系统授权 → 真实 scanner 页面 | GT5 Pro 相机权限和真实二维码扫描 |
| 二维码预检 | 只预接受 `repose://pair/v1/<base64url>`；总长 ≤4096；拒绝空白、query、fragment、padding 和额外路径 | Dart 边界测试；Kotlin `PairingUriV1` 负责最终字节解码 | 恶意无线 peer 实测 |
| 权限体验 | 相机预说明、拒绝重试、永久拒绝跳设置；附近设备由用户发起的 CDM/设置流程请求；通知不申请 | Flutter tests；API 35 附近设备说明 → 系统权限 → `Allowed`；静态 manifest 测试 | GT5 Pro 上逐项系统弹窗和 realme 设置返回实测 |
| Mac Debug 配对 | 一次性 QR、CoreBluetooth peripheral、120 秒过期；精确 v1 UUID | `bluetoothd` 广告 start/stop 的有限观察 | Android discovery/connect/control/status |
| Android Debug 配对 | CDM 关联和关联设备 GATT central；session-bound `WAITING`/`ACCEPTED` | Debug 144/144；API 35 安装、association `id=1`、`READY TO PAIR` 与扫码页 | 模拟器/真机 GATT `ACCEPTED`、GT5 Pro 安装 |
| 自动解锁 | fail-closed | UI 和 build gate 明确区分 Debug 配对与自动解锁 | AuthorizationHost、锁屏、离开/靠近、密码 fallback |
| iOS | 共享扫码 UI；相机用途声明和 CocoaPods permission 宏 | plist/Podfile 静态契约，Podfile Ruby 语法 | CocoaPods install、Xcode 编译、模拟器或 iPhone |

## Debug 配对契约

Mac 和 Android 共用 [配对 payload v1](../protocol/repose-pairing-v1.md)：

- URI：`repose://pair/v1/<unpadded-base64url>`；
- Repose service：`A53E0001-7A6B-4D59-9F2E-5245504F5345`；
- control characteristic：`A53E0002-7A6B-4D59-9F2E-5245504F5345`；
- status characteristic：`A53E0003-7A6B-4D59-9F2E-5245504F5345`；
- Mac 广告本地名：`Repose Mac`；
- 一次性会话有效期：120 秒。

Android 先由用户完成 CDM association；只有之后的相机扫描和 Kotlin 严格解码也成功，才尝试
连接该关联设备。Debug central 写入当前 session 的 control 值，只接受同一 session 的
`WAITING` 或 `ACCEPTED`。
这些 Debug 字符串是当前联调协议，不是 `repose-unlock-v1.md` 的 Challenge/Response frame。

## 本轮自动化与构建证据

| 验证项 | 命令 | 结果 |
|---|---|---|
| 依赖解析 | `cd mobile && flutter pub get` | **PASS**, exit 0；`permission_handler 12.0.1`，`permission_handler_android 13.0.1`；较新的 Android 14.1.0 因当前 Gradle Kotlin DSL 不兼容而未采用 |
| Gradle 配置冒烟 | `cd mobile/android && ./gradlew help` | **PASS**, exit 0；降级后不再出现 `kotlin { compilerOptions }` unresolved 配置错误 |
| QR/权限聚焦测试 | `cd mobile && flutter test test/pairing_scanner_test.dart test/app/pairing_scanner_flow_test.dart test/app/camera_permission_manifest_test.dart test/app/localized_workflows_test.dart` | **PASS**, 9/9 |
| Flutter 全量测试 | `cd mobile && flutter test` | **PASS**, 91/91 |
| Flutter 静态分析 | `cd mobile && flutter analyze` | **PASS**；0 issues (`No issues found`) |
| iOS Podfile 语法 | `cd mobile/ios && ruby -c Podfile` | **PASS**；Syntax OK；不是 iOS build |
| Android Debug APK | `cd mobile && flutter build apk --debug` | **PASS**；`mobile/build/app/outputs/flutter-apk/app-debug.apk` |
| Android Debug native JVM | `cd mobile/android && ./gradlew --no-daemon :repose_unlock_native:testDebugUnitTest` | **PASS**, 144/144 |
| Android Release native JVM | `cd mobile/android && ./gradlew --no-daemon :repose_unlock_native:testReleaseUnitTest` | **PASS**, 120/120；使用 fail-closed host API |
| Android Profile native JVM | `cd mobile/android && ./gradlew --no-daemon :repose_unlock_native:testProfileUnitTest` | **PASS**, 120/120；使用 fail-closed host API |
| Web/React | `npm test` | **PASS**, 52/52 |
| Rust full workspace | `cargo test --manifest-path src-tauri/Cargo.toml --workspace --all-targets` | **PASS** |

本轮最后一次 APK 的 SHA-256 为：

```text
118a13cbf5f8fb56f6e927c67d4ec7453b7407e92933ed2e830a321dad374abc
```

构建输出位于 ignored 目录；校验值只对应本轮本地 Debug 工件，不是签名发布清单。

Debug-only pairing tests 只进入 Debug variant；Release/Profile 的 120 项测试继续验证各自不包含
`DebugReposeUnlockHostApi`，并由 fail-closed host API 保持 production gate。API 31–35 的前台
association/GATT 兼容代码也只存在于 Debug source set；production presence runtime 仍要求
API 36，应用 Release build 仍由生产签名门禁拒绝。

## API 35 模拟器观察

`adb -s emulator-5554 install -r -t` 已成功覆盖安装上述 SHA-256 对应的 Debug APK。系统状态
显示该模拟器为 API 35，包 `ai.repose.repose_unlock` 已安装；`dumpsys companiondevice` 显示
association `id=1`，`dumpsys package` 显示 `BLUETOOTH_SCAN`、`BLUETOOTH_CONNECT` 和
`CAMERA` 已授权。

前台逐步观察到：

1. 附近设备页先解释权限用途，再打开 Android 系统权限提示；返回后状态为 `Allowed`。
2. 完成 CDM association 后，首页显示 `READY TO PAIR`。
3. 配对区唯一入口是 `Scan Mac QR code`；没有配对码输入框或复制/粘贴入口。
4. 点击扫码后先显示相机用途说明，再出现 Android 相机权限提示，随后进入实际
   `mobile_scanner` 页面；扫描页同样没有输入或复制入口。

该 Android Emulator 没有可用于连接 Mac 主机 CoreBluetooth 广告的 BLE passthrough。以上
观察止于 scanner 页面，未扫描出可继续连接的真实 Mac peer，也未观察 service discovery、
control write、status read/notify 或 `ACCEPTED`。CDM `id=1` 只证明模拟器系统保存了 association，
不证明对端是本轮 Mac peripheral，更不证明 pairing 已完成。

## 权限与用户提示

Android 最终 merged manifest 的来源目前包括：

- app：`android.permission.CAMERA`；相机 feature 为 optional；
- native module：`BLUETOOTH_SCAN`（`neverForLocation`）、`BLUETOOTH_CONNECT`、
  `REQUEST_OBSERVE_COMPANION_DEVICE_PRESENCE`；
- 没有 `POST_NOTIFICATIONS`，所以通知在 UI 中显示为此版本不申请。

扫码按钮先显示“只在扫码页打开相机、不保存照片”的说明。相机普通拒绝不会把用户送出应用；
永久拒绝时才建议打开本应用系统权限页。扫码成功后只把原始 URI 交给现有 `beginPairing`，不在
Dart 中解析或记录配对 secret。CDM association 是独立的用户确认步骤，不能由二维码绕过。

iOS `Info.plist` 含 `NSCameraUsageDescription`，Podfile 含 `PERMISSION_CAMERA=1`。这只保证当前
源代码中的静态权限配置完整；当前机器没有已验证的完整 Xcode/iOS SDK，因此 iOS 仍是
**NOT RUN**。

## 设备与系统验证矩阵

| 场景 | 结果 | 可得出的结论 |
|---|---|---|
| 开发 Mac 广告 local name + service UUID | **LIMITED PASS** | `bluetoothd` 观察到 `Repose Mac` 与精确 service UUID 开始；只证明 Mac 侧广告 |
| 开发 Mac 120 秒到期停止广告 | **LIMITED PASS** | 观察到 stop；只证明本次 Debug 会话生命周期 |
| API 35 模拟器 Debug APK 安装 | **PASS**：`install -r -t` | 只证明当前 Debug APK 可在该模拟器安装/启动 |
| API 35 模拟器附近设备权限 | **PASS**：说明 → 系统权限 → `Allowed` | 只证明前台权限 UX |
| API 35 模拟器 CDM association | **PASS**：`id=1` | 只证明系统 association 存在，不证明 Mac peer |
| API 35 模拟器首页/扫码页 | **PASS**：`READY TO PAIR`、唯一扫码入口、相机说明/授权、真实 scanner、无输入/复制 | 只证明前台 UI 和系统权限流程 |
| 模拟器 ↔ Mac GATT service/control/status | **NOT OBSERVED**：无主机 BLE passthrough | 不得声称看到 Mac service 或完成 `ACCEPTED` |
| 当前 Debug APK 安装到 RMX3888 | **NOT RUN**：ADB `unauthorized` | 不得把模拟器观察外推到 realme UI 7.0 |
| GT5 Pro 相机权限、CDM 和 GATT | **NOT RUN** | 不得声称真机 pairing 已完成 |
| 应用退后台、熄屏、Doze、进程回收 | **NOT RUN** | 不得声称后台可靠性 |
| RSSI 校准、30 次离开/返回、延迟/耗电 | **NOT RUN** | 不得声称距离精度或性能目标 |
| macOS AuthorizationHost / 密码 fallback | **NOT RUN / GATE CLOSED** | 不得声称系统自动解锁 |
| Android/iOS release | **EXPECTED DENY / GATE CLOSED** | 当前工件不可发布 |

此前 RMX3888 的较早 Debug APK 安装/启动和测试 UID 下 5/5 Keystore/SQLite instrumentation 仍是
有效的有限历史证据，详见 [GT5 Pro 记录](android-gt5-pro-results.md)。它们没有执行本轮相机、
CDM 或 GATT central 路径；当前连接状态为 ADB `unauthorized`，所以本轮没有向真机安装。

## 发布边界

以下条件没有因为本轮 Debug 配对而改变：

1. macOS Authorization Services 未读取或修改，Authorization Plugin 未安装进
   `authorizationhost`；
2. API 31–35 前台 association/GATT 兼容路径只在 Android Debug source set；Release/Profile
   继续使用 API 36 production runtime 门禁和 fail-closed host API；
3. production BLE role 仍为 `Disabled`；Android Release 仍由生产签名门禁拒绝，iOS
   Release/Archive 仍由编译门禁拒绝；
4. 系统密码必须始终可用，本轮没有锁屏或密码 fallback 实验；
5. 只有完成专用 Mac、GT5 Pro 真机互操作、后台/重启/功耗/误判、签名公证和系统恢复矩阵后，
   才能讨论开放自动解锁。

总体发布结论仍为 **NOT PRODUCTION READY / GATE CLOSED**。完整状态见
[验证汇总](verification-summary.md) 和 [手机靠近解锁说明](../phone-unlock.md)。
