# 手机靠近自动解锁 macOS：设计文档

日期：2026-09-07
状态：已批准
范围：Repose 的手机靠近解锁能力，不包含计时生命周期优化

## User Story

作为已经登录 Mac 的 Repose 用户，我希望携带已配对的 Android 或 iPhone 离开后再次靠近 Mac 时，系统能够自动解除当前用户的 macOS 锁屏，这样我可以像使用手机车钥匙一样恢复工作，而不必输入密码或在手机上确认。

验收范围：

- 解锁的是 macOS 系统锁屏，不是 Repose 遮罩。
- 仅处理已经登录的本地用户会话锁屏或睡眠唤醒后的锁屏。
- 重启、注销、FileVault 开机登录、快速用户切换、Guest 和远程登录不自动解锁。
- Android 与 iOS 使用同一套 Flutter 伴侣应用界面和协议。
- 日常解锁不要求手机生物识别或点击确认；首次安装、配对和校准可以要求明确操作。
- 系统密码始终可用；任何组件失败都不得阻断正常密码解锁。

## 背景与可行性边界

Repose 当前是一个 Tauri 2 / Rust macOS 菜单栏应用，最低支持 macOS 14。仓库已有系统锁屏能力，但没有手机应用、BLE 配对或系统解锁链路。

macOS 14.6.1 的实机只读检查表明，`system.login.screensaver` 是已登录会话解锁使用的授权规则，并支持第三方授权机制与 `use-login-window-ui` 并存。实现不得修改负责开机与注销登录的 `system.login.console`。

Apple 仍公开 Authorization Plugin API，但插件注册与授权策略包含可能随 macOS 变化的接口。完整产品实现之前必须完成可卸载、可恢复的兼容性原型。若插件无法在无人操作时触发授权、无法安全降级到密码界面，或任何故障可能把用户挡在系统外，项目必须停止在原型阶段。

BLE 加密挑战响应可以证明附近通信方持有已配对手机的私钥；BLE RSSI calibration 只能提供统计意义上的距离判断，不能提供密码学距离约束，也不能完全抵御无线中继。这个残余风险已被接受，首版定位为便利级自动解锁。

## 已评估方案

### 方案 A：Authorization Plugin + Unlock Service + 手机伴侣应用

这是采用的方案。系统插件只消费一次性许可；独立服务负责 BLE、配对、距离状态和密码学验证；手机应用持有硬件保护的密钥。它能解锁真正的 macOS 会话，不保存或模拟输入系统密码，并能保留系统密码 fallback。

代价是需要管理员安装、Developer ID 签名、公证以及跨 macOS 版本验证。

### 方案 B：局域网发现与授权

局域网通信更容易保持，但同一网络不能表示手机接近 Mac，远程转发也更容易，因此不作为距离依据。

### 方案 C：保存密码、模拟输入或使用全屏遮罩

这类方案实现更简单，但会保存高价值凭据，容易被系统更新破坏，或者根本不是真正的 macOS 锁屏，因此明确排除。

## 总体架构

```text
Repose 设置界面（普通用户）
  ├─ 系统组件安装、修复与卸载入口
  ├─ 扫码配对、撤销与距离校准
  └─ 健康状态与诊断
          │
          ▼
Unlock Service / Agent ── BLE ── Flutter 伴侣应用
          │                         ├─ Android/Kotlin 原生后台层
          │                         └─ iOS/Swift 原生后台层
          ▼
一次性、会话绑定的解锁许可
          │
          ▼
最小 Authorization Plugin
          │
          ▼
system.login.screensaver
  ├─ Repose 解锁机制
  └─ use-login-window-ui（始终保留）
```

### Repose 主程序

现有 Tauri/Rust 应用继续以普通用户权限运行。它负责设置、配对、校准和健康状态，不直接作出授权决定，也不保存手机私钥或 Mac 密码。Repose UI 退出后，已经安装的解锁链路仍可工作。

### Authorization Plugin

插件保持最小：获取当前授权会话标识，向 Unlock Service 查询并原子消费一次性许可，然后返回授权结果。插件不运行蓝牙、不解析复杂协议、不显示自定义登录 UI、不访问网络，也不进行长时间等待。

没有匹配许可、服务不可用、响应超时或内部错误时，插件不得批准授权，并必须让保留的 `use-login-window-ui` 继续提供系统密码认证。具体返回语义和最大等待时间由兼容性原型验证后固化。

原型 bundle 只暴露 `unlock` mechanism。后续安装器必须通过命名规则
`ai.repose.unlock` 引用 `ReposeUnlock:unlock,privileged`，不得直接改写
`system.login.console`；本原型构建和测试阶段不读取或修改 authorizationdb。

