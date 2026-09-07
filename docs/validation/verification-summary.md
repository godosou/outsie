# 手机靠近解锁验证汇总

日期：2026-09-08

发布结论：**NOT PRODUCTION READY / GATE CLOSED**

## 结论

当前仓库提供可审查、可自动测试的工程原型，但没有完成真实“锁屏 → 离开 → 靠近 → 系统
解锁”。自动化结果只能证明纯逻辑、固定协议、失败关闭边界和构建契约；它们不能证明 BLE
后台可靠性、RSSI 距离质量、AuthorizationHost 行为、密码 fallback 延迟或平台功耗。

本次开发没有读取或修改真实 authorizationdb，没有安装/加载 Authorization Plugin 或 launchd
服务，没有锁定当前 Mac，也没有执行任何 `--apply` 命令。

## 分层状态

| 层 | 当前状态 | 已有证据 | 尚缺证据 |
|---|---|---|---|
| Rust 校准/状态机/协议/防重放/许可 | **自动化实现** | 单元、属性、固定向量与并发失败测试 | 真机时序与端到端事件来源 |
| macOS 授权规则变换与事务工具 | **自动化原型，apply 硬关闭** | plist fixture、fake adapter、故障/恢复与静态包验证 | 专用 Mac、签名、公证、真实 Authorization Services/launchd、断电恢复 |
| macOS Authorization Plugin/IPC | **已编译原型，未加载系统** | 固定 wire、peer/超时/单次许可与 sanitizer 测试 | AuthorizationHost 无人操作重评估、密码 fallback/keychain、跨 OS 矩阵 |
| Repose 设置页 | **自动化实现，后端关闭** | 纯 model、挂载 React 交互、窄 Tauri command 与 capability 测试 | 生产后端与真实配对/校准流程 |
| Flutter 共享手机界面 | **Debug/test shell** | Dart controller/widget 测试、analyze、显式 Release gate | 商店签名、可发布配置、真机端到端 |
| Android API 36 presence/Keystore | **已编译离线原型** | JVM 契约、AAR surface、lint/debug/APK 与 instrumentation 编译 | 物理 GT5 Pro 上的 presence、硬件 security level、后台/重启/功耗 |
| Android BLE/响应器 | **自动化实现；生产角色 Disabled** | 94 个 Android JVM 测试中的 transport 26 个、responder 24 个；固定向量、分片/生命周期、持久化/CAS、lint、Debug 与 instrumentation APK 编译 | ADB 设备、真实 Keystore/SQLite/GATT、角色选择、30 次循环与端到端延迟 |
| macOS production durable runtime | **未接入 / deny-only** | core 抽象、memory/fake store 与 broker 并发契约 | 生产持久化 adapter、真实 session/BLE runtime 和私有 service wiring |
| iOS 原生后台层 | **BLOCKED / NOT RUN** | Release/Archive compile-time gate | 支持 iOS 26 SDK 的完整 Xcode、模拟器、物理 iPhone 与恢复矩阵 |

## 当前环境证据

| 项目 | 观察结果 |
|---|---|
| Mac | MacBook Pro `Mac15,3`, Apple M3, macOS 14.6.1 (`23G93`) |
| Apple 工具链 | `/Library/Developer/CommandLineTools`; Apple clang 16；没有选中完整 Xcode |
| iOS SDK | `iphoneos` SDK 不可用；iOS 测试 **NOT RUN** |
| Flutter | 3.38.9 stable / Dart 3.10.8（缓存元数据与后续离线构建环境） |
| Android | SDK platform/build-tools 36 已供自动化编译使用 |
| 目标手机 | 用户指定 realme GT5 Pro、realme UI 7.0、Android 16 |
| ADB | 2026-09-08 在允许访问本地 ADB daemon 的环境运行 `adb devices -l`，exit 0 但设备列表为空；GT5 Pro 验证 **NOT RUN** |
| macOS 签名 | `security find-identity -v -p codesigning` 未找到有效 identity |

更完整的逐平台记录见：

- [macOS Authorization 结果](macos-authorization-results.md)
- [Android GT5 Pro 结果](android-gt5-pro-results.md)
- [iOS 后台结果](ios-background-results.md)

## 自动验证清单

