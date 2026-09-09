# 技术与应用集成调研

日期：2026-09-08。方法：阅读官方 API/命令说明及项目分支验证记录。这里归纳与本产品有关的技术方向，不声称代表行业趋势统计。

## 客观发现

- tmux 支持 `split-window -h/-v`、`select-pane`、`kill-pane` 和 zoom；命令可明确指定目标。左右/上下分屏与关闭/放大应分别定义。[官方入门](https://github.com/tmux/tmux/wiki/Getting-Started)、[手册](https://man.openbsd.org/tmux.1)
- Codex 官方命令页当前重定向到统一桌面应用文档；列出打开已有任务、创建任务并预填路径/提示词的链接，以及可自定义快捷键。预填提示词不自动发送。[官方文档](https://developers.openai.com/codex/app/commands/)
- 飞书有 AppLink 文档入口，但本轮抓取正文为空，不能据此验证具体路由。官方另有飞书项目快捷键清单，该模块不能等同于飞书桌面聊天客户端。[AppLink](https://open.feishu.cn/document/common-capabilities/applink-protocol/applink-introduction/applink-application)、[飞书项目快捷键](https://www.feishu.cn/content/5l84vlik)
- Hammerspoon 展示了按 bundle ID 启动/激活应用及读取前台应用的 Mac 自动化接口。[应用 API](https://www.hammerspoon.org/docs/hs.application.html)
- iOS Core Bluetooth 后台扫描与广播受到约束；后台模式不等于任意常驻。Android 也区分扫描、连接与保持连接的后台策略。[Apple](https://developer.apple.com/library/archive/documentation/NetworkingInternetWeb/Conceptual/CoreBluetooth_concepts/CoreBluetoothBackgroundProcessingForIOSApps/PerformingTasksWhileYourAppIsInTheBackground.html)、[Android](https://developer.android.com/develop/connectivity/bluetooth/ble/background)
- 蓝牙分支 `c8caa60` 的 `docs/validation/verification-summary.md` 明确记录真实端到端尚未完成、生产角色关闭。已有手机 UI 与协议原型不能外推为可用控制链路。

## 对我们的启示

<!-- HWPR -->
按“直接命令/公开链接优先，已验证快捷键补充”的顺序适配。tmux 命令绑定具体窗格；Codex 先做导航与草稿；飞书先做已保存链接与经用户验证的快捷键。

<!-- HWPR -->
首版采用手机前台交互，显示连接和执行结果。Mac 接收已配置动作的标识与受约束参数，不能把任意远程脚本文本当作协议。断连不补发过期动作，回执未知先查询而非重试。

待实测：BLE 角色、延迟与重连；终端定位 tmux 会话；用户已改快捷键；Codex 安装版本；飞书链接打开位置；Mac 锁屏、休息和睡眠交互。
