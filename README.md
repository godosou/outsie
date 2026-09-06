# Repose · 歇一会

一款面向 Mac 的休息提醒应用。以柔和的森林绿、简洁的排版与舒展的留白，帮助你在工作中定时休息。

## 打开 Mac 应用

完成打包后，在 Finder 中双击 `release/Repose.app`。应用包含运行环境，日常使用不需要启动浏览器、Node.js 或开发服务器。关闭主窗口后，应用继续在菜单栏运行。

这是本地开发版本，只有本机使用的 ad hoc 签名，尚未通过 Apple Developer 签名或公证。生成的 ZIP 对应打包电脑的芯片架构，跨电脑分发前需要补充签名、公证和对应架构验证。

## 休息和安全锁屏

- **强制休息**：休息开始后遮盖所有连接的显示器，并显示剩余时间。每次小休息可延迟 1 分钟，大休息可延迟 5 分钟，均仅限一次；延迟结束后重新执行完整休息，不再提供延迟按钮。普通退出与程序切换在强制休息期间被禁用，倒计时结束后恢复工作。
- **闲置锁屏**：用于安全保护；根据整个系统的鼠标和键盘闲置时间，在超过 30 秒时请求 macOS 锁定会话。锁定后由 macOS 要求系统密码或 Touch ID 解锁。

这两个计时用途不同：休息时长控制工作间隔，30 秒闲置阈值负责离开电脑后的安全。浏览器预览仅用于查看界面；系统级行为需要运行 Mac 应用。

强制休息由普通 macOS 应用实现。应用可覆盖显示器和拦截常见工作操作，但系统级强制退出、关机或管理员操作仍由 macOS 保留，无法承诺所有系统操作都不可绕过。实际权限、触发行为与桌面接口详见 [桌面版说明](README-desktop.md)。

## 本地开发与打包

需要 macOS 13 或更新版本、Node.js 和 npm。

```sh
npm install
npm run desktop:dev
```

生成可直接打开的 Mac 应用和压缩包：

```sh
npm run package:mac
```

产物位于 `release/Repose.app` 和 `release/Repose-0.1.1-mac-<架构>.zip`。打包脚本只在本项目生成文件，不安装到 `/Applications`，不修改系统权限或开机启动项。

旧版正在运行时，可将新版保存为独立文件，避免覆盖运行中的应用：

```sh
npm run build
node scripts/package-mac.mjs --output-name Repose-0.1.1.app
```

从菜单栏退出旧版后，再打开 `release/Repose-0.1.1.app`，原有设置会继续保留。

```sh
npm test
npm run build
```

`npm run dev` 是浏览器中的界面预览；`npm run desktop` 运行已构建的桌面应用。

打包采用 Electron 官方的 [预构建二进制打包方式](https://www.electronjs.org/docs/latest/tutorial/application-distribution)，将应用资源放入 `.app` 的 `Contents/Resources/app`。桌面主进程位于 `electron/main.cjs`，安全隔离接口位于 `electron/preload.cjs`。
