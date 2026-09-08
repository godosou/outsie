# 手机靠近解锁验证汇总

日期：2026-09-08

发布结论：**NOT PRODUCTION READY / GATE CLOSED**

## 结论

当前仓库提供可审查、可自动测试的工程原型，但没有完成真实“锁屏 → 离开 → 靠近 → 系统
解锁”。自动化结果只能证明纯逻辑、固定协议、失败关闭边界和构建契约；它们不能证明 BLE
后台可靠性、RSSI 距离质量、AuthorizationHost 行为、密码 fallback 延迟或平台功耗。

Debug 构建现在增加了 Mac 一次性二维码／CoreBluetooth peripheral 与 Android 相机／CDM／
GATT central 配对路径。开发 Mac 上观察到精确广告开始并在 120 秒到期后停止；API 35 模拟器
上观察到 APK 安装、附近设备授权、CDM association `id=1`、`READY TO PAIR` 和真实相机扫描
页。模拟器不能通过 Mac 主机蓝牙完成 GATT，`ACCEPTED` 未观察；realme 真机仍为 ADB
`unauthorized`。因此这里的“Debug 前台流程可用”不得解释为“真机配对或系统自动解锁可用”。

本次开发没有读取或修改真实 authorizationdb，没有安装/加载 Authorization Plugin 或 launchd
服务，没有锁定当前 Mac，也没有执行任何 `--apply` 命令。

## 分层状态

| 层 | 当前状态 | 已有证据 | 尚缺证据 |
|---|---|---|---|
| Rust 校准/状态机/协议/防重放/许可 | **自动化实现** | 单元、属性、固定向量与并发失败测试 | 真机时序与端到端事件来源 |
| 一次性配对 payload v1 | **自动化实现** | Rust/Kotlin 共用 164-byte fixture；严格长度、UTF-8、SEC1 P-256、尾部拒绝和固定 BLE UUID | 真机交换、密钥安装与销毁观察 |
| macOS Debug 配对 | **Debug only / limited observation** | Mac 二维码与 CoreBluetooth peripheral；`bluetoothd` 观察到 `Repose Mac` + 精确 service UUID 开始广播，并在 120 秒到期后停止 | Android 发现/连接/write/notify、持久化、后台和 release |
| macOS 授权规则变换与事务工具 | **自动化原型，apply 硬关闭** | plist fixture、fake adapter、故障/恢复与静态包验证 | 专用 Mac、签名、公证、真实 Authorization Services/launchd、断电恢复 |
| macOS Authorization Plugin/IPC | **已编译原型，未加载系统** | 固定 wire、peer/超时/单次许可与 sanitizer 测试 | AuthorizationHost 无人操作重评估、密码 fallback/keychain、跨 OS 矩阵 |
| Repose 设置页 | **Debug 配对 UI；系统解锁后端关闭** | 生成二维码、倒计时/过期隐藏、无复制入口、CoreBluetooth Debug backend；纯 model/React 交互测试 | 生产后端、真实配对/校准与 Authorization 集成 |
| Flutter 共享手机界面 | **Debug/test shell** | Mac 同款品牌、中英文、相机扫码与权限/后台引导；91 个 controller/widget 测试、analyze、Debug APK；API 35 模拟器走查扫码入口 | realme 真机扫码、商店签名、可发布配置、端到端 |
| Android Debug 配对 | **API 35 前台 UX observed；GATT E2E NOT OBSERVED** | API 35 安装；附近设备说明/授权；CDM `id=1`；`READY TO PAIR`；唯一 QR 入口；相机说明/授权和 `mobile_scanner` 页面；Debug JVM 144/144 | 主机蓝牙 GATT、`ACCEPTED`、GT5 Pro 安装与 Mac 互操作 |
| Android API 36 presence/Keystore | **部分真机验证；production path 未接入** | JVM 契约、AAR surface、lint/debug；RMX3888 测试 UID 下 5/5 Keystore/SQLite instrumentation | 具体 TEE/StrongBox 枚举值、production UID、presence、后台/重启/功耗 |
| Android 解锁 BLE/响应器 | **自动化实现；production role Disabled** | 当前树 Debug 144/144、Release 120/120、Profile 120/120；固定向量、分片/生命周期、持久化/CAS；真机历史 5/5 Keystore/SQLite 测试 | 真实解锁 GATT/Companion Presence、角色选择、30 次循环与端到端延迟 |
| macOS production durable runtime | **未接入 / deny-only** | core 抽象、memory/fake store 与 broker 并发契约 | 生产持久化 adapter、真实 session/BLE runtime 和私有 service wiring |
| iOS 原生后台层 | **BLOCKED / NOT RUN** | Release/Archive compile-time gate | 支持 iOS 26 SDK 的完整 Xcode、模拟器、物理 iPhone 与恢复矩阵 |

