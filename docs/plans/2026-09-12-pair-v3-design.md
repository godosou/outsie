# 配对 v3：每台 Mac 各自一把钥匙

2026-09-12 · 设计稿

> **04:15 更正：下面「传递钥匙」那套是错的，别照着做。**
>
> 手机上的 K 存在 AndroidKeyStore 里，是**不可导出**的 HMAC 密钥
> （`PresenceKey.kt`：「non-exportable AndroidKeyStore HMAC_SHA256 key」），
> 而且手机屏幕上此刻就写着「钥匙存在手机的安全芯片里，谁也拿不出来，包括这个 App」。
>
> **手机拿不出 K，所以传不了。** 要让它能传，就得把钥匙从安全芯片挪到
> EncryptedSharedPreferences——那是把一句真话改成假话来换一个功能。
>
> 正确的做法在文末「更正后的设计」。原文保留，因为它记录了一个推论链：
> 「一部手机一把钥匙」这个前提本身是错的，而整套传输设计都是为了迁就它。

## 为什么写下来而不是直接做

P3 剩下的两件事——「keyId 归手机」和「配对改成传递钥匙」——**是同一件事**，
拆开做没有意义：手机自己选 keyId，Mac 就必须在配对时被告知这个 id；而要让同一把
钥匙进到第二台 Mac，配对就不能再是「各自派生一把新的」。

它跨三个代码库（Swift / Kotlin / Rust），改的是密钥交换，而且还剩三个没定的问题
（见最后一节）。2026-09-12 凌晨没有动它——半成品的密钥交换比没有更糟，而验证它
必须先删掉当前唯一能用的那把钥匙。

**当前的 v2 配对完全可用**，本文档不影响它。

## 今天是什么样

```
SAS（ECDH + commit-before-reveal + 六位数字）
  → K = HKDF(ecdhX, salt = transcript, info = "repose-pair-v2 presence-key")
  → 两端各自算出同一个 K，Mac 写进 /var/db/repose-unlock/presence-key.1
```

`keyId` 是两端的常量 `1`（`SpikeContract.PRESENCE_KEY_ID`、Rust 里的
`presence-key.1`）。**一台 Mac 一部手机时这完全正确。**

它在两个方向上都撑不住：

- **一部手机配两台 Mac**：手机只有一个射频、一把钥匙、广播一个 keyId。两台 Mac
  各自派生的 K 不同，手机没法同时是两把钥匙。
- **一台 Mac 配两部手机**：两部手机都会写 `presence-key.1`，后一个覆盖前一个。

## v3 长什么样

SAS 那一段**一个字都不改**——它已经验过，而且是这个流程里唯一需要人参与的部分。
改的是它产出的东西的用途：

```
K_t = HKDF(ecdhX, salt = transcript, info = "repose-pair-v3 transport")
```

叫 **传输密钥**，不是在场密钥。标签不同，所以 v2 的 K 和 v3 的 K_t 天然分域，
录到的 v2 会话没法当成 v3 的密钥。

手机自己持有 `K`（32 字节）和 `keyId`（首次使用时随机取 1..255，之后终身不变），
在一个新特征值 **FFF8** 上提供：

```
keyId(1) ‖ nonce(12) ‖ AES-GCM(K_t, K)(32) ‖ tag(16)      61 字节
AAD = transcript
```

**AAD 必须是 transcript**：它把这次密钥传递绑定到刚才那次人眼核对过六位数字的
会话上。少了它，一段录下来的密文可以被塞进另一次配对。

Mac 读 FFF8，用 K_t 解密，装进 `presence-key.<keyId>`。

**首次配对和后续配对走同一条路**：手机第一次用时自己生成 K，之后每台 Mac 都是
收下同一把。分支意味着两种情况里有一种测得少。

## Mac 侧要跟着改的

今天写死 slot 1 的地方（`grep -n "presence-key.1" src-tauri/src/unlock.rs`）：

- `presence_key_state()` —— 改成扫描目录，返回一组
- `UnlockSnapshot.device` —— 改成 `devices: Vec<PairedDevice>`
- `revoke_script()` —— 已经按 id 取路径，但 `revoke_device` 的 `device_id != "1"`
  守卫要改成「这个 slot 存在吗」
- `calibration_samples(csv, since, key_id)` —— 已经带 key_id 参数了，
  但校准界面要知道自己在给哪一部手机做
- uninstall 脚本 —— 要删掉所有 slot，不只是 1
- Kotlin 的 `PRESENCE_KEY_ID` 常量 —— 改成从 AppStore 读

`presence-verify` **不用改**：它已经按 keyId 找文件（`KeyStore.key(for:)`）。

## 三个还没定的问题

写下来是因为它们会决定实现，不是实现细节。

### 1. keyId 撞号

两部手机各自随机取，撞的概率约 1/255。计划原本写的是 Mac 回一个「换一个」，
那要多一个特征值和一次往返。

**倾向**：不做协商。Mac 发现该 slot 已被**别的指纹**占用时直接拒绝，并在界面上说
「这两部手机的编号撞了，在手机上重新生成一次再配」。0.4% 的情况，多一次操作，
换掉一整轮协议往返。**绝不能静默覆盖**——那会让另一部手机悄无声息地失效。

