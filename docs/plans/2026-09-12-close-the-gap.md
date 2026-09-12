# 按设计文档收口：差距分析与计划

2026-09-12 · 设计文档：`docs/design/phone-key.html`（章节号以它为准）

## 差距

代码是这一晚对着聊天消息改出来的，设计文档是最后定稿的。两者相反的地方列在这里，
每一条都指向文档的章节。**从这一份开始，代码只对着文档改。**

| # | 文档 | 代码现状 | 要改 | 怎么验 |
|---|------|----------|------|--------|
| B1 | §04 §08 目录按 Mac 存 | 一份，带 keyId 字段 | `ConsoleCatalogue.save/load` 按 keyId 分槽 | 单元：槽键函数；真机：两台各一份 |
| B2 | §10 整理按 Mac 存 | 一份 | `ConsoleArrangement.load/save` 按 keyId | 单元 |
| B3 | §04 没有那台的目录 → 「还没同步过」 | 有哪份显示哪份 | 控制页按目标 keyId 取 | 真机 |
| B4 | §08 收到后按签它的钥匙归档 | 覆盖那一份 | `ConsoleServer.accept` 存到 `parsed.keyId` | 单元（verify 已返回 keyId） |
| C1 | §04 一格一个按钮，三列 | 一行一个 | 控制页网格 | 真机 |
| C2 | §10 整理：长按拖动、× 收起、恢复，都在控制页 | 独立 ArrangeScreen，▲▼ | 删 ArrangeScreen / `Screen.ARRANGE`；控制页编辑态；`View.startDragAndDrop` | 单元：顺序整份写下；真机 |
| C3 | §10 只有操作级别 | 有 App 级 hiddenApps / appOrder | 删这两个字段及其测试 | 单元 |
| A1 | §03 手机没有 tab 栏 | 两个 tab | 删 bottomNav | 真机 |
| A2 | §04 控制属于某台电脑 | 全局 CONTROL | 卡片「控制」带 keyId 进入，标题 = 那台电脑 | 真机 |
| A3 | §03 「这把钥匙」删掉；解除配对、手机丢了 进主屏「更多」 | MacsScreen + 链接 | 删 MacsScreen / `Screen.MACS`；HomeScreen 折叠 | 真机 |
| A4 | §04 卡片上「重新量距离」 | 无 | 加链接；E 做完前指向说明 | 真机 |
| D1 | §04 Mac 手机行只写时间地点；数字进「更多」 | 每行「量一下距离」；数字在校准弹窗 | 行改文案；数字进「更多」；按钮保留到 E | TS 单元 + 目视 |
| D2 | §01 表面无 keyId / hex | `macLabel` 「Mac %04X」 | 「一台 Mac」 | 单元 |
| D3 | §01 文案风格 | 旧措辞 | 两端字符串按 §01 改 | 目视 |
| D4 | §04 权限行按官网口气介绍手机工作台 | 「替你按键的权限」 | 改文案 | 目视 |
| E1 | §14 cmd 4 / 5 | 无 | `SpikeContract` + `presence-verify` 接受 | 协议自检向量 |
| E2 | §14 状态字节 2–5 | 只有锁/开 | `state-advertise` + `MacState` 解析 | 单元 + 向量 |
| E3 | §05 §06 手机四屏 | 无 | `CalScreen.kt` | 真机走动 |
| E4 | §06 Mac 侧只显示结果 | CalibrationSheet 驱动全流程 | watcher 收 cmd 4/5 驱动 calibrate_*；弹窗降级为「在手机上量」 | 真机走动 |

## 顺序

**B → C → A → D → E。** B 是 A2 和 C 的前提；C 删掉的页面 A 顺手收尾；D 是 A–C
之后的一次文案扫除；E 单独一节，因为它碰协议、碰无线电，而且量错了会连累解锁。
E 在 A–D 上真机看过之后再动。

## 每一步的做法

1. 先写测试，能在 JVM 上跑的都在 JVM 上跑（`gradlew testGenuineDebugUnitTest`、
   `npm test`、`cargo test --lib`）。SharedPreferences 不进单元测试，把槽键和整理逻辑
   留成纯函数。
2. 改完装真机，截图对照文档那一章的帧。
3. 一步一个 commit，commit 说明引用文档章节。

## 不在这份计划里

- 官网承诺的「面板跟随前台 App」和「耳机语音输入」：文档 §15 已记为差距，等产品决定
  是改官网还是排进下一版协议。
- 严格模式：不做。
