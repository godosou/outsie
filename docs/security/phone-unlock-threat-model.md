# 手机靠近解锁威胁模型

日期：2026-09-08

状态：**原型威胁模型；生产门禁关闭**

## 安全目标

系统只应在同一已登录本地会话、本次锁屏后确实经历稳定离开—返回、并且已配对手机完成有效
密码学挑战时，产生一次短时、单次消费的解锁许可。任何解析、存储、时钟、蓝牙、手机、服务、
插件或授权策略故障都不得批准解锁，也不得破坏系统密码 fallback。

系统不以 BLE RSSI 代替身份认证，不保存 macOS 密码，不自动处理 FileVault、重启、注销、
Guest、快速用户切换或远程登录。

## 受保护资产

- macOS 已登录会话与 `system.login.screensaver` 授权策略；
- Mac 和手机的长期 P-256 身份私钥；
- 已配对身份、公钥、配对代次、撤销 tombstone 与单调防重放状态；
- 当前 console UID、audit session、`lockEpoch` 和一次性内存许可；
- 用户密码、FileVault 恢复密钥以及修复/安装信任材料；
- 诊断数据中可能泄露位置、设备或重放信息的字段。

## 信任边界

| 边界 | 信任假设 | 失败策略 |
|---|---|---|
| macOS Authorization Plugin | 只消费本地服务签发的一次性许可；不做 BLE 或复杂解析 | 超时、拒绝或崩溃都转入系统密码候选 |
| Unlock Service | 校验会话、距离状态、协议、重放与许可；生产端点只接受预期系统调用方 | 状态不一致即清除许可并拒绝 |
| Android Keystore / Apple 密钥存储 | 长期私钥不可导出，后台可用性由平台能力决定 | 密钥缺失或不可用要求重新配对/密码 fallback |
| 手机应用 UID | 同一签名 APK、同一 UID 的原生代码属于可信计算基 | 不把 AAR/Pigeon 封装描述成同 UID 强隔离 |
| BLE 无线链路 | 完全不可信，可被窃听、丢包、重排、重放、阻断或中继 | 固定上限解析、双向身份签名、ECDH/AEAD、防重放；阻断只导致拒绝 |
| 本地普通进程 | 不可信，可能连接 IPC、重放帧或竞态调用 | 固定 socket、peer identity、审计 token、代码签名与一次性消费 |
| root / 内核 / 已攻陷同 UID 代码 | 不在本原型可抵御范围 | 不作安全保证，需操作系统恢复/撤销 |

## 核心契约中已覆盖的威胁与设计控制

本节描述规范、纯逻辑实现和自动化测试所要求的控制，不表示每个生产 adapter 已接入。当前
macOS 服务生产入口仍是经过 peer 验证的 deny-only/offline processor，没有连接 durable replay
runtime 或许可 broker；Android 持久化 responder 已有固定向量、CAS 与编译证据，并在 RMX3888
上通过测试专用 UID 的 5/5 Keystore/SQLite instrumentation。production app UID、Companion
Presence 与 GATT 尚未执行。真实系统、无线链路和平台后台行为仍受文末发布门禁约束。

### 伪造手机或 Mac

挑战由已配对 Mac 身份密钥签名，响应由已配对手机身份密钥签名。验证密钥只能来自本地可信
配对记录，不能由传入帧选择。临时 P-256 ECDH、HKDF-SHA256 与 AES-256-GCM 绑定完整挑战、
响应前缀和方向。原始签名必须是规范的 64 字节 low-S 格式，公钥必须是规范且在 P-256 曲线
上的 SEC1 点。

### 重放、乱序与跨会话复用

固定帧严格检查 magic、版本、kind、reserved、精确长度与尾随字节。挑战和响应绑定 Mac ID、
设备 ID、配对代次、console UID、audit session、`lockEpoch`、ChallengeId、nonce 和计数器。
手机端契约要求持久化响应缓存让同一挑战精确重传同一响应；不同挑战必须以原子 CAS 分配
更新计数器。Rust core 的 durable store 抽象、撤销 tombstone 和一次性许可测试模型阻止旧
代次、旧锁屏或重复消费；生产 macOS durable adapter/runtime 尚未接入服务入口。

### 蓝牙分片与生命周期混淆

GATT 分片只是固定上限的外层传输信封，不参与授权语义。内层只接受精确的 v1 Challenge 或
Response。连接、association、配对代次、runtime 或 operation token 改变时丢弃部分帧；间隙、
重叠、总长度变化、冲突重复、越界和旧回调均失败。重连只能重传同一完整挑战，不能触发手机
重新签名或增加计数器。

### 本地 IPC 冒充与许可竞态

插件—服务 IPC 使用固定 84 字节协议、绝对截止时间、半关闭/EOF 约束、请求 nonce、完整会话
绑定、服务实例和 watch ID 关联。服务端在解析前验证 peer；核心 broker 模型把许可、状态机
和 durable authority 在固定锁序中线性化。许可约三秒过期、绑定当前会话、只能原子消费一次，
服务重启或会话变化即失效。普通生产入口目前只验证并拒绝请求，不会签发或消费许可。