## 当前环境证据

| 项目 | 观察结果 |
|---|---|
| Mac | MacBook Pro `Mac15,3`, Apple M3, macOS 14.6.1 (`23G93`) |
| Apple 工具链 | `/Library/Developer/CommandLineTools`; Apple clang 16；没有选中完整 Xcode |
| iOS SDK | `iphoneos` SDK 不可用；iOS 测试 **NOT RUN** |
| Flutter | 3.38.9 stable / Dart 3.10.8（缓存元数据与后续离线构建环境） |
| Android | SDK platform/build-tools 36 已供自动化编译；API 35 `emulator-5554` 已用于前台 UX 观察 |
| 目标手机 | 用户称 realme GT5 Pro / realme UI 7.0；设备自报 realme `RMX3888`、Android 16、API 36、build `RMX3888_16.0.10.500(CN01)` |
| ADB | API 35 `emulator-5554` 为 `device` 并已安装本轮 APK；realme 为 `unauthorized`，本轮未安装；记录不包含真机序列号 |
| macOS 签名 | `security find-identity -v -p codesigning` 未找到有效 identity |

更完整的逐平台记录见：

- [macOS Authorization 结果](macos-authorization-results.md)
- [Android GT5 Pro 结果](android-gt5-pro-results.md)
- [手机 UI、权限与 Debug 配对结果](mobile-ux-results.md)
- [iOS 后台结果](ios-background-results.md)

## 自动验证清单

合入前必须从当前工作树重新运行并记录退出码；不能引用旧日志替代新证据。