### 2. 多把钥匙时，状态信标怎么广播

`repose-macstate-v2` 的 tag 是用**某一把** K 算的（`emitMacStateTags(keys:keyId:)`）。
两部手机各有各的 K，Mac 得让两边都能验，但一次只能广播一个载荷。

选项：按窗口轮流（30 秒一把，手机最坏等 N×30 秒才更新）；或缩短窗口专门为此；
或让状态信标改用一把与手机无关的、配对时也交换的「广播密钥」。

**没有定。** 这是 v3 里唯一一个我认为还需要想清楚再写的地方。

### 3. 校准是每部手机一份

`calibration.json` 现在是一台 Mac 一份。两部手机的信号强度不一样（发射功率、
放口袋还是放桌上），阈值本该各算各的。而 `permit-bridge.sh` 现在只有一组
NEAR/FAR。

**倾向**：`calibration.json` 改成按 keyId 索引的字典，bridge 按行里的 keyId 选
阈值。桥的改动不大（它已经在按名字读 `auth=`/`cmd=`，多读一个 keyId 就行）。

## 验证计划

没有第二台 Mac 和第二部手机之前，能验的：

- 已知答案向量（Python 独立算）：K_t 的派生、AES-GCM 的密文与 tag
- 单部手机走完 v3 配对，确认 Mac 装进的是**手机报的那个 keyId**，不是 1
- 手机重新生成 keyId 后再配一次，确认 Mac 上出现第二个 slot、两个都能解锁
  —— 这一条用一部手机也能验，它就是「两部手机」的代码路径

**不能验的**：真正的两部手机同时在场、两台 Mac 各自的状态信标。
要诚实地写在验证记录里。


---

# 更正后的设计（2026-09-12 04:15）

## 一句话

**每台 Mac 各自一把钥匙，配对还是各自派生，什么都不用传。**

配对流程（SAS、commit-before-reveal、六位数字）**一个字节都不改**。变的只有两样：

1. 手机在配对时告诉 Mac **这把钥匙用哪个 keyId**，以及**它是哪一部手机**
2. 手机为它每一把钥匙各开一路广播

## 为什么这样反而更简单

原来的推论是：「手机只有一把钥匙 → 只能广播一个 keyId → keyId 不能由 Mac 分配 →
所以要传钥匙」。**第一步就错了。** 手机可以有 N 把钥匙，一台 Mac 一把——
安卓的 BLE 支持同时开多路广播集，不需要轮流，也就没有延迟代价。

去掉的东西：K_t、AES-GCM、FFF8 上的密文、一整套已知答案向量。
保住的东西：钥匙从来没有离开过安全芯片。

## 线上多出来的东西

新增一个只读特征值 **FFF8**：

```
keyId(1) ‖ phoneId(8)
```

两个都进 SAS transcript：

```
transcript = SHA256("repose-pair-v3 sas" ‖ pkM ‖ pkP ‖ nm ‖ np ‖ keyId ‖ phoneId)
```

**必须进 transcript**：不然中间人可以改 keyId，让 Mac 把钥匙装到别的槽位、
覆盖另一部手机。进了 transcript，一改六位数字就对不上，人眼那一关就挡住了。

`phoneId` 是手机第一次运行时随机生成的 8 字节，**不是秘密**，只用来认。
Mac 把它存在 `presence-key.<id>.phone` 旁边。

## 重新配对 vs 加一部手机

这两件事过去分不出来，现在分得出：

- **同一部手机重新配对**：phoneId 相同 → Mac 删掉这部手机原来那个槽位，装新的。
  不会留下一堆死钥匙。
- **第二部手机**：phoneId 不同 → 新增一个槽位，两部并存。
- **keyId 撞号**（两部手机各自随机挑到同一个数，约 1/255）：Mac 发现该槽位被
  **别的 phoneId** 占着，拒绝并说「这两部手机的编号撞了，在手机上重新生成一次」。
  **绝不静默覆盖。**

## 还剩的问题

1. ~~多把钥匙时状态信标怎么广播~~ **已做**（2026-09-12）。CoreBluetooth 的
   peripheral 一次只能广播一份载荷，所以不是"开多路"，是**轮流**：
   `presence-verify` 每个窗口对**每把**见到的钥匙各发一行 macstate，
   `state-advertise` 每三秒换一把。手机那边 20 秒就忘（`MAC_STATE_STALE_MS`），
   三秒一轮意味着六部手机也还在窗口内。

   一开始写的是"每个窗口发一行、用先到的那把钥匙"——那样两部手机各 60 秒才轮到
   一次，**两边都会长期显示「不知道」**。

2. **校准是每部手机一份。** `calibration.json` 改成按 keyId 索引的字典，
   bridge 按行里的 keyId 选阈值。**还没做。**

## 能验的

一部手机也能验完机制本身：重新配对一次，确认 Mac 上出现的是
`presence-key.<某个不是 1 的数>`，旧的 slot 1 被删掉，然后空密码回车能进。
**这条路径就是「第二部手机」走的同一条代码。**