合入前必须从当前工作树重新运行并记录退出码；不能引用旧日志替代新证据。

| 验证项 | 命令 | 本轮结果 |
|---|---|---|
| Clean JS dependency install | `npm ci` | **PASS**, exit 0；38 packages；报告 esbuild/fsevents install scripts 尚未列入 npm allowScripts，不影响随后测试/构建 |
| Web/React tests | `npm test` | **PASS**, exit 0；46/46；`react-test-renderer` 输出上游 deprecated warning |
| Web production build | `npm run build` | **PASS**, exit 0；TypeScript + Vite production build |
| Rust formatting | `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check` | **PASS**, exit 0 |
| Rust lint | `cargo clippy --manifest-path src-tauri/Cargo.toml --workspace --all-targets -- -D warnings` | **PASS**, exit 0 |
| Rust workspace tests | `cargo test --manifest-path src-tauri/Cargo.toml --workspace --all-targets` | **PASS**, exit 0；sandbox 首轮仅 3 个 AF_UNIX bind 用例因 EPERM 失败，随后在允许本地 socket 的环境完整重跑全部 workspace 通过 |
| macOS plugin native tests | `make -C native/macos/authorization-plugin test SANITIZE=address,undefined` | **PASS**, exit 0；sandbox 首轮临时 AF_UNIX bind 被 EPERM 拒绝，允许本地 socket 后两项 sanitizer 测试通过 |
| macOS prototype build | `./scripts/build-macos-auth-prototype.sh --configuration release --arch arm64 --sign ad-hoc` | **PASS**, exit 0；arm64 ad-hoc 原型，仅为未公证开发产物 |
| macOS artifact verification | `./scripts/verify-macos-auth-artifacts.sh target/macos-auth/release` | **PASS**, exit 0；静态 artifact/本地 designated requirement 验证，不代表生产签名或系统加载 |
| Flutter dependency resolution | `cd mobile && flutter pub get` | **PASS**, exit 0；依赖解析成功；报告 8 个受当前约束限制的较新版本及 locale warning |
| Flutter static analysis | `cd mobile && flutter analyze` | **PASS**, exit 0；No issues found；仅重复 locale warning |
| Flutter tests | `cd mobile && flutter test` | **PASS**, exit 0；58/58；仅重复 locale warning |
| Android JVM/lint/debug | `cd mobile/android && ./gradlew --no-daemon clean testDebugUnitTest lintDebug assembleDebug assembleDebugAndroidTest` | **PASS**, exit 0；clean 后 94/94 JVM tests；lint、app Debug APK 与 plugin Debug AAR 均成功；有 SDK XML 工具版本、Kotlin DSL 与 Gradle 9 deprecation warning |
| Android instrumentation APK compile | 同上 `assembleDebugAndroidTest` | **PASS (COMPILE ONLY)**, exit 0；app/plugin instrumentation APK 编译成功；3 个 responder androidTest 与既有测试均未在设备执行 |
| Android Release safety gate | `cd mobile/android && ./gradlew --no-daemon assembleRelease` | **EXPECTED DENY**, exit 1；在编译前以 `production signing is configured` 门禁拒绝，不是可发布产物 |
| Timer scope audit | `git diff --exit-code 6a5b60e -- src/hooks/useBreakTimer.ts src/lib/timer.ts src/lib/timer.test.ts` | **PASS**, exit 0；既有休息计时器生命周期无改动 |
| Secret/material scan | `git grep -n -I -E '(PRIVATE KEY|pairingSecret|sessionKey|recovery key)'` + PEM/keystore 扩展名扫描 + 人工误报复核 | **PASS**；命中仅为审计命令与 FileVault recovery 文档描述；没有 PEM 私钥标记或未跟踪 keystore/签名文件 |
| Worktree/artifact audit | `git status --short` + 未跟踪文件分类 | **PASS**；未跟踪项均为本任务源文件/文档，没有生成安全工件；构建输出保持 ignored |

## 真机与系统矩阵

| 验证 | 结果 | 发布影响 |
|---|---|---|
| GT5 Pro manufacturer/model/API/BLE capability | **NOT RUN** | Android BLE role 不得启用 |
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

在此之前，正确行为是保持 production BLE role、Android/iOS Release、macOS installer backend
和 Repose 安装入口全部关闭。