| 验证项 | 命令 | 本轮结果 |
|---|---|---|
| Clean JS dependency install | `npm ci` | **PASS**, exit 0；38 packages；报告 esbuild/fsevents install scripts 尚未列入 npm allowScripts，不影响随后测试/构建 |
| Web/React tests | `npm test` | **PASS**, exit 0；52/52；`react-test-renderer` 输出上游 deprecated warning |
| Web production build | `npm run build` | **PASS**, exit 0；TypeScript + Vite production build |
| Rust formatting | `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check` | **PASS**, exit 0 |
| Rust lint | `cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings` | **PASS**, exit 0 |
| Rust workspace tests | `cargo test --manifest-path src-tauri/Cargo.toml --workspace --all-targets` | **PASS**, exit 0；当前工作树 full workspace 通过；sandbox 首轮 3 个 AF_UNIX bind 用例因 EPERM 失败后，在允许本地 socket 的环境完整重跑通过 |
| macOS plugin native tests | `make -C native/macos/authorization-plugin test SANITIZE=address,undefined` | **PASS**, exit 0；sandbox 首轮临时 AF_UNIX bind 被 EPERM 拒绝，允许本地 socket 后两项 sanitizer 测试通过 |
| macOS prototype build | `./scripts/build-macos-auth-prototype.sh --configuration release --arch arm64 --sign ad-hoc` | **PASS**, exit 0；arm64 ad-hoc 原型，仅为未公证开发产物 |
| macOS artifact verification | `./scripts/verify-macos-auth-artifacts.sh target/macos-auth/release` | **PASS**, exit 0；静态 artifact/本地 designated requirement 验证，不代表生产签名或系统加载 |
| Flutter dependency resolution | `cd mobile && flutter pub get` | **PASS**, exit 0；`permission_handler 12.0.1` / `permission_handler_android 13.0.1`；报告 11 个受当前约束限制的较新版本及 locale warning |
| Flutter QR/权限聚焦测试 | `cd mobile && flutter test test/pairing_scanner_test.dart test/app/pairing_scanner_flow_test.dart test/app/camera_permission_manifest_test.dart test/app/localized_workflows_test.dart` | **PASS**, exit 0；9/9；覆盖 canonical URI 上限/拒绝、相机说明、拒绝/永久拒绝、重复扫描、双语与 Android/iOS 静态权限契约 |
| Flutter static analysis | `cd mobile && flutter analyze` | **PASS**, exit 0；0 issues（`No issues found`）；仅重复 locale warning |
| Flutter tests | `cd mobile && flutter test` | **PASS**, exit 0；91/91；覆盖扫码新增项及原有品牌、权限、状态同步、配对/撤销和校准回归 |
| iOS camera Podfile syntax | `cd mobile/ios && ruby -c Podfile` | **PASS**, exit 0；只证明 Ruby/宏配置语法，未运行 CocoaPods、Xcode 或 iOS app |
| Flutter Android Debug build | `cd mobile && flutter build apk --debug` | **PASS**, exit 0；`build/app/outputs/flutter-apk/app-debug.apk`，SHA-256 `118a13cbf5f8fb56f6e927c67d4ec7453b7407e92933ed2e830a321dad374abc`；已安装 API 35 模拟器，未安装 GT5 Pro |
| Mac Debug CoreBluetooth advertisement | Mac App「开始配对」+ `bluetoothd` 观察 | **LIMITED PASS**；广告 local name=`Repose Mac`、service UUID=`A53E0001-7A6B-4D59-9F2E-5245504F5345`，120 秒到期后观察到停止；未观察 Android peer 或 GATT characteristic 交换 |
| Android 权限状态分类 | `cd mobile/android && ./gradlew --no-daemon :app:testDebugUnitTest --tests '*PermissionStatusTest'` | **PASS**, exit 0；5/5；不需要权限、首次请求、拒绝、永久拒绝、已授权 |
| Android 权限与品牌资源检查 | `cd mobile/android && ./gradlew --no-daemon :app:lintDebug` | **PASS**, exit 0；无 error；工具版本、资源限定符、legacy 图标与 KTX 建议 warnings，未隐藏 |
| Android current Debug unit tests | `cd mobile/android && ./gradlew --no-daemon :repose_unlock_native:testDebugUnitTest` | **PASS**, exit 0；144/144；包含仅位于 Debug source set 的 API 31–35 前台 association/GATT 兼容路径 |
| Android current Release unit tests | `cd mobile/android && ./gradlew --no-daemon :repose_unlock_native:testReleaseUnitTest` | **PASS**, exit 0；120/120；Release variant 使用 fail-closed host API，不包含 Debug pairing host |
| Android current Profile unit tests | `cd mobile/android && ./gradlew --no-daemon :repose_unlock_native:testProfileUnitTest` | **PASS**, exit 0；120/120；Profile variant 使用 fail-closed host API，不包含 Debug pairing host |
| Android 既有 instrumentation APK compile | `cd mobile/android && ./gradlew --no-daemon assembleDebugAndroidTest` | **PASS（本轮配对改动前）**；app/plugin instrumentation APK 编译成功；当前树待主验证复跑 |
| Android 既有 device instrumentation | `cd mobile/android && ./gradlew --no-daemon :repose_unlock_native:connectedDebugAndroidTest` | **PASS**；RMX3888 上 5/5 methods、3 classes；测试专用包 `ai.repose.mobile.unlock.test`，不等价于当前配对 APK、production UID 或 BLE E2E |
| Android API 35 Debug install | `adb -s emulator-5554 install -r -t mobile/build/app/outputs/flutter-apk/app-debug.apk` | **PASS**；当前 Debug APK 覆盖安装成功；真机仍未安装 |
| Android API 35 CDM/附近设备 UI | 系统 chooser、权限页与 `dumpsys companiondevice/package` 观察 | **PASS（前台 UX）**；association `id=1`；附近设备用途说明 → 系统权限 → `Allowed`；不证明 GATT |
| Android API 35 scanner UI | 首页与相机流程走查 | **PASS（前台 UX）**；`READY TO PAIR`；唯一 `Scan Mac QR code` 入口；说明 → 系统权限 → 真实 scanner 页面；无输入/复制入口 |
| Android Release safety gate | `cd mobile/android && ./gradlew --no-daemon assembleRelease` | **EXPECTED DENY**, exit 1；在编译前以 `production signing is configured` 门禁拒绝，不是可发布产物 |
| Timer scope audit | `git diff --exit-code 6a5b60e -- src/hooks/useBreakTimer.ts src/lib/timer.ts src/lib/timer.test.ts` | **PASS**, exit 0；既有休息计时器生命周期无改动 |
| Secret/material scan | `git grep -n -I -E '(PRIVATE KEY|pairingSecret|sessionKey|recovery key)'` + PEM/keystore 扩展名扫描 + 人工误报复核 | **PASS**；命中仅为审计命令与 FileVault recovery 文档描述；没有 PEM 私钥标记或未跟踪 keystore/签名文件 |
| Worktree/artifact audit | `git status --short` + 未跟踪文件分类 | **PASS**；未跟踪项均为本任务源文件/文档，没有生成安全工件；构建输出保持 ignored |

