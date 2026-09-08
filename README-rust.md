# Repose（Rust / Tauri 版）

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

产物位于 `src-tauri/target/release/bundle/macos/Repose.app`。当前构建面向 Apple Silicon，最低支持 macOS 14；本地构建没有 Apple Developer 签名或公证。

生成带版本号的 App 和 DMG（含本机 ad hoc 签名及 SHA-256 校验文件）：

```sh
npm run package:mac:release
```

产物位于 `release/Repose-<版本>-mac-<架构>/`。DMG 内包含 App 和 Applications 快捷入口；脚本会校验镜像与 App 签名，并拒绝覆盖已存在的同版本发布目录。

## 原生能力

- 关闭主窗口后继续驻留顶部菜单栏。
- Rust 每秒检查全系统键盘和鼠标闲置时间，30 秒后请求 macOS 锁屏。
- 强制休息为每块显示器创建系统 WKWebView 覆盖页，并通过 AppKit 展示策略禁用应用切换、强制退出面板、隐藏应用和普通退出。
- 大休息在每块覆盖页中展示同一套离线 3D 拉伸训练：8 个动作每 30 秒自动轮播，支持前后切换，并重点覆盖肩颈和上背。主窗口和覆盖页共享动作与程序化关节动画定义。
- 小休息按稳定休息 ID 从五组文案库轮换提醒、延期、追回与完成反馈；同一次休息始终显示同一句。微笑小花在 Dock、窗口和菜单栏保持一致；花瓣使用单一连续轮廓，笑脸直接融入其中。
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

3D 训练使用本地 Three.js 与打包的 CC0 MakeHuman 人体网格（约 1.2 MB），无需联网。灰白人体与红色表面区域提示借鉴解剖教学表达，红色不代表精确分割的肌肉。`prefers-reduced-motion` 会固定到代表性姿势；WebGL 初始化或模型加载失败时保留文字指导，不影响原生休息倒计时。素材和转换过程见 `src/assets/STRETCH-HUMAN-LICENSE.md`。

## 验证打包后的拉伸界面

运行以下命令可在真实 WKWebView 中预览拉伸动作，不启动休息计时、覆盖屏幕或闲置锁屏，也不修改已保存的偏好：

```sh
REPOSE_STRETCH_PREVIEW=1 "src-tauri/target/release/bundle/macos/Repose.app/Contents/MacOS/repose"
```

正常双击 App 时仍运行完整的休息提醒应用。前端双入口产物可使用 `npm run verify:build` 检查。

图标源为 `public/favicon.svg`，菜单栏单色版为 `public/tray.svg`。分别运行 `npx tauri icon public/favicon.svg -o src-tauri/icons` 和 `npx tauri icon public/tray.svg -p 18 -o src-tauri/icons/tray` 可重新生成。`npm run verify:mac` 会检查最终 App 中的图标声明、ICNS 内容、名称与版本；发布脚本自动执行此检查。

实现依据：[Tauri 2](https://v2.tauri.app/start/)、[macOS 使用系统 WKWebView](https://v2.tauri.app/reference/webview-versions/)、[macOS App 打包](https://v2.tauri.app/distribute/macos-application-bundle/)。