### Unlock Service

服务负责：

- 维护当前 console UID、审计会话、锁定状态和每次锁屏生成的 `lockEpoch`。
- 保存已配对手机公钥、撤销状态、距离校准参数和防重放计数器。
- 执行 BLE 发现、距离状态机和加密挑战响应。
- 在全部条件满足时生成约三秒有效、只能消费一次的内存许可。
- 通过本地受保护 IPC 为插件提供最小查询接口，并校验调用方代码签名和审计令牌。

服务不得保存、读取或模拟输入用户的 macOS 密码。

首版仅为当前活动的本地用户提供自动解锁。快速用户切换或 console UID 不一致时清除所有许可并拒绝自动解锁。

## Flutter 手机应用

手机伴侣应用使用 Flutter，共享配对、设备管理、校准、状态与诊断界面。安全关键的后台 BLE 和硬件密钥操作由自有 Flutter 插件中的平台原生代码负责，不能依赖 Dart isolate 持续运行。

```text
mobile/
├─ lib/
│  ├─ pairing/
│  ├─ calibration/
│  ├─ devices/
│  └─ protocol_models/
└─ packages/repose_unlock_native/
   ├─ android/  # Kotlin
   └─ ios/      # Swift
```

- Android 首个目标设备是真我 GT5 Pro、Android 16 / API 36。使用 `ObservingDevicePresenceRequest` 与 `CompanionDeviceService.onDevicePresenceEvent()`，并使用 Android Keystore 中不可导出的 P-256 私钥。
- iOS 使用 AccessorySetupKit、Core Bluetooth state restoration 和 Secure Enclave / Keychain。后台恢复事件必须在 Flutter engine 启动前由 Swift 原生层接收。
- 日常签名密钥在手机完成重启后的首次解锁后可供后台使用，不配置每次签名都要求生物识别的访问控制。手机被强制停止、蓝牙关闭或平台拒绝后台恢复时，Mac 安全地退回密码解锁。

Android 优先完成端到端原型，随后保持相同线协议实现 iOS。

## 配对协议

1. Mac 必须处于已解锁的本地用户会话，系统组件健康检查通过。
2. Repose 创建两分钟有效、只能使用一次的随机配对秘密，并显示包含 Mac 身份公钥、服务标识和协议版本的二维码。
3. 手机扫码后在硬件保护的密钥存储中生成 P-256 密钥对。
4. 双方使用临时 P-256 ECDH 建立加密会话，校验二维码携带的高熵秘密，并交换长期公钥和用户可识别的设备信息。
5. 用户在首次配对时确认设备名称。
6. Mac 保存手机公钥、设备标识与初始防重放状态；配对秘密立即销毁。

删除或重新配对设备只能在 Mac 已解锁时执行。密钥丢失、应用重装或计数器状态无法恢复时要求重新配对，不能静默降级认证强度。

## 距离校准

配对后执行一次引导式 calibration：

1. 用户将手机放在日常携带位置，在期望解锁边界（默认约一米）停留约八秒，采集“靠近”分布。
2. 用户携带手机走远，再采集约八秒的“离开”分布。
3. 使用稳健统计量计算 `nearThreshold` 和 `farThreshold`，两者之间保留迟滞区。
4. 若近、远样本严重重叠，校准失败并要求重试，不能生成过宽的范围。

运行时不使用单个 RSSI 样本，而使用二至三秒窗口的中位数或等价稳健滤波。只有持续低于远离阈值或可靠断连后，状态机才进入 `ARMED`；随后持续高于靠近阈值才可发起挑战。

每台手机和 Mac 组合分别保存校准结果。后台自适应只能收紧阈值，放宽范围必须由用户手动重新校准。

## BLE 角色原型

BLE 角色在可行性原型中比较后确定：

- 首选 Mac 作为 central 扫描手机，使决定距离的 RSSI 由可信 Mac 侧采集。
- 若 iOS 后台 peripheral 广播无法达到可靠性要求，则验证 Mac 广播、手机作为 central 后台连接的方向，并结合 AccessorySetupKit / Android Companion Device Presence。

两种方向均需测量后台唤醒、RSSI 可用性、手机锁屏、系统回收与重启行为。若可靠方案不能同时满足“稳定采集距离”和“无人操作触发挑战”，则项目不进入系统授权集成。

## 解锁协议与状态机

