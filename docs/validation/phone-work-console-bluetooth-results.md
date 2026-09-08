# 手机工作台蓝牙仿真验证

日期：2026-09-08。分支：`codex/phone-work-console`。

这是首轮仿真阶段记录。后续已安装普通蓝牙版真机并修复慢心跳交互和连接状态提示；最终结果见[真机交接记录](phone-work-console-device-handoff.md)。

## 结论与范围

已将工作台从局域网通道改为复用既有配对的 BLE 通道，并完成无硬件协议仿真和 Android 模拟器交互验证。本次只安装到 `emulator-5554`，未安装到实体手机；不代表真实蓝牙无线或真实 Mac 按键注入已经验收。

## 协议与原生仿真

`scripts/verify-console-bluetooth.sh` 已通过，Kotlin/Rust 跨运行时集成用例执行 1 项，失败、错误、跳过均为 0。脚本先构建 Rust 仿真进程，再运行 Android JVM 用例并检查 XML 报告，避免缺少仿真二进制时跳过造成假通过。

- Android 生产 HKDF/AES-GCM 及分片代码与 Rust 生产解密、授权、工作台服务互通；独立生成的固定向量覆盖双向计数及加密帧。
- 模拟 ATT 20 字节请求分片和 64 字节响应分片，通过进程管道传输，不创建局域网控制连接。
- 验证状态读取、App 激活、配置缓存省略、版本化布局保存及重复请求拒绝。
- 假键盘记录每次按键及时间：完整序列先发第一键，等待至少 500ms 再发第二键；取消和断连后等待超过原定间隔，第二键均不出现。
- 重连后的旧 challenge、重放计数及撤销配对后的消息被拒绝。
- macOS Objective-C 测试编译实际 BLE 实现，替换 CBPeripheralManager 验证分片、订阅、通知背压及生命周期。旧 challenge 的撤销不能关闭新订阅。
- Rust 回归覆盖旧连接代次不能清空新连接，以及取消后已排入主线程队列的键不能注入。

## 自动检查

| 检查 | 结果 |
| --- | --- |
| Rust workspace 测试 | 405 通过 |
| 最后原生撤销修复后的 Rust 定向复测 | lib 21、debug_bluetooth_pairing 13 通过 |
| Rust fmt、Clippy `-D warnings` | 通过 |
| Android Debug JVM 全量测试 | 151 通过、0 跳过 |
| Android Profile 编译 | 通过，保留原有不可用门控 |
| Flutter 全量测试 | 106 通过 |
| 最后布局修复后的工作台定向测试 | 12 通过，含真实主题下长按拖动、箭头重排和保存 |
| Flutter analyze | 无问题 |
| JS/React 测试 | 118 通过 |
| TypeScript、前端构建 | 通过 |
| macOS Debug App 构建及 verify-mac-app/verify-build | 通过 |
| git diff --check | 通过 |

上述全量与定向复测不是互相独立的用例总数，不能相加。

## 模拟器交互与修复

显式 Debug 构建参数 `REPOSE_CONSOLE_SIMULATION=true` 提供模拟 Mac，页面显示蓝牙仿真标记；不调用真实无线或系统键盘。普通构建不启用此入口。

模拟器已实际操作连接、两秒序列及中途停止、切换 Codex、进入排序、长按将分屏按钮移到第二位、保存布局。保存后界面恢复可执行状态，切换 App 正常。

人工检查发现真实应用主题使用 `Size.fromHeight(52)` 作为实心按钮最小尺寸，造成排序行中的保存按钮无限宽。使用实际主题的测试复现 `BoxConstraints forces an infinite width` 后，为该行按钮指定有限最小宽度；回归及重新安装后的界面检查均通过。未修改全局主题或正式渲染设置。

[Device screenshot omitted from public source.]

[Device screenshot omitted from public source.]

## 构建产物及下一阶段

- 仿真 APK：`mobile/build/app/outputs/flutter-apk/app-console-simulation.apk`。
- SHA-256：`5b1d27d9d16ebb25af4a63b05803ea7de4278c1b3d3da4948cac39b6be2476ab`。
- Mac 普通 Debug 构建：`src-tauri/target/debug/bundle/macos/Repose.app`。
- 当前 `app-debug.apk` 同样是仿真构建，不能将其当作真机 BLE 包。真机阶段必须重新构建不带仿真参数的 APK。
- 后续真机验收：既有两端确认配对、真实 GATT 连接与通知、Mac 辅助功能权限、实际 App 快捷键及 tmux 前缀、关蓝牙/撤销配对时停止。iOS 原生 BLE 控制不在本次验收范围。
- 当前沿用 Debug 配对进程内凭据，应用重启后需重新配对。

使用方法见 [手机蓝牙 App 工作台](../phone-work-console.md)，协议见 [蓝牙实现方案](../plans/2026-09-08-work-console-bluetooth.md)。
