# Repose Electron 0.1.1 桌面版（基线）

当前 Rust / Tauri 分支请阅读 [Rust 版说明](README-rust.md)。本文件记录 `main` 分支的 Electron 实现。

Repose（歇一会）提供 Electron 桌面运行方式，使用与网页预览相同的界面。关闭主窗口后，应用继续留在菜单栏／系统托盘，休息计时继续运行。需要彻底退出时，选择托盘菜单中的「退出 Repose」。

## 运行

需要 Node.js 和 npm。在项目目录运行：

```sh
npm install
npm run build
npm run desktop
```

开发时运行 `npm run desktop:dev`。启动脚本会在 `127.0.0.1:47832` 启动 Vite，等待服务就绪后打开 Electron。若该地址已有 Repose 的 Vite 开发服务，则直接复用；退出桌面进程时，只关闭脚本自行启动的服务。

首次启动 Electron 需要下载对应操作系统的运行文件。如果下载未完成，请恢复网络后运行 `npx install-electron --no`，再重试。运行 `npm run package:mac` 可生成自带运行环境的 `release/Repose.app`，无需通过命令行启动。当前采用本机 ad hoc 签名，尚未做 Apple Developer 签名或公证，也未包含自动更新和开机启动设置。

## 桌面行为

- 菜单栏／托盘可打开窗口、暂停或继续提醒、立即开始短休息或长休息，以及退出应用；强制休息期间暂停、开始其他休息及普通退出的菜单项禁用。
- 关闭窗口会隐藏窗口；最小化或隐藏期间，渲染进程继续执行计时。
- 默认启用强制休息：休息开始时，每块显示器显示覆盖整个屏幕的休息页，没有跳过、暂停或关闭按钮，倒计时结束自动恢复。每次短休息可以延迟 1 分钟、长休息可以延迟 5 分钟，各限一次。延迟期间恢复工作，随后完整执行同一次休息，不再显示延迟按钮，也不会提前计为休息完成。macOS 使用与 Stretchly 相同的原生 Kiosk 机制，通过系统展示策略禁止 Command-Tab 应用切换、隐藏应用及普通退出；覆盖页只允许延迟按钮所需的 Tab、Enter 和空格键，继续阻止应用快捷键。安全认证由下文的系统锁屏功能负责。
- 一个休息窗口拥有原生全屏 Kiosk，其他显示器使用跨工作区覆盖页。关闭前先退出 Kiosk，等待全屏退出事件后再销毁窗口，恢复系统切换和菜单栏。关闭普通主窗口后，应用继续驻留屏幕顶部的菜单栏。
- 强制休息由主进程维护独立截止时间；即使界面计时停滞，覆盖页也会按时关闭。主界面或覆盖页崩溃时会清理覆盖页。关闭强制休息后，休息提醒恢复为普通窗口提示。
- 延迟操作在所有显示器之间共享一次机会；主进程确认计时器进入同一次休息的延迟状态后，才退出 Kiosk 并移除覆盖页。延迟期间暂停入口禁用，避免无限推迟这次休息；休息安排与延迟使用状态会在重启后恢复。
- 系统通知遵循操作系统的通知权限和勿扰设置；操作系统休眠时应用无法执行提醒。
- 同一用户只运行一个 Repose 实例。再次启动会显示已有窗口。
- 桌面数据保存在 Electron 的 `Repose` 用户数据目录，与浏览器预览数据相互独立。

## 30 秒无操作安全锁屏

安全锁屏与强制休息相互独立。启用后，应用通过系统空闲时间判断鼠标和键盘活动，连续 30 秒没有操作时请求 **macOS 登录会话锁定**。恢复使用需要由 macOS 完成身份验证。首次启动或重新启用时至少保留 30 秒缓冲，避免沿用启动前的空闲时间立即锁屏。

在强制休息期间触发安全锁屏时，应用先恢复系统会话快捷键，保留屏幕覆盖页，再请求 macOS 锁定。锁屏失败会恢复 Kiosk 约束；解锁时若休息尚未结束，会继续剩余休息。

旧版系统存在 `CGSession` 时使用 `CGSession -suspend`。当前 macOS 没有该程序时，通过系统事件发送 Apple 官方的 Control-Command-Q 锁屏快捷键，需要授予 Repose「辅助功能」和相应「自动化 → 系统事件」权限。在偏好设置中可打开系统辅助功能设置。应用会检查系统锁屏事件；命令失败或未确认实际锁定时，界面会显示错误并关闭该开关，授权后可重新启用。

不会用屏幕保护程序、显示器休眠或休息覆盖页冒充安全锁屏。权限尚未授予时，不能承诺自动锁定成功。原生开发验证使用临时目录与模拟锁屏调用，不会为了测试锁定当前用户的会话。

相关文档：[Apple 锁屏快捷键](https://support.apple.com/en-us/102650)、[Electron 系统空闲与锁屏事件](https://www.electronjs.org/docs/latest/api/power-monitor)。

## 前端接口

浏览器预览中 `window.repose` 不存在。Electron 通过隔离的预加载脚本提供以下接口：

```ts
interface ReposeDesktop {
  isDesktop: true
  onCommand(callback: (command: 'toggle-pause' | 'start-short-break' | 'start-long-break' | 'postpone-break' | 'strict-break-finished' | 'idle-lock-failed') => void): () => void
  setStatus(status: { running: boolean; phase: string; remaining: number; breakId: string | null; canPostpone: boolean; postponeSeconds: number }): void
  setPreferences(preferences: { strictBreaks: boolean; idleLockEnabled: boolean; idleLockSeconds: number }): void
  notify(notification: { title: string; body: string }): void
  showBreak(): void
  postponeBreak(): Promise<boolean>
  openSecuritySettings(): void
}
```

`remaining` 以秒为单位；前端在计时状态变化后同步到托盘。`breakId` 在同一次休息和它的延迟期间保持不变；延迟后 `canPostpone` 为 `false`。`postpone-break` 应将当前休息延迟一次，并以相同 `breakId` 的 `focus` 状态确认；`postponeBreak()` 仅在主进程收到确认后返回 `true`。`strict-break-finished` 应将当前休息计为完成；`idle-lock-failed` 应关闭安全锁屏开关并展示授权指引。`onCommand` 返回取消订阅函数，应在组件卸载时调用。主进程会验证消息来源和参数，延迟覆盖页的 IPC 仅接受当前登记休息窗口的主框架和固定本地页面。渲染页面不开放 Node.js、任意 IPC 或任意系统命令执行能力。锁屏只执行固定的系统命令，设置页面只打开固定的系统 URL。

实现参考：[Electron BrowserWindow](https://www.electronjs.org/docs/latest/api/browser-window)、[Tray](https://www.electronjs.org/docs/latest/api/tray)、[contextBridge](https://www.electronjs.org/docs/latest/api/context-bridge)。

Kiosk 原生实现参考：[Electron 44.2.0 的 macOS SetKiosk](https://github.com/electron/electron/blob/v44.2.0/shell/browser/native_window_mac.mm#L1047)、[Stretchly 休息窗口](https://github.com/hovancik/stretchly/blob/trunk/app/main.js#L819)。本机原生验证通过公开 AppKit 接口读取展示策略：强制休息中为 `506`（包含禁止应用切换等标记），结束后恢复为 `0`；验证还覆盖单个 Kiosk 窗口、多屏覆盖、普通退出拦截和锁屏失败后的 Kiosk 恢复。该验证的系统锁屏执行被模拟，未锁定用户实际会话。
