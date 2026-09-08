# 开源参考

日期：2026-09-08。方法：查阅项目官网、许可证、API 与发布记录；未试装、未评估完整依赖许可证树。

| 项目 | 事实与成熟度证据 | 适用判断 |
|---|---|---|
| Bitfocus Companion | 广播/演示控制工具，有手机或浏览器虚拟按钮、浏览器配置、按下/抬起动作；核心 MIT，有公开发布记录 | 参考按钮、多动作、状态反馈、适配模块；领域不同，不直接作为本项目基础 |
| Hammerspoon | macOS 自动化工具，Lua 调用应用/窗口等能力；MIT，有公开发布记录 | 参考桌面执行能力划分；用户编程门槛需要由产品模板吸收 |

来源：[Companion 产品](https://bitfocus.io/companion)、[MIT](https://bitfocus.io/legal/mit)、[发布记录](https://github.com/bitfocus/companion/releases)；[Hammerspoon](https://www.hammerspoon.org/)、[应用 API](https://www.hammerspoon.org/docs/hs.application.html)、[许可证](https://github.com/Hammerspoon/hammerspoon/blob/master/LICENSE)、[发布记录](https://github.com/Hammerspoon/hammerspoon/releases)。

本次未记录 stars、最近提交间隔或推断维护承诺。公开发布记录表示存在可审查的演进历史，不等于适合直接复用。

## 对我们的启示

<!-- HWPR -->
保留 Repose 的 Tauri / Rust + Flutter 方向，先定义连接、模式、动作适配和反馈的边界。技术设计阶段再评估直接实现与借助外部自动化工具的取舍，本轮不增加第三方运行依赖。
