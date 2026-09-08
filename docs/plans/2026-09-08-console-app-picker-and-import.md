# 本机 App 选择与可导入快捷操作

用户在 Mac 上配置手机按钮，不应查找 Bundle ID。点「添加 App」或「更换 App」后，可以搜索本机应用名称、查看图标和安装路径，也可以从系统文件选择器选择其他位置的 `.app`。选择后自动保存目标标识和实际路径。同一应用安装多个版本时，执行器匹配选中的路径。恢复默认操作保留所选应用和手机显示名称。

## Codex 默认操作

2026-09-08 核对 [OpenAI 官方命令参考](https://learn.chatgpt.com/docs/reference/commands) 的 macOS 列表。77 个按钮覆盖 Codex 适用的、有普通键盘默认绑定的功能，并保留既有「粘贴」。每个功能使用一个官方快捷键组合；数字范围分别展开成按钮。包括通用导航、任务切换、模型、语音、终端、文件、审查、浏览器、复制信息。

- 未修改的旧三按钮预置启动时自动扩充；已有自定义操作不被覆盖，可点「补齐 Codex 默认操作」将缺项加入草稿，再保存。
- 单个 App 上限提高到 96 个操作；整体仍限制 16 个 App、192 KiB、每操作 20 步、每步等待 0–5 秒。
- 按钮名称标注焦点或功能可用性前提。快捷键是发送给目标 App 的键盘事件，不能保证自动定位其内部控件；例如清空终端前需先聚焦终端。
- 搜索历史对话没有官方默认键，不编造绑定；用户可在 Codex 分配后自定义。
- Appshot 的左右 Command 同时按下暂不支持。ChatGPT 专属的快速/临时聊天不属于 Codex 预置；按住说话、鼠标和仅修饰键的手势也不在本期序列模型内。
- 用户在 Codex 中重映射按键后，需同步更新 Repose 配置。旧版本 Codex 的绑定可能与当前官方列表不同。
- 数据源：`src/lib/codexPresets.json`，Mac Rust 默认与配置界面共用，防止两份预置不一致。

## 导入与 AI 生成

Mac 工作台提供「导入配置文件」和「让 AI 生成配置」。后者显示可复制的生成说明，包含 JSON 示例、允许按键和字段、容量限制及等待语义。用户替换 App 名称和需求，让 AI 生成 `.json` 文件。

导入仅新增草稿配置，不覆盖现有 App、不执行按键、不自动保存。每个导入 App 必须交互选择本机实际应用，然后检查操作并保存。取消编辑丢弃导入。手机产生更新时，保留现有版本冲突保护。

可直接使用 [Safari 示例](../examples/safari-console.json) 验证，第二个操作是「⌘F → 等待 2 秒 → Escape」。文件格式：

```json
{"schemaVersion":1,"apps":[{"name":"Safari","actions":[{"name":"新建标签","icon":"+","kind":"hotkey","steps":[{"key":"t","modifiers":["meta"],"delayMs":0}]}]}]}
```

只接受上述字段。`icon` 可省略，默认 `⌘`；其他字段必填。内部 ID 在导入时生成，不接受 Bundle ID、安装路径、脚本和命令。整个文件先完整校验，失败不产生部分导入。每个 App 1–96 个操作，名称 1–64 字符，图标 1–16 字符；`kind` 只能是 `hotkey` 或 `sequence`。`delayMs` 为该步按下前的等待毫秒数，必须是 0–5000 整数。

`key` 支持一个 ASCII 可见字符、Enter、Tab、Space、Escape、Backspace、Delete、ArrowLeft、ArrowRight、ArrowUp、ArrowDown、Home、End、PageUp、PageDown、F1–F20。`modifiers` 为 meta、ctrl、alt、shift 的无重复数组。单键操作恰好一步，连续序列 1–20 步。

Bundle ID 和安装路径是内部持久化字段；便携导入文件独立使用 `schemaVersion: 1`，不是运行中 `work-console-v1.json` 的直接备份恢复格式。
