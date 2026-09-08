# App 工作台：实现与真机交接

2026-09-08，分支 `codex/phone-work-console`。基于蓝牙配对分支快照继续实现，未修改正在开发的原始配对 worktree。

## 已完成

- 手机以 App 为入口，使用 Mac 保存的预置及自定义快捷键、录制或手工编辑的按键序列；支持等待、停止、按钮拖动及独立布局保存。
- 控制通道使用既有配对的 BLE 加密连接，无局域网 IP、第二个配对二维码或网络回退。
- 修复实际主题下排序保存按钮无限宽的问题。
- 修复慢心跳期间点击被忙状态丢弃的问题：一条用户操作等当前心跳结束再发，断连后丢弃，不并发或重放。
- Mac 将“蓝牙未授权 / 已关闭 / 不支持 / 正在发布 / 发布失败”分开显示，收到真实广播成功回调后才显示等待手机。

## 实际设备检查

| 环境 | 已验证 | 未宣称完成 |
| --- | --- | --- |
| Android 模拟器 | App 切换、两秒序列及停止、长按拖动并保存 | 无真实无线、无系统键盘 |
| realme RMX3888 | 普通蓝牙 APK 安装成功、启动成功，原生已配对设备列表为空时显示中文配对指引，无仿真入口、无 IP 输入 | 尚未扫码、确认配对或传输工作台无线指令 |
| 本机 Mac | 普通 Debug App 启动；蓝牙系统开启且 Repose 已获蓝牙权限；服务发布成功，界面显示“等待手机配对” | 辅助功能未授权，未注入真实 App 按键 |
| Mac 配置操作 | UI 修改间隔为 2 秒并保存，磁盘 JSON 为 2000ms；重启后保留配置；恢复 tmux 100ms 默认值；录制 Ctrl+B、Enter 后 Escape 正确结束，取消草稿恢复已保存内容 | 录制验证不等于操作其它 App 的权限验证 |

[Device screenshot omitted from public source.]

## 最后检查

- Flutter 全量 **108 通过**，静态分析无问题。
- 前端全量 **119 通过**；TypeScript 与生产构建通过。
- Rust 本次定向 **22 个 lib + 13 个蓝牙原生集成测试通过**；Clippy `-D warnings` 通过。
- Android/Rust 跨运行时协议仿真再次通过，**1 项执行、0 跳过**。实际 Kotlin 加密/分片与 Rust 工作台相连，覆盖计时间隔、取消、断连、重放和撤销。
- Mac Debug App 打包、图标/标识及前端入口验证通过；普通 Android Debug APK 构建成功。
- 前一轮 Rust workspace 405 项、Android JVM 151 项结果见[仿真记录](phone-work-console-bluetooth-results.md)，不与上述定向用例相加。

## 安装包

- 真机已安装：`mobile/build/app/outputs/flutter-apk/app-console-bluetooth.apk`。
- 真机包 SHA-256：`67ddd6e7ffa3c26a7b10715db748f821696a9f892245f204b79e16fafbcc7cfd`。
- Mac：`src-tauri/target/debug/bundle/macos/Repose.app`，已启动本分支构建。`/Applications/Repose.app` 是另一个安装位置，授权和验收时应选择本分支这个 App。
- `app-console-simulation.apk` 保留为首轮显式仿真产物，不能用它验收真机蓝牙。

## 用户回来后只需参与这些步骤

1. 在本分支 Mac App 的「App 工作台」点击「打开权限设置」，在系统的辅助功能列表中允许此 Repose。该权限允许它向其它 App 发送按键，需要用户确认系统授权。
2. 在 Mac 的「手机钥匙」点击「开始配对」，用已安装的手机 App 扫描新生成的二维码，按提示选择这台 Mac 并完成两端确认。需要用户实际将相机对准屏幕；不保留一张会在用户回来前过期的二维码。

随后可继续验收：手机进入「App 快捷操作」，选择 Mac；先验证 tmux 切换/分屏，再验证自定义“快捷键 → 等待 2 秒 → 下一键”、停止、关蓝牙及撤销配对。此部分因扫码和辅助功能授权尚未完成，明确保留为待验收，不计入无线通过结果。

当前仍沿用配对分支的 Debug 门控及进程内凭据，重启 App 后需重新配对；Profile/Release 的原有门控没有被绕过。iOS 原生 BLE 控制及自动解锁正式发布不属于这次 Android 工作台验收。
