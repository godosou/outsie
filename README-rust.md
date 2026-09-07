# Repose Lite（Rust / Tauri 版）

这个分支把 Electron 桌面层替换为 Tauri 2：应用逻辑运行在 Rust 中，界面使用 macOS 自带的 WKWebView，不再随安装包携带 Chromium。现有 React 界面、休息计划和记录逻辑继续复用。

## 运行与打包

需要 Rust、Node.js、npm 和 Xcode Command Line Tools。

```sh
npm install
npm run desktop:dev
```

生成 macOS App：

```sh
npm run package:mac
```

产物位于 `src-tauri/target/release/bundle/macos/Repose Lite.app`。当前构建面向 Apple Silicon，最低支持 macOS 14；本地构建没有 Apple Developer 签名或公证。

## 原生能力

- 关闭主窗口后继续驻留顶部菜单栏。
- Rust 每秒检查全系统键盘和鼠标闲置时间，30 秒后请求 macOS 锁屏。
- 强制休息为每块显示器创建系统 WKWebView 覆盖页，并通过 AppKit 展示策略禁用应用切换、强制退出面板、隐藏应用和普通退出。
- 小休息可延迟 1 分钟，大休息可延迟 5 分钟；同一次休息只能延迟一次，之后执行完整休息。
- 延迟和结束由稳定的休息 ID 确认，重复请求不能增加延迟机会。
- 隐藏窗口时关闭 WKWebView 后台降频，确保菜单栏驻留期间计时仍准确；计时状态每 15 秒落盘一次，并在页面退出时立即保存。

Rust 桌面实现位于 `src-tauri/src/lib.rs`，macOS AppKit/CoreGraphics 桥接位于 `src-tauri/native/macos.m`，前端桥接位于 `src/tauriBridge.ts`。

实现依据：[Tauri 2](https://v2.tauri.app/start/)、[macOS 使用系统 WKWebView](https://v2.tauri.app/reference/webview-versions/)、[macOS App 打包](https://v2.tauri.app/distribute/macos-application-bundle/)。

## 手机靠近解锁原型

仓库包含一个“手机离开后重新靠近，再尝试解锁已登录 macOS 会话”的安全原型。它由
Rust 状态机和协议、最小 macOS Authorization Plugin/服务、React/Tauri Mac 设置页、面向
Android/iOS 的 Flutter 手机界面以及 Android 原生后台层组成。BLE RSSI 只用于经过校准的
便利级接近判断；真正的授权还需要已配对手机持有的 P-256 私钥完成挑战响应。

该能力目前仍是 **GATE CLOSED**：普通构建不会安装或启用系统授权组件，Android 生产 BLE
角色保持禁用，iOS 原生后台实现等待受支持的 Xcode/iOS SDK，GT5 Pro 与专用测试 Mac 的真机
矩阵也尚未执行。现阶段不能把它当作可用的自动解锁功能，更不能跳过系统密码。

- [使用范围、配对、校准和恢复说明](docs/phone-unlock.md)
- [安全威胁模型](docs/security/phone-unlock-threat-model.md)
- [验证状态与发布门禁](docs/validation/verification-summary.md)
- [线协议 v1](docs/protocol/repose-unlock-v1.md)