## 设备与系统矩阵

| 验证 | 结果 | 发布影响 |
|---|---|---|
| GT5 Pro manufacturer/model/OS/API | **PASS**：RMX3888 / Android 16 / API 36 / 已记录 build | 仅为设备身份，不选择 Android BLE 角色 |
| GT5 Pro Bluetooth/BLE/Companion/Keystore feature 查询 | **PASS**：系统声明支持 | 仅是 feature 声明，不证明真实无线或后台行为 |
| API 35 模拟器当前 Debug APK 安装 | **PASS**：`install -r -t` | 只证明模拟器可安装/启动当前 Debug 工件 |
| API 35 模拟器 CDM association | **PASS**：`id=1` | 只证明系统 association 创建；不证明真实 Mac 设备身份或 GATT |
| API 35 模拟器附近设备权限 UX | **PASS**：前置说明 → 系统权限 → `Allowed` | 只证明当前前台权限流程 |
| API 35 模拟器扫码 UX | **PASS**：`READY TO PAIR`、唯一扫码入口、相机说明/授权、真实 scanner 页面、无输入/复制入口 | 只证明相机页面与导航；没有完成有效 Mac QR/GATT |
| GT5 Pro 测试 UID Keystore/SQLite instrumentation | **PASS**：5/5 | 不外推到 production UID、Companion Presence 或 GATT |
| Mac Debug 广告 start/120s expiry stop | **LIMITED PASS**：`Repose Mac` + 精确 service UUID | 只证明开发 Mac 广告生命周期观察，不证明手机发现或连接 |
| 模拟器 ↔ Mac GATT control/status/`ACCEPTED` | **NOT OBSERVED**：模拟器无 Mac 主机蓝牙通路 | 不得声称 Debug 配对端到端完成 |
| GT5 Pro CDM 关联 | **NOT RUN** | 不得声称系统 association 已建立或 presence 可用 |
| Mac ↔ GT5 Pro GATT control/status/`ACCEPTED` | **NOT RUN** | 不得声称 Debug 配对端到端完成 |
| GT5 Pro 熄屏/UI 关闭/Doze/系统回收 | **NOT RUN** | 不得声称后台可靠 |
| GT5 Pro force-stop/蓝牙切换/重启 | **NOT RUN** | 只允许密码 fallback；未量测恢复行为 |
| GT5 Pro 口袋/背包 calibration 与 30 次循环 | **NOT RUN** | 不得声称距离精度或三秒目标 |
| Android p50/p95 latency、误近、耗电 | **UNASSESSED** | 不得选择生产角色或发布 |
| iPhone 前台/后台/挂起/state restoration | **NOT RUN / BLOCKED** | iOS Release gate 关闭 |
| 专用 Mac 安装与无人操作授权重评估 | **NOT RUN / GATE CLOSED** | macOS apply 与 UI 安装入口关闭 |
| 20 次密码 fallback 与延迟 | **NOT RUN / GATE CLOSED** | 不能证明不会阻断用户 |
| macOS 14/15/26、sleep/wake、keychain、FUS | **NOT RUN** | 无支持矩阵声明 |
| 故障、断电、repair、uninstall | **NOT RUN** | 无生产恢复路径声明 |
| 真实端到端自动解锁 | **NOT RUN** | 功能不可称完成或 production-ready |

## 发布门禁

只有以下证据全部通过并经独立审查，才能把状态从关闭改为候选发布：

1. 专用、可恢复 Mac 上的签名/公证、AuthorizationHost、密码 fallback、keychain、故障、断电、
   repair 与 uninstall 矩阵；
2. GT5 Pro 上明确选出的 BLE 角色以及后台、force-stop、重启、校准、误判、耗电和延迟结果；
3. 受支持 Xcode/iOS SDK 与物理 iPhone 上的完整后台恢复矩阵；
4. 每个平台的 Release 签名、密钥硬件级别、不可导出性和生产配置；
5. 真实端到端测试证明只解锁已登录本地会话，并证明重启、注销、FileVault、Guest、快速用户
   切换和远程登录始终不自动解锁。

在此之前，正确行为是保持 production BLE role、Android/iOS Release、macOS Authorization /
installer backend 和自动解锁入口全部关闭。API 31–35 的前台兼容逻辑只在 Debug variant；
Release/Profile 继续使用 API 36 production runtime 门禁和 fail-closed host API。Debug 配对入口
只能用于明确标注的联调，不能绕过上述门禁。
