# VM 首次启动（约 10 分钟，只能手动）

`tart create --from-ipsw` 装出来的是一台全新的 macOS，第一次开机要走一遍初始设置向导。
这一段没法自动化，所以把它压缩成一条直线，照着点就行。

镜像用的是 `UniversalMac_14.6.1_23G93` —— 和这台宿主机**完全相同的版本和 build**。
所以在 VM 里得到的结论可以直接迁移，不用担心版本差异。

## 1. 启动

```bash
tart run repose-spike
```

会弹出一个窗口。**保持它开着** —— 关掉窗口等于关机。

## 2. 初始设置向导（照这个点）

| 步骤 | 选什么 | 为什么 |
|---|---|---|
| 语言 / 国家 | 随意 | |
| 辅助功能 | 跳过 | |
| 网络 | 已自动连上 | tart 默认 NAT，能上网 |
| 迁移助理 | **不传输任何信息** | |
| Apple ID | **跳过 / 稍后设置** | 不要登录你的真实 Apple ID，这是一台一次性机器 |
| 服务条款 | 同意 | |
| 账户 | 名称 `admin`，密码 `admin` | **必须记住**，后面 ssh 和 sudo 都要用；`vm-env.sh` 默认这个用户名 |
| 定位服务 | 关 | |
| 分析 / Siri / 屏幕使用时间 | 全部跳过 | |
| Touch ID | 跳过 | |
| 外观 | 随意 | |

密码用弱密码是刻意的：这台 VM 会被反复快照回滚，不存任何真实数据，而实验里要频繁
输密码验证 fallback。

## 3. 开启远程登录

系统设置 → 通用 → 共享 → **远程登录** 打开。

宿主机的验收测试通过 ssh 驱动这台 VM，没有这一步什么都跑不了。

## 4. 设置「锁屏后立即要求密码」

系统设置 → 锁定屏幕 → **「在屏幕保护程序开始后要求输入密码」→ 立即**。

**这一步最容易漏，漏了整个实验就没有意义** —— 没有密码要求就没有可解的锁。
macOS 14 已经读不到这个设置的 defaults 键，只能靠实际锁屏来验证。

## 5. 回到宿主机验证连通

```bash
source tools/vm-spike/vm-env.sh
repose_vm_check
```

应该看到 ssh 可达、锁屏状态可读、guest 版本是 `14.6.1 / 23G93`。

## 6. 立刻打快照

```bash
tart stop repose-spike
tart clone repose-spike repose-spike-clean
tart run repose-spike
```

`repose-spike-clean` 是干净基线。实验把 VM 搞坏之后：

```bash
tart delete repose-spike
tart clone repose-spike-clean repose-spike
```

**在装任何东西之前先打这个快照。** 整个实验的安全性都建立在「随时能回到干净状态」上。

## 7. 确认 oracle 在 VM 里成立

在 VM 的终端里：

```bash
./lockstate.sh          # 应输出 unlocked
open -a ScreenSaverEngine
# 等几秒，从宿主机读：
```

宿主机：

```bash
eval "$REPOSE_LOCKSTATE_CMD"    # 应输出 true
```

在 VM 里输密码解锁，再读一次应输出 `false`。

**这个往返走通了，才能开始 A1。** 如果 `IOConsoleLocked` 在 VM 里不随锁屏变化（虚拟机
没有真实显示器，理论上有这个风险），就得先换一个 oracle，否则后面所有结论都不可信。

## 已知的坑

- **关窗口 = 关机。** 长时间实验要让窗口一直开着。
- `pmset displaysleepnow` 在 VM 里的行为需要实测。`vm-env.sh` 默认用它触发锁屏，因为
  `launchctl asuser` 需要 root 而手工创建的账户不保证有免密 sudo。如果它不触发锁屏，
  退回到在 VM 窗口里手动 `open -a ScreenSaverEngine`。
- VM **没有蓝牙直通**。第一阶段全部用模拟存在源（ssh 写 permit 文件），真实 BLE 是
  第二阶段的事。
