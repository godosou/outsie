# 配对 v3：钥匙归手机，Mac 只是收下它

2026-09-12 · 设计稿，**还没实现**

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
