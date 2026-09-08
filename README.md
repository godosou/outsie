# Outsie · AI 时代，先照顾好自己。

一款面向 Mac 的休息提醒应用。以柔和的森林绿、简洁的排版与舒展的留白，帮助你在工作中定时休息。

**[产品主页](https://godosou.github.io/outsie/)** · [下载 Mac 预览版](https://github.com/godosou/outsie/releases/tag/v0.6.2) · [主页源码](website/)

Outsie 是产品对外使用的暂定名称，Mac 应用目前仍显示为 Repose（歇一会）。当前 `main` 包含 Rust / Tauri 0.6.2 应用，使用 macOS 自带 WKWebView，开发和打包方法见 [Rust 版说明](README-rust.md)。`electron/` 保留早期实现。

手机相关功能保留在独立分支：

- [`codex/phone-work-console`](https://github.com/godosou/outsie/tree/codex/phone-work-console)：手机 App 工作台、快捷键及按键序列。
- [`codex/phone-proximity-unlock`](https://github.com/godosou/outsie/tree/codex/phone-proximity-unlock)：手机钥匙、靠近与离开、蓝牙配对。

分支中的原生功能和真机验证状态以各分支文档为准。主页 Demo 在浏览器中模拟交互，不执行系统锁屏、手机配对或真实 AI 指令。

## 打开 Mac 应用

完成打包后，在 Finder 中双击 `src-tauri/target/release/bundle/macos/Repose.app`。应用使用系统 WKWebView，日常使用不需要浏览器、Node.js 或 Rust。关闭主窗口后，应用继续在菜单栏运行。

这是本地开发版本，只有本机使用的 ad hoc 签名，尚未通过 Apple Developer 签名或公证。生成的 ZIP 对应打包电脑的芯片架构，跨电脑分发前需要补充签名、公证和对应架构验证。

## 休息和安全锁屏

- **3D 拉伸跟练**：大休息会自动轮播 8 个离线 3D 拉伸动作，其中 6 个重点照顾肩颈和上背。每个动作展示名称、要领、安全提示和动作进度，也可手动切换前后动作；小休息继续提供简洁的护眼引导。
- **嘴欠的休息伙伴**：小休息从初次提醒、通知、延期、再次提醒与完成五组文案中稳定轮换；语气会逐步变得更“嘴欠”，但不恐吓、不羞辱，也不虚构健康风险。窗口、休息页和 Dock 使用统一的微笑小花图标，花瓣与笑脸融在一块柔和轮廓里。
- **强制休息**：休息开始后遮盖所有连接的显示器，并显示剩余时间。每次小休息可延迟 1 分钟，大休息可延迟 5 分钟，均仅限一次；延迟结束后重新执行完整休息，不再提供延迟按钮。普通退出与程序切换在强制休息期间被禁用，倒计时结束后恢复工作。
- **闲置锁屏**：用于安全保护；根据整个系统的鼠标和键盘闲置时间，在超过 30 秒时请求 macOS 锁定会话。锁定后由 macOS 要求系统密码或 Touch ID 解锁。

3D 人物采用本地打包的 CC0 MakeHuman 人体网格与骨骼，由 Three.js 实时渲染，使用灰白人体、红色区域提示和固定教学视角，运行时不下载远程模型。红色仅为拉伸区域示意，并非精确肌肉解剖。系统开启“减少动态效果”时显示代表性姿势；WebGL 或模型加载失败时保留完整文字指导。动作以舒适为准，如有疼痛或眩晕请立即停止。素材出处见 `src/assets/STRETCH-HUMAN-LICENSE.md`。

进入「舒展身体」并点击「开始这次休息」，即可立即体验大休息跟练。暂停休息时，人物动画也会暂停；继续后从暂停处播放。

这两个计时用途不同：休息时长控制工作间隔，30 秒闲置阈值负责离开电脑后的安全。浏览器预览仅用于查看界面；系统级行为需要运行 Mac 应用。

强制休息由普通 macOS 应用实现。应用可覆盖显示器和拦截常见工作操作，但系统级强制退出、关机或管理员操作仍由 macOS 保留，无法承诺所有系统操作都不可绕过。实现与权限详见 [Rust 版说明](README-rust.md)。

## 本地开发与打包

需要 macOS 14 或更新版本、Rust、Node.js、npm 和 Xcode Command Line Tools。

```sh
npm install
npm run desktop:dev
```

生成可直接打开的 Mac 应用：

```sh
npm run package:mac
```

产物位于 `src-tauri/target/release/bundle/macos/Repose.app`。内部应用标识保持不变，以便升级后继续使用原来的本地设置与系统权限。

```sh
npm test
npm run build
```

`npm run dev` 是浏览器中的界面预览；`npm run desktop` 启动 Tauri 开发版。

## 公开安装包

[GitHub Release v0.6.2](https://github.com/godosou/outsie/releases/tag/v0.6.2) 提供 Apple Silicon / macOS 14+ 的 DMG 与 SHA-256 校验文件。应用名仍为 Repose，当前包包含休息与拉伸功能；手机功能不在此包内。此版本为未经过 Apple 公证的预览版，安装说明见 [发布记录](docs/releases/v0.6.2.md)。

为避免在二进制中保留个人编译路径，公开打包时使用路径映射：

```sh
RUSTFLAGS="--remap-path-prefix=$HOME=/build" CFLAGS="-ffile-prefix-map=$HOME=/build" npm run package:mac:release
```

发布前应检查安装包内容、签名、版本和校验和。打包脚本保留已存在的同版本产物；不要覆盖先前发布的文件。