```text
UNLOCKED
  └─ macOS 锁屏 → LOCKED_UNARMED（创建 lockEpoch）
       └─ 稳定远离/断连 → LOCKED_ARMED
            └─ 稳定重新靠近 → CHALLENGING
                 ├─ 验证成功 → PERMIT_READY
                 └─ 失败/超时 → LOCKED_ARMED + 冷却
PERMIT_READY
  ├─ 插件消费 → UNLOCKING
  └─ 三秒过期 → LOCKED_ARMED
任意状态收到解锁、用户切换、注销或服务重启 → 清除许可和临时状态
```

每次挑战包含协议版本、Mac ID、手机 ID、`lockEpoch`、双方随机数、单调计数器和短时单调截止时间。双方通过临时 P-256 ECDH、HKDF-SHA256 和 AES-GCM 保护会话；手机的硬件密钥签名覆盖完整握手 transcript。

Unlock Service 验证：

- 当前仍是同一已登录、已锁定的本地 console 会话。
- 手机未撤销，签名与 transcript 完整有效。
- `lockEpoch`、随机数和单调计数器从未使用。
- 距离状态满足本次锁屏后的“离开再靠近”。
- 挑战仍在短时单调截止时间内。

许可绑定 console UID、审计会话和 `lockEpoch`，只保存在内存中，约三秒后失效并只能原子消费一次。解锁完成或任何会话变化立即销毁全部许可。

## 安装、升级、修复与卸载

系统组件只由独立、签名的管理员安装工具修改。流程采用两阶段事务：

1. 检查系统版本、签名、公证状态、现有授权规则结构和冲突。
2. 保存 `system.login.screensaver` 的结构化备份及校验值。
3. 安装 Unlock Service 和 Authorization Plugin，验证文件所有者、权限、签名及 IPC 健康状态。
4. 最后将 Repose 机制合并到现有授权规则，保留 `use-login-window-ui` 和其他第三方机制。
5. 立即回读并结构化比较结果；只有在从最新 live rule 精准移除 Repose candidate、回读证明密码 fallback 仍唯一且无并发漂移后，才允许恢复旧组件或移除本次组件。若 Authorization Services 返回结果不明确、live rule 无法安全解析、rollback 回读失败或发现外部写入，则保持 Repose 路径禁用并保留其依赖，进入显式 `repair`，绝不盲写旧 preimage 覆盖第三方变化。

安装、升级、修复和卸载都由 root-owned `O_NOFOLLOW` 单写者锁与 fsync phase journal 串行化。升级先移除并回读 screensaver candidate，再替换同一 generation 的组件并验证 loaded image/deny-only closure；持久化新 receipt 后必须把返回的 target fingerprint 写入 journal，后续每次 closure 验证都精确匹配该 target，而不是接受任意 `Trusted` generation；named rule 就绪后才最后恢复 candidate。卸载严格反向：先精准移除并回读 candidate，再验证并移除 Repose 自己的 named rule；既存 prior fingerprint 必须在每个 bootout/quarantine 原子操作内部重验，外层 stale read 不能授权停载或删除新 generation；receipt 最后删除。任一 policy 步失败都保留全部依赖。断电恢复根据 durable phase、prior/target receipt fingerprint 与最新 live 状态完成已经激活且闭包完整的事务，或安全停在 password-only；缺少 target marker、fingerprint 不同以及修复的 pre-terminal/abort-marker 歧义一律保守停用，不使用 stale backup 模拟 CAS。

Task 7 的 unsigned/ad-hoc 包只用于静态 plan。包内 `SHA256SUMS` 只能证明自洽，不能证明真实性；通用 verifier 不执行包中的 helper。所有 Production mutation（install/uninstall/repair）在 backend、lock、journal、Authorization Services、launchd 或目标文件访问之前硬关闭，直到 Task 8 固定 Developer ID/designated requirement、整体签名 manifest、plugin/service signing ID、deny-only build measurement、协议/package generation、最低 OS/arch、防降级规则，并实现 fd-relative sealed staging 及 ACL/xattr/file-flags 校验。`--health-check-deny-only` 仅是受信 installed measurement 后的 liveness sanity，不是 attestation。

Authorization Services 不提供 compare-and-swap。最后时刻读取、精准结构变换与结构化回读只能检测可观察到的漂移，不能保证另一 privileged writer 不会在 read 与 `AuthorizationRightSet/Remove` 之间写入。Task 8 开闸必须在专用维护窗口中进行并提供独占的管理员操作协调；文档和状态输出不得把本地 installer lock 描述成对第三方 writer 的原子互斥。Task 8 还必须为 production filesystem/service adapter 增加 operation×syscall 故障注入，覆盖短写、rename、file/dir fsync、`EXDEV`、launchctl timeout/permission/not-found、quarantine 与 receipt replace/delete 的断电切点；在此之前 gate 保持关闭。

