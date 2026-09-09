# permit 机制的安全设计（A4 / B3 的直接输入）

日期：2026-09-09

分支：`feat/phone-unlock-walking-skeleton`

## 现状与缺口

spike 里的判据是一行代码：

```c
#define PERMIT_PATH "/tmp/repose-permit"
/* open() 成功且 S_ISREG → Allow */
```

这在一次性 VM 里是对的选择 —— 它把「macOS 会不会听从插件」这个问题和别的变量隔开了，
而 A1 因此拿到了干净的答案。但它**不能进产品**，原因有四条，逐条都是独立的：

| # | 缺口 | 后果 |
|---|---|---|
| 1 | `/private/tmp` 是 1777 | 任何本地 uid `touch` 一下就越过锁屏 |
| 2 | 没有时效 | 插件从不 `unlink`，permit 一旦存在就永久有效 |
| 3 | 不绑定本次解锁尝试 | 一个 permit 可被任意次数、任意会话重复使用 |
| 4 | 方向是 fail-open | 存在源崩溃后文件残留 = 一直放行；正确的方向应该是崩溃即拒绝 |

第 4 条最容易被忽略：**presence 守护进程挂掉之后，机器应该更难进，而不是更容易进。**

## 「加个时间戳」不够

计划文档里原先写的修复是「改成带时间戳并校验有效期」。这**关不掉第 1 条**：
一个全局可写文件里的时间戳，攻击者可以直接写一个新鲜的。时间戳只解决第 2 条。

真正的修复必须同时做到：**只有受信任的进程能产生 permit**，以及**permit 只能用一次**。
这两件事文件系统都表达不了，所以正确答案不是改文件格式，是**换传输方式**。

## 正确形状：向 root 守护进程发一次请求

插件不再读文件，而是连一个 unix socket 问：「现在这次解锁尝试，手机在不在？」

```
SecurityAgent 里的 mechanism ──connect──▶ /var/run/repose/consume.sock
                            ◀─Allow/Deny─  root 守护进程（持有 BLE 与配对密钥）
```

这一下解决全部四条：产生者是 root 守护进程（第 1 条）；请求-响应天然有时效，
不存在残留物（第 2、4 条）；守护进程可以把一次响应绑定到一次请求并原子消费（第 3 条）。

## 旧分支已经写好的资产

`codex/phone-proximity-unlock` 的这几个 crate 正是这个形状，而且已经有测试：

| 资产 | 价值 |
|---|---|
| `repose-unlock-ipc` | 固定长度 wire 格式 + 向量测试 |
| `repose-unlock-service/src/permit_broker.rs` | 并发消费的契约测试 |
| `repose-unlock-service/src/peer_identity.rs` | 校验对端 uid / audit session / 代码签名要求 |
| `repose-unlock-core/src/permit.rs` | `ConsumedPermit` 用私有构造器 + `compile_fail` doctest，把「恰好消费一次」做成类型级保证 |

最后一条特别值得保留：它不是靠约定或注释，是靠类型系统让「伪造一个已消费凭据」编译不过。

## 但有一处必须改：对端身份 pin 错了

`peer_identity.rs` 里写着：

```rust
pub(crate) const AUTHORIZATIONHOST_REQUIREMENT: &str =
    "identifier \"com.apple.authorizationhost\" and anchor apple";
```

它假设 mechanism 跑在 `authorizationhost` 里。**A1 实测的是另一回事**：

```
2026-09-09T11:43:02.835 pid=7893 uid=92 AuthorizationPluginCreate hostVersion=4
```

`uid=92` 是 `_securityagent`。我们装的是**非特权**机制（`ReposeSpike:permit`，
不带 `,privileged`），它跑在 SecurityAgent 而不是 authorizationhost。
而带 `,privileged` 的那个变体在同样配置下**完全没有动静**，原因未查。

所以：
- 如果最终用非特权变体，对端要求必须改成 SecurityAgent 的签名标识；
- 如果要用特权变体，得先搞清楚它为什么不被调用。

**这两条路的安全性质不同**，值得单独决策而不是顺手选一个：特权变体以 root 运行，
能读只有 root 可读的东西，但一旦出问题影响面更大；非特权变体权限更小，
但守护进程必须接受一个非 root 对端的请求。

## 文件权限的一个具体约束

顺带记下一个容易踩的点：如果将来任何环节还需要插件读文件，那个文件**不能是 0600 root**
—— 机制以 uid 92 运行，读不了。需要的是 root 拥有、其他人可读、只有 root 可写：
目录 `root:wheel 0755`，文件 `root:wheel 0644`。

## 一条来自本项目的教训

今天早些时候在这台开发机上发现过一个 root 守护进程，它的控制套接字是这样创建的：

```
srw-rw-rw-  root  daemon  /private/var/run/ai.repose.unlock-control.sock
```

`0666`，任何本地进程可写，而同一份 plist 里的另一个套接字是 `0600`。
**IPC 换掉文件之后，套接字权限就成了新的单点。** 设计要求写死：
consume socket `0600` 或 `0660` 配受限组，绝不 world-writable；
并且在安装后由健康检查断言权限位，不能只靠 plist 写对。

## 待定

- `,privileged` 变体为何不被调用 —— 决定上面那个身份 pin 怎么写
- 守护进程如何把「一次 IPC 请求」绑定到「一次具体的解锁尝试」：
  `peer_identity.rs` 已经在取 audit session id，那可能就是正确的绑定键，需要验证
- permit 的有效期用什么时钟：墙钟可被调整，`CLOCK_MONOTONIC` 重启归零，
  macOS 上 `mach_continuous_time` 可能更合适
