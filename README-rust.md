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
- 普通无键鼠操作仍算专注；macOS 锁屏、显示器休眠、系统睡眠或用户会话离开会冻结专注。
- 专注期间的锁屏/休眠按真实时长记为被动休息：不足应休息时长时保留原专注进度，足量时只完成一次应有休息并返回完整专注周期；解锁或会话恢复会强制结束残留的不活动状态。
- 已开始的主动休息在锁屏或睡眠期间继续，到期后只完成当前休息，不把多余离开时间计入下一轮。
- 活动推进使用 `performance.now()`，原生不活动区间和严格休息截止时间使用包含睡眠的 `mach_continuous_time`；系统墙钟只用于日期与展示。
- 计时状态每 15 秒落盘，并在生命周期边界和页面退出时立即保存；退出后重开只恢复快照，不补算应用未运行期间的专注或休息。
- 「我的记录」提供单日 24 小时图表，以堆叠柱分别展示每小时的专注与休息，并可查看最近 35 天；所有分时数据仅保存在本机。
- 从 0.3.x 升级时会保留原有每日总量和休息足迹，但旧数据没有小时信息，因此不会猜测或伪造升级前的分时分布。

Rust 桌面实现位于 `src-tauri/src/lib.rs`，macOS AppKit/CoreGraphics 桥接位于 `src-tauri/native/macos.m`，前端桥接位于 `src/tauriBridge.ts`。

实现依据：[Tauri 2](https://v2.tauri.app/start/)、[macOS 使用系统 WKWebView](https://v2.tauri.app/reference/webview-versions/)、[macOS App 打包](https://v2.tauri.app/distribute/macos-application-bundle/)。