正常升级和卸载只精准修改 Repose 自己的机制，不用旧备份覆盖其他软件后续变更。仅当当前规则损坏且安全验证允许时，修复工具才可使用备份恢复。

提供可独立运行的 `repose-unlockctl repair` 与 `repose-unlockctl uninstall`。系统更新后若发现组件缺失、签名异常或规则变化，自动解锁保持禁用并提示管理员修复；不得静默重写系统授权策略。

## 失败处理与安全属性

- 插件、服务、BLE、手机、Flutter UI 或 Repose 主程序任一不可用时，系统密码仍然可用。
- 所有解析器都使用严格长度限制、版本检查和拒绝未知字段的策略。
- IPC 使用操作系统审计令牌、代码签名要求和 root 所有的端点约束调用者。
- 重放旧挑战、旧计数器、旧 `lockEpoch` 或旧许可一律失败。
- 手机一直位于桌边时，锁屏后不会立即解锁。
- 不向云端发送配对密钥、距离样本或解锁事件；诊断日志不得包含密钥、完整随机数或可重放消息。
- BLE relay 和已解锁手机被盗后靠近 Mac 是已知残余风险，必须在设置界面中说明。

## 分阶段实施与验证门槛

### 阶段 0：纯逻辑与安装夹具

- 以测试先行实现校准算法、状态机、协议编码、挑战验证和一次性许可存储。
- 在临时 plist 夹具上测试授权规则合并、精准卸载和回滚，不修改真实系统。
- 使用固定密码学测试向量在 Rust、Kotlin、Swift 与 Dart 之间验证兼容性。

### 阶段 1：Android BLE 原型

- 在 GT5 Pro / Android 16 与已解锁 Mac 之间验证配对、硬件密钥、后台 BLE、RSSI calibration 和挑战响应。
- 覆盖屏幕关闭、Flutter UI 退出、系统回收、蓝牙切换、手机重启与 realme 后台限制。
- 此阶段不得接入真实 macOS 解锁。

### 阶段 2：macOS 授权原型

- 先用模拟许可验证 Authorization Plugin 是否能在无人操作时参与已登录会话解锁。
- 在可恢复测试环境覆盖 macOS 14.6.1、15 和 26。
- 注入服务停止、插件崩溃、插件缺失、IPC 超时、签名错误、规则冲突和卸载中断。
- 任一故障可能阻断或显著延迟密码认证时，停止项目，不进入完整集成。

### 阶段 3：iOS BLE 原型

- 验证前台、后台、挂起、系统回收、强制退出、蓝牙关闭、重启后首次解锁前后。
- 验证 iOS 26 AccessorySetupKit 与 Core Bluetooth state restoration 的真实行为。

### 阶段 4：端到端集成

- 执行真实“锁屏 → 离开 → 靠近 → 解锁”。
- Android 正常后台状态下，进入稳定靠近范围后的目标解锁时间不超过三秒。
- 验证手机留在桌边、范围边界抖动、挑战重放、伪造设备、旧会话和普通本地进程访问。
- 验证重启、注销、FileVault、快速用户切换绝不自动解锁。
- 验证卸载后保留其他授权机制并恢复原有密码体验。

## 测试策略

- Rust 单元测试：状态机、校准统计、协议模型、反重放、许可原子消费和授权规则变换。
- 属性测试：任意事件序列下，未完成有效挑战绝不产生许可；同一许可最多成功消费一次。
- 模糊测试：BLE 帧、IPC 请求和持久化记录解析器。
- 跨语言契约测试：共享二进制 fixture 和签名测试向量。
- Android instrumentation 测试：Companion Device 生命周期、Keystore、进程恢复和 Flutter 原生桥。
- iOS XCTest：Core Bluetooth 恢复、Keychain/Secure Enclave 与 Flutter 原生桥。
- macOS 集成测试：代码签名校验、launchd 生命周期、授权插件超时和失败注入。
- 手工破坏性测试只在具备密码 fallback、外部管理员恢复手段和完整备份的测试 Mac 上运行。

## 完成标准

只有同时满足以下条件才称为完成：

- Android 与 iOS 在平台允许的后台状态下能完成已配对挑战响应。
- 距离状态必须经历锁屏后的稳定远离和稳定重新靠近。
- 没有有效手机挑战时，任何代码路径都不能产生解锁许可。
- 所有注入故障均保留可用且无明显额外延迟的系统密码界面。
- 安装、升级、修复和卸载不会删除或覆盖其他授权机制。
- 支持矩阵、后台限制、残余风险和恢复方式均有用户文档。
- 设计、实施计划、测试和构建命令均进入仓库，且计时生命周期代码未被修改。
