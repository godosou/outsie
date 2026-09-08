# App 选择、Codex 默认快捷键和配置导入验证

日期：2026-09-08。分支：`codex/phone-work-console`。

## 结果

- `npm test`：125 通过。新增覆盖导入的按键顺序与 2000ms 等待、错误文件原子拒绝、类型和容量边界、保留已有配置、App 选择后才允许保存、App 更换保留宏。
- `cargo test --lib`：25 通过。新增覆盖选择路径持久化、恢复预置保留 App 选择、旧预置升级不覆盖自定义按键、96 个操作边界。
- `flutter test test/work_console_test.dart`：15 通过；直接读取共享 Codex JSON，确认 77 个默认操作完整解析、96 个可用、97 个拒绝。
- 原生 App 元数据测试：编译运行 `src-tauri/tests/native_console_apps.m`，读取 117 个已安装 App，验证 PNG 图标、Terminal 发现、无效目标拒绝、同 Bundle ID 不同路径匹配。没有注入系统按键。
- `scripts/verify-console-bluetooth.sh`：Kotlin/Rust 加密蓝牙仿真通过，零跳过；使用包含完整 Codex 列表的 Rust 配置，验证现有执行、取消、断线与重放行为。
- `npx tauri build --debug --bundles app` 成功；Mac 包与前端产物检查通过。
- `flutter build apk --debug` 成功；普通蓝牙 APK 已安装到已连接的 `9535e9c2` 测试手机，ADB 返回 Success。

## Mac 界面检查

重新启动当前工作树的开发 App，验证「App 工作台」已有导入、AI 说明、App 选择入口，不再出现 Bundle ID 文本框。打开 App 选择器，图标、应用名和安装路径显示正常；搜索 Terminal 返回单个结果。

选择 Terminal 后原 tmux 步骤保持不变。保存成功，持久配置版本 3：tmux 7 操作、Codex 77 操作、飞书 3 操作，tmux 目标路径为 `/System/Applications/Utilities/Terminal.app`。Codex 页面可看到终端、语音、任务、审查和浏览器等默认按键。

点击导入能够打开 macOS 文件选择窗口；完整文件读取与草稿选择/保存交互由 React 测试验证。本轮未在真实目标应用逐个执行全部 77 个快捷键。

## 产物与尚需用户参与

- Mac：`src-tauri/target/debug/bundle/macos/Repose.app`。
- Android：`mobile/build/app/outputs/flutter-apk/app-console-bluetooth.apk`。
- APK SHA-256：`f60ec54f3ff6edaf871fe8f4022a4bf4afe8b0deef66a650c39358902e819e84`。
- 导入示例：`docs/examples/safari-console.json`。
- 真实 Mac 键盘注入仍需用户授予辅助功能权限，并在手机和 Mac 完成配对确认。仿真通过不代表真实无线链路和 77 个 App 快捷键逐项验收通过。
- 当前功能边界：无默认绑定的历史对话搜索由用户自行绑定；Appshot 双 Command、鼠标、长按和仅修饰键手势不支持。
