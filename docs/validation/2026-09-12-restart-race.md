# 配对成功之后，在场管线会自己退出

2026-09-12 · 日常机（macOS 26.6.2）· 真我GT5 Pro

## 症状

配对全程成功——两端六位数字一致、密钥写进 `/var/db/repose-unlock/presence-key.1`、
面板走到「配好了，还差一次验证」。然后锁屏，**空密码回车进不去**。

进程只剩两个：

```
rssi-scan          还在扫
state-advertise    还在广播
presence-verify    没了
permit-bridge      没了
```

`bridge.log`：

```
permit-bridge: starting: auth=VALID required, near>=-72 far<=-85 ...
permit-bridge: run flag gone; clearing permit and exiting
```

启动几秒后就说运行标记不见了。

## 原因

配对成功的那段代码（`unlock.rs`，`unlock_pair_confirm` 里）是：

```rust
std::thread::spawn(move || {
    let _ = set_presence_running(&restart, false);   // 停
    let _ = set_presence_running(&restart, true);    // 起
});
```

两次独立调用，**中间不等**。而旧管线的 `cleanup()`（`presence-pipeline.sh`）是：

```sh
rm -f "${RUNFLAG}"          # 先摘标记
kill "${SCAN_PID}"
sleep 4                     # 给特权半边四秒退干净
```

运行标记原本是一个固定路径 `${WORK}/running`，**所有次运行共用**。于是：

```
t=0.0  旧管线收到 TERM，cleanup 开始
t=0.0  旧管线 rm running
t=0.1  新管线启动，创建 running          <- 同一个文件名
t=0.1  旧管线还在 sleep 4 里
       ……
```

只要新管线创建标记的时刻落在旧管线摘标记**之前**（两者由不同进程调度，顺序不保证），
旧管线就会删掉新管线刚建的那一个。新桥起来，一看标记没了，按设计退出。

`PipelineAction::Restart` 里那个 `sleep 2` 帮不上忙：一是这条路径走的是
Stop + Start 两次调用，根本不经过 Restart；二是 2 秒本来就短于旧管线自己写的 4 秒。

## 修法

**一次运行一个标记**，名字由调用方给：

```rust
static RUN_SEQ: AtomicU64 = AtomicU64::new(0);
let seq = RUN_SEQ.fetch_add(1, Relaxed);
let runflag = work.join(format!("running.{}.{seq}", std::process::id()));
// .env("REPOSE_RUNFLAG", &runflag)
```

```sh
RUNFLAG="${REPOSE_RUNFLAG:-${WORK}/running}"
```

这样一个正在退出的管线**只可能删掉自己建的那一个**，时序再怎么交错都无所谓。
比"把 sleep 从 2 调到 5"强的地方在于：后者是拿一个猜的数字去赌另一个猜的数字。

`std::process::id()` 单独不够——同一次 App 运行里重启两次会撞名，所以加了序号。

## 验证

修完重装，用面板开关跑一遍 Stop→Start：

```
$ pgrep -lf "rssi-scan|presence-verify|permit-bridge|state-advertise"
11541 rssi-scan
11543 state-advertise
12651 presence-verify --run-flag .../presence-run/running.11262.0
12653 bash permit-bridge.sh

$ tail -1 bridge.log
permit-bridge: ENTER (rssi=-53) -> asserting permit
```

然后锁屏、空密码回车，`IOConsoleLocked` 由 true 变 false，插件日志 `result=Allow`。

## 附带发现

配对一次要输**两遍**管理员密码：一遍写密钥，一遍起特权半边。两次
`with administrator privileges` 是分开发起的。架构上第二次是必要的（新密钥要新的
验证器进程），但从用户那一侧看是"按了一个按钮，被问了两次密码"。**还没修。**
