# App 快捷操作工作台实现

## 分支与来源

开发分支 `codex/phone-work-console` 基于蓝牙分支 `codex/phone-proximity-unlock` 的 `df70331`，并在 `d9d01a5` 记录 2026-09-08 正在开发的 131 个文件快照。原蓝牙工作目录不变。

## 用户故事

手机选择 tmux、Codex 或飞书后激活 Mac 上的对应 App，并显示该 App 的快捷操作。Mac 可新增、编辑、删除、恢复默认操作，也可录制键盘序列。手机可拖动所有操作按钮，每个 App 的顺序独立保存。只保留 App 切换，不增加工作模式。

## 实现边界

- 保留蓝牙配对代码及其既有 Debug/Release 边界。
- 本版控制使用同局域网 TLS 通道。Mac 显式开启，手机扫码连接，证书指纹固定校验，随机 256 位授权令牌；关闭通道立即撤销，重启需重新扫码。
- 手机只发送配置中 App/操作的 ID，不能上传任意按键或脚本；只有 Mac 主窗口可以编辑操作。
- Mac 激活指定 bundle ID，确认前台仍匹配后逐步发送键盘事件，需辅助功能权限。
- 锁屏、休眠、强制休息、主动停止、心跳超过 3 秒取消剩余步骤。不补发断线请求，不重试执行请求。
- 配置持久化，带 revision 防止手机排序覆盖 Mac 最新编辑。
- tmux 是终端内配置，默认目标 Terminal；用户可改成 iTerm 等 bundle ID。
- 真实手机与蓝牙无线回归需设备在场；模拟器/测试通过不等于真机验收。

## 两端协议 v1

JSON 均 camelCase。Tauri 命令成功直接返回数据，失败 reject 字符串。HTTPS 响应为 `{ok:true,data:...}` 或 `{ok:false,error:"..."}`。

Config: `{revision:number,apps:App[]}`。
App: `{id,name,bundleId,actions:Action[]}`；actions 顺序即手机布局。
Action: `{id,name,icon,kind:"hotkey"|"sequence",steps:Step[]}`。
Step: `{key:string,modifiers:("meta"|"ctrl"|"alt"|"shift")[],delayMs:number}`；delayMs 为该步发送前等待，0–5000；最多20步、每App最多12按钮、最多16App；完整配置紧凑 JSON 上限192KiB。

Status: `{config,enabled:boolean,connected:boolean,running:boolean,activeAppId:string|null,lastError:string|null,accessibility:boolean,blocked:boolean}`。

Mac Tauri 命令：`console_status` -> Status；`console_save {config}` -> Status；`console_reset {appId,revision}` -> Status；`console_start {host}` -> `{qrPayload,status}`；`console_stop` -> Status；`console_run {appId,actionId}` -> Status（已保存操作试运行）；`console_cancel` -> Status；`console_accessibility` -> Status。host 由用户填写 Mac 局域网 IP，默认空，UI 提示系统网络设置查看。

扫码 URI：`repose://console/v1/<base64url(JSON)>`，JSON `{v:1,host,port,token,fingerprint}`。fingerprint 为证书 DER SHA256 小写十六进制。手机 TLS 连接必须只信任此指纹，不能接受其他有效 CA 证书来绕过固定校验。token 不落日志、不持久化到手机。

HTTPS `POST /console`，`Authorization: Bearer <token>`，请求体：
- `{type:"status"}` 心跳+状态，每秒调用；
- `{type:"activate",requestId,appId}`；
- `{type:"execute",requestId,appId,actionId}`；
- `{type:"cancel",requestId}`；
- `{type:"reorder",requestId,appId,revision,actionIds:string[]}`。
所有类型返回 Status。非 status 请求使用唯一 requestId，服务端拒绝重复，无客户端重试。每次开启生成一组临时授权；同一二维码的持有者共享控制权，关闭控制可统一撤销。

## 验证

Rust：配置验证、持久化冲突、执行取消/前台变化/心跳过期、未授权和重复请求；React：快捷键解析、录制与配置编辑；Flutter：扫码校验、TLS固定、请求不重发、排序与操作界面；完整现有测试与构建。
