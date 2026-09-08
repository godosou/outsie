# 手机 App 工作台

手机选择 App 后显示该 App 的快捷操作。Mac 管理配置，手机执行及排列按钮；没有独立的「工作模式」。

## 使用

1. 启动本分支构建的 Repose Mac App，侧栏选择 **App 工作台**。
2. 在系统设置 → 隐私与安全性 → 辅助功能中允许 Repose。该权限只用于用户触发的按键操作。
3. 手机与 Mac 连接同一局域网。在 Mac 工作台填写系统设置 → 网络中显示的局域网 IP，点击 **开启并生成二维码**。
4. 打开 Android Repose App，点击顶部工作台入口，扫码连接。
5. 选择 tmux、Codex 或飞书，点击按钮执行。tmux 需已在对应终端窗口中运行；默认终端是 Terminal，可在 Mac 改为 iTerm 的 `com.googlecode.iterm2`。
6. Mac 编辑 App、按钮名称/图标、组合键、录制序列或手动调整步骤及等待时间，保存后手机自动同步。例如：`⌘K → 等待 2 秒 → Enter → 等待 1 秒 → ⌘V`，每一步可独立调整等待秒数。先保存再试运行；快捷键取决于目标 App 当前版本和自身设置，均可修改。
7. 手机进入 **调整布局**，长按拖动任意按钮（包括键盘序列）或使用移动箭头，完成后保存。布局按 App 独立保存。

tmux 预置采用默认 Ctrl+B 前缀。关闭分屏预置会打开 tmux 自带确认提示，不自动确认关闭。使用不同前缀或希望调整确认步骤时，可在 Mac 修改序列。

录制只捕获工作台录制区域内的按键；Escape、焦点离开或页面隐藏会结束录制。系统拦截的快捷键可手工填写。不录制输入法合成文本。按键按 Mac 当前键盘布局解析。

## 连接与中止

- 本版操作通道是 **局域网 TLS**。蓝牙配对代码已经带入，继续保留其原有 Debug/Release 限制，控制数据尚未通过 BLE 传输。
- 二维码内含本次开启的临时授权和证书指纹。每次重新开启生成新授权；关闭控制或退出 Mac App 会撤销。重启后重新扫码，手机不保存令牌。
- 同一二维码的持有者共享该授权。二维码不是系统解锁凭据；这条通道不能绕过锁屏或强制休息。
- 每步按键前检查目标前台 App、执行取消、锁屏/休眠和强制休息。手机离开工作台或进入后台后断开；心跳超过 3 秒后停止剩余步骤。已发送的按键不可撤回，断线后不补发。
- 手机只发送已保存的 App/操作 ID；不能通过网络新增按键或脚本。
- 每 App 最多 12 按钮，每序列 20 步，每步等待 0–5000ms，最多 16 App，配置上限 192KiB。Mac 编辑和手机排序使用版本号检测冲突。

## 构建

在当前分支工作目录执行：

```sh
npm ci
npm test
npm run build
npx tauri build --debug --bundles app
cd mobile
flutter pub get
flutter test
flutter analyze
flutter build apk --debug
```

Mac 输出：`src-tauri/target/debug/bundle/macos/Repose.app`。
Android 输出：`mobile/build/app/outputs/flutter-apk/app-debug.apk`。

这次交付是开发构建；未完成实体手机 BLE/局域网无线验收、macOS 辅助功能授权后的真实按键验收或 iOS 打包。运行测试和跨语言 TLS 联调使用假键盘，不会向用户应用输入。

设计及协议：[实现方案](plans/2026-09-08-work-console-implementation.md)。
