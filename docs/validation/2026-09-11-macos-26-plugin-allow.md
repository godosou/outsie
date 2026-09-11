# macOS 26.6.2 会加载插件，并在锁屏上放行

2026-09-11 · 日常机（Apple Silicon, macOS 26.6.2 / 25G83）

## 为什么单独记一条

`docs/validation/` 下此前二十余份记录，grep `macOS 26` 零命中。所有关于插件
能不能被加载、会不会被调用、放行是否生效的证据，都来自 macOS 14.6.1 的 Tart
VM——**而那台 VM 没有蓝牙射频**，permit 是经 ssh 塞进去的。

也就是说：插件那半边的全部证据来自一台不能测蓝牙的机器，蓝牙那半边的全部证据
来自一台从未验证过插件的机器。中间那一段，谁都没走过。

`docs/product-tech-research/2026-09-08-securityagent-plugin-loading.md` 把这条
列为"必须单独验证的高风险项"，并记录了一个真实案例：另一家的锁屏插件在
macOS 26.5 被 SecurityAgentHelper 拒绝，解锁卡 17 分钟、login keychain 锁死。

## 观察到的

`/tmp/repose-plugin.log`（root:wheel 0600），两次独立调用：

```
23:18:56.356 pid=86083 uid=0 AuthorizationPluginCreate hostVersion=4
23:18:56.361 pid=86083 uid=0 MechanismCreate id=permit mode=permit
23:18:56.362 pid=86083 uid=0 MechanismInvoke enter mode=permit
23:18:56.362 pid=86083 uid=0 permit: /var/run/repose-spike/permit present, root-owned and fresh after 0ms
23:18:56.362 pid=86083 uid=0 MechanismInvoke result=Allow
23:18:56.387 pid=86083 uid=0 MechanismDestroy

23:19:36.601 pid=88171 ... 同上 ... result=Allow
```

## 因此确定的

1. **macOS 26.6.2 会加载 ad-hoc 签名的第三方 SecurityAgent 插件。** 没有被
   SecurityAgentHelper 拒绝。
2. **它会在锁屏授权流程里调用我们的 mechanism。** `hostVersion=4`，`uid=0`。
3. **放行生效。** 插件读到 root 拥有且新鲜的 permit，返回 `Allow`，
   `k-of-n=1` 于是让这次空密码登录通过。
4. 判定耗时 `0ms`——permit 是一个文件的 stat，不在解锁的感知路径上。

## 因此仍未确定的

- **规则里另有一个第三方插件**（`com.openai.sky.CUAService.AuthorizationPlugin.remote`）。
  `k-of-n=1` 意味着任意一个放行即可，两者的相互影响没有单独测过。
- 这两次调用之间，没有测过"permit 不存在时是否正确拒绝"。
  拒绝路径在 VM 上验过（e3），在这台机器上没有。
- 不知道跨大版本升级后是否仍然成立。这正是本条记录存在的理由：
  **一条带版本号的事实，版本变了就不再是事实。**

## 复现

```sh
# 需要 root：日志是 0600
sudo tail -f /tmp/repose-plugin.log
# 另一边：锁屏，密码框留空，回车
```