### 授权策略安装损坏

设计只操作 `system.login.screensaver` 与 Repose 自己的 named rule，明确禁止
`system.login.console`。安装策略最后启用，卸载策略最先禁用；所有阶段记录 journal，回读结构
并保留第三方候选和 `use-login-window-ui`。Authorization Services 不提供 CAS，因此即使有
本地单写者锁，也不声称能原子排除另一个 privileged writer；生产开闸必须使用专用维护窗口
和独占管理员协调。

### 故障导致用户被锁在外面

自动路径是额外候选，不取代系统密码。没有许可、服务离线、手机 force-stop、蓝牙关闭、
Keystore 不可用、超时或异常均应拒绝自动路径。插件不等待长时 BLE 操作。真实密码 fallback
延迟、登录 keychain、断电恢复、repair/uninstall 必须在可恢复专用 Mac 上通过后才可安装。

## 残余风险与明确不保证项

### BLE 中继与 RSSI 局限

RSSI 是受环境影响的统计信号，不是精确距离，也没有飞行时间或超宽带的距离约束。攻击者可能
中继合法手机的无线交互，使远处手机看起来“接近”。密码学能证明持钥者参与，不能证明无线
路径没有被延长。首版只定位为便利级解锁；高风险环境应关闭自动解锁并使用密码。

### 已解锁手机被盗

日常响应不要求每次生物识别，因此持有一台已经可供后台使用密钥的被盗手机并靠近 Mac，可能
满足手机侧条件。设备锁、快速在 Mac 上撤销配对和密码 fallback 是补偿控制。

### 同 UID / 高权限攻陷

Android Keystore alias 对应用 UID 隔离，不对同 UID 内的恶意原生依赖提供额外边界。Java
carrier 的非公开构造与窄 Pigeon API 只减少普通跨包误用面。若要抵御同 UID 供应链代码，需要
把密钥操作迁入独立 UID 的受保护服务。root、内核、系统框架或授权插件进程被攻陷同样超出
本原型范围。

### Android 密码 provider 的内存擦除边界

手机响应器会在成功和错误路径清零自己持有的 ECDH 结果、明文、HKDF/AEAD key、nonce 与
中间 `ByteArray`，并把 Keystore 临时私钥限制在有上限、跨进程租约保护的 alias 槽中。但是
Android/JCA `Cipher`、`Mac` 和 `SecretKeySpec` 可能在 provider 内部复制 key 或展开 key
schedule；公开 API 不能可靠证明这些不透明副本已立即擦除。因此这里只承诺显式拥有数组的
擦除和最小 provider 对象作用域，不声称 Java 运行时内部状态可验证清零。生产 BLE 角色仍为
Disabled，此限制必须与真机 provider/内存策略一起接受或消除后才能修改发布安全声明。

### 可用性与拒绝服务

无线干扰、手机系统后台限制、强行停止、低电、蓝牙关闭、进程回收或平台升级都能阻止自动
响应。设计接受这种拒绝服务，并必须无条件退回系统密码，不以降低验证强度换取可用性。

### 尚未验证的平台行为

RMX3888 上的基础密钥/存储 primitive 已有部分真机证据，但 GT5 Pro/realme UI 7 的完整后台与
功耗矩阵、iPhone Core Bluetooth 恢复、macOS
AuthorizationHost 真实行为以及跨 macOS 版本兼容性均未完成。自动化夹具、模拟 adapter、
ad-hoc 签名或成功编译不能替代这些证据。

## 安全日志规则

日志可以包含协议版本、阶段、有限错误码、组件健康、脱敏设备标签、计数器是否前进以及延迟
分桶。不得包含密码、私钥、配对秘密、ECDH 原始秘密、HKDF/AEAD 密钥、完整 nonce、完整帧、
完整签名、可重放 transcript、恢复密钥或原始 QR 内容。错误消息必须保持有限枚举，避免把
底层异常或密钥材料穿过 Pigeon/UI 边界。

当前没有生产日志 sink、日志文件、保留期限或导出流程；`open_unlock_diagnostics` 只返回
gate-closed 的脱敏状态。未来日志落点、所有者/权限、轮换、retention 和用户导出前脱敏都必须
独立评审并加入发布门禁。

## 发布阻断条件

出现以下任一情况都维持 **GATE CLOSED**：

- 密码 fallback、登录 keychain 或无人操作授权重评估未在专用 Mac 通过；
- Developer ID、notarization、组件 designated requirement 或 sealed staging 未固定；
- GT5 Pro 真机未证明所选 BLE 角色、后台可靠性、误判、延迟和电量；
- iOS 未在受支持 Xcode/iOS SDK 与物理 iPhone 上验证；
- 安装、升级、repair、uninstall、故障注入或断电恢复有未通过项；
- 任一生产构建仍能绕过失败关闭、持久化/撤销线性化或密码 fallback。
