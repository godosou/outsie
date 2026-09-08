# 工作台改回蓝牙控制

用户确认控制必须使用蓝牙。局域网通道从产品入口移除，沿用手机钥匙配对二维码和系统关联；无需 IP 或第二个二维码。开发在 codex/phone-work-console 继续。

## 共享协议（实现依据）

Mac 同一个 CoreBluetooth peripheral，同一服务 A53E0001-7A6B-4D59-9F2E-5245504F5345，保留配对 control0002/status0003，增加 console RX0004(WRITE)、TX0005(NOTIFY)、challenge0006(READ)。完整UUID都同后缀。console开启期间停止配对不可删除这三个控制特征/断连；更新配对状态不重复发布已发布服务。

手机必须已完成原有 Android 系统关联和手机内配对确认，Mac 也必须完成配对确认。Mac 在确认时从原始 pairing QR 解码 sessionId16 + pairingSecret32 并授权；Android 原生确认时存同一session/secret及associationId。原生内存保存，撤销配对删除，进程重启需重新配对（与当前 Debug配对底座一致）。不将密钥传给Flutter，不通过明文 BLE 发送密钥。

### 分片

RX/TX 每个ATT value格式：`0xC1 | messageId:u32BE | index:u16BE | count:u16BE | payload`，头9字节，index0开始，count>=1。首片index0重置当前接收；后续严格同ID/count且连续；总加密包上限256KiB，最多24000片；同一连接只有一个在途RPC；超时30秒关闭连接不重发。Android write-with-response顺序发包，ATT MTU请求517，实际单片上限min(MTU-3,512)；Mac通知使用central.maximumUpdateValueLength并尊重updateValue背压。

手机先订阅TX然后读取challenge。Mac订阅时为central随机生成challenge16；重新订阅/断开/关闭控制使旧challenge失效并取消操作。只允许一个console central订阅，第二个拒绝读写控制数据，不替换第一连接。

### 加密包

`version:u8=1 | direction:u8 (0手机->Mac,1Mac->手机) | sessionId:16 | challenge:16 | counter:u64BE | ciphertext+GCMtag16`，header42字节同时作为AAD。

key=HKDF-SHA256(ikm=pairingSecret32, salt=challenge16, info=UTF8("repose-console-ble-v1"), len32)。nonce=`direction:u8 | 0,0,0 | counter:u64BE`。AES-256-GCM。每方向counter从1严格递增，不能重放/跳号；错误认证不消耗接收计数，断连重建challenge。Mac按session查找已授权secret，检查challenge当前有效，首次成功解密后绑定该session，直到断开。任何错误只能返回ATT失败/断开，不执行请求。响应counter独立递增。

plaintext UTF8 JSON：请求沿用work-console implementation文档 status/activate/execute/cancel/reorder 和requestId；新增可选knownRevision数字用于减少BLE带宽。响应仍{ok:true,data:Status}或{ok:false,error:string}；knownRevision等于当前配置则data省略config；手机原生/Dart客户端用上次config补全。首次status必须带完整config。心跳每秒，执行lease维持3秒；请求无自动重试。

## Rust/native接口

native导出：`repose_ble_console_start(callback)` -> bool；`repose_ble_console_stop()`；`repose_ble_console_send(centralUtf8, bytes, len)` -> bool；`repose_ble_console_revoke(centralUtf8, expectedChallenge, len)` -> bool。revoke仅接受16字节challenge，在主队列内同时检查central和当前challenge再清除连接；旧worker不能撤销同central的新订阅。send拒绝零长度；非空响应也校验header中的当前challenge。CoreBluetooth peripheral无法主动物理断开central，逻辑撤销后拒绝读写，手机需重新订阅获取新challenge。

callback签名 `void(const char *central, int event, const uint8_t *data, size_t len)`。event1=subscribe带challenge16；event2=完整密文；event3=unsubscribe/radiooff；event4=radioState，central为空字符串、data为1字节枚举：0未知、1服务已发布且广播中、2蓝牙关闭、3未授权、4不支持、5发布/广播失败、6正在发布。start、radio变化和广播回调时发出。回调不阻塞主队列，Rust复制数据后入队处理。native组装分片，限制单central，TX排队背压。native send发异步通知，返回是否受理。不能持有Rust工作台state锁调用同步主队列方法。

Rust WorkConsole transport替换Bluetooth adapter，不启用TLS。start命令不再需要host，返回Status。status增加transport:"bluetooth"及pairedDevices:[{id,name}]、bluetoothReady、bluetoothState（unknown/ready/poweredOff/unauthorized/unsupported/failed/starting）。Mac配对确认授权桥由root实现。

## Android/Flutter接口

MethodChannel `ai.repose/work_console_ble`：
- `devices` -> List<{id:String,name:String}>（已确认并有密钥的Mac）
- `connect` {deviceId:String} -> null，完成GATT发现/订阅/读取challenge才成功
- `request` {message:String(JSON)} -> String(JSON完整响应envelope)
- `disconnect` -> null
Flutter ConsoleTransport保持request(map)->Status、close接口，改为MethodChannel BLE实现。选择原生devices中的已配对Mac连接，无新QR，无IP。控制页暂无配对时引导返回手机钥匙完成配对。旧LAN客户端不再用于产品路径，可删除TLS相关测试并换BLE mock通道测试。Android原生密钥/加密在native，Flutter仅持JSON和选中MacId。

Android console代码由独立Variant factory在debug注册，profile/release返回明确unsupported，不放宽既有配对安全门。原生Coordinator confirm时把DecodedPairingPayload与associationId注册到DebugConsoleCredentials；revoke删除。GATT使用CDM已关联地址，不额外扫描/局域网权限。

## 分工与验证

Root: Rust AES-GCM/HKDF协议、WorkConsole状态/配对授权接入、真机联调。
Native Mac: 扩展现有peripheral分片/队列/生命周期。
Android: 原生GATT、密钥注册、加密和MethodChannel。
UI: Mac/Flutter移除LAN入口改已配对Mac连接、测试。

验证加密跨语言向量、分片/背压/重放/断连、所有回归、Debug构建；真机只执行明确测试动作，不触碰用户实际聊天/项目操作。
