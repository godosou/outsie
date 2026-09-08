# 手机工作台竞品参考

日期：2026-09-08。方法：搜索并阅读官方功能、联网、应用跟随和多动作文档；未实际安装体验。

| 产品 / 功能 | 客观发现 | 官方来源 |
|---|---|---|
| Stream Deck Mobile | 手机/平板控制电脑，配置、分页和插件；免费 6 键，Pro 最多 64 键，具体价格依地区和渠道 | [产品页](https://www.elgato.com/ww/en/s/stream-deck-mobile) |
| Smart Profiles | 根据前台应用切配置，支持 Mobile；编辑窗口打开时暂停自动切换 | [指南](https://www.elgato.com/uk/en/explorer/products/stream-deck/smart-profiles-stream-deck/) |
| Multi Actions | 单键顺序执行多个动作并可加延时，不支持多动作嵌套 | [说明](https://help.elgato.com/hc/en-us/articles/360027960912-Elgato-Stream-Deck-Multi-Actions) |
| Mobile 联网 | 依赖桌面软件与同一网络；本次不能证明蓝牙支持 | [入门](https://help.elgato.com/hc/en-us/articles/16786832942221-Elgato-Stream-Deck-Mobile-2-0-Getting-Started) |
| Touch Portal | 桌面编辑执行、移动端控制；局域网，USB 说明限 Windows + Android；支持宏、条件与插件 | [FAQ](https://touch-portal.com/faq.php?faqId=how-does-touch-portal-work) |
| Touch Portal 页面 | 页面可按应用或窗口标题自动切换 | [页面指南](https://touch-portal.com/blog/post/tutorials/understanding_section_pages.php) |

未核实这些产品存在经过验证的 tmux / Codex / 飞书原生集成。未核实 Touch Portal 当前具体付费条款，因此不做价格排序。

## 对我们的启示

<!-- HWPR -->
借鉴大按钮、面板、模板和多动作；增加清晰的工作模式概念。主动进入模式打开应用，前台跟随只切手机面板，固定面板用于跨应用工作。宏编辑器显式展示等待、失败位置及执行目标。

<!-- HWPR -->
优势应通过预置模板与可靠目标控制验证。蓝牙连通性、端到端延迟和模板支持程度要分别实测，不能从竞品联网方式推导。
