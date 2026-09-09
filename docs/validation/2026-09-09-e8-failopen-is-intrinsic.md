# E8 —— fail-open 是 evaluate-mechanisms 的通性；但存在 fail-closed 形态（有代价）

日期：2026-09-09

状态：**已确认（CONFIRMED）**，全程命令行、焦点无关、每次都无条件还原规则。

承接 [E3 fail-open](2026-09-09-e3-fail-open.md)。E3 证明了当前 `k-of-n=1` 形态在
bundle 缺失时可被空密码绕过。E8 要回答的是：**这个 fail-open 能不能靠换规则形态关掉。**

## 三个形态，一组对照

把 `system.login.screensaver` 临时换成不同形态，移走 bundle，用
`security authorize`（无 GUI、无凭据）观察放行还是拒绝，测完还原。

| 实验 | 规则形态 | bundle 缺失时的结果 |
|---|---|---|
| E3 | `k-of-n=1` over `[ai.repose.spike, use-login-window-ui]`，机制子规则单独可满足 | **fail-open**：exit 0，秒回放行 |
| E8 | 直接 `evaluate-mechanisms`，`mechanisms=[ReposeSpike:permit]`，无 k-of-n、无兜底 | **fail-open**：exit 0，秒回放行 |
| E8b | `evaluate-mechanisms`，`[ReposeSpike:permit, builtin:authenticate, …]` | **fail-closed**：卡在 `builtin:authenticate` 等密码（CLI 无从输入 → 超时） |
| E8b' | 同 E8b，但喂**错误密码** | **fail-closed**：弹 `Password:` → `NO (-60005)` 拒绝 |

E8b/b' 的插件日志在 bundle 缺失时为空，证明机制确实没运行；却仍走到了密码校验。

## 真正的机制语义

E8（链里只有我们的机制）缺失 → 秒回放行，而 E8b（其后接密码机制）缺失 → 落到密码。
把两者放在一起，唯一自洽的解释是：

> **authd 把「无法加载的机制」当作「该步骤通过（Allow / 继续下一步）」，而不是「失败」。**

于是整条权限是否被授予，取决于这一步之后**还有没有必须通过的关卡**：
- 缺失机制是**唯一关卡**（E3 的子规则、E8 的单机制规则）→ 通过即完成 → **放行(fail-open)**。
- 缺失机制**后面还跟着一个必然运行的密码机制**（E8b）→ 那个机制仍要密码 → **fail-closed**。

所以 fail-open **不是** k-of-n 特有的 bug，是 `evaluate-mechanisms` 对缺失机制的通用处理。
但也正因如此，**fail-closed 形态是存在的**：只要保证一个 builtin 密码机制排在我们机制之后、
且永远会运行。

## 代价：Allow 不等于「跳过密码」

E8b 的条件 1（bundle 在、permit 在、机制 `result=Allow`）**同样卡在 `builtin:authenticate`
要密码**。这说明：

> **机制返回 `kAuthorizationResultAllow` 只表示「本步通过、继续下一个机制」，并不跳过后续的
> `builtin:authenticate`。**

而当前 `k-of-n=1` 形态之所以能做到「手机在场、空密码即解锁」，恰恰是因为 `ai.repose.spike`
这个**子规则里只有我们的机制**——它单独 Allow 就满足了整个 k-of-n=1，**永远不会走到
`use-login-window-ui`**。也就是说：

> **当前能免密解锁，正是因为机制是「唯一关卡」——而这与 fail-open 是同一个根因。**

## 由此得到的核心张力

| 想要的性质 | 需要的结构 | 副作用 |
|---|---|---|
| 手机在场 → 免密解锁 | 机制是唯一关卡（能单独 Allow 授予） | bundle 缺失 → fail-open |
| 插件缺失 → 要密码 | 机制后必跟一个必然运行的密码机制 | 手机在场时**也**要密码（Allow 不跳过它） |

单靠规则形态，**两者不可兼得**。同时拿到两者，只有一条路：

> **机制在手机在场时，主动向授权上下文注入可满足 `builtin:authenticate` 的凭据**
> （用户名 + 口令，或等价的凭据 context），让后续的密码机制**静默通过**；手机不在或插件
> 缺失时不注入，密码机制照常提示。这样：
> - 手机在 → 机制注入 → `builtin:authenticate` 静默通过 → 免密解锁 ✓
> - 手机不在 → 不注入 → `builtin:authenticate` 提示密码 ✓
> - 插件缺失 → 机制被跳过 → `builtin:authenticate` 提示密码 → **fail-closed** ✓

这把三种情况统一到一个 fail-closed 的链上。**但它要求插件持有/能取得用户口令去回填 context**，
这是一个独立的、重量级的安全设计（口令的存储与保护），记为 **E10**，是本功能「免密且安全」
可行性的真正核心。

## 结论

1. **当前 `k-of-n=1` 形态不能进产品**（G2 阻塞）：bundle 缺失即 fail-open。
2. **换成「机制 + 其后 builtin 密码机制」的必经链可以 fail-closed**——这是正确的规则骨架。
3. **但要在该骨架上恢复「手机在场免密」，必须解决 E10（机制向 context 注入凭据）。**
   在 E10 落地前，安全的形态不能免密，免密的形态不安全。
4. 无论最终形态如何，**预防「悬空引用」仍是必须**：健康检查断言规则引用的 bundle 存在且可加载，
   否则立即把子规则从 authdb 移除。见 [E3 记录](2026-09-09-e3-fail-open.md)的约束清单。

## 复现

`$CLAUDE_JOB_DIR/tmp/e8.sh`、`e8b.sh`、`e8bp.sh`（本次运行的脚本）。核心命令：
`security authorizationdb write system.login.screensaver < <plist>`，配合
`security authorize [-C admin] system.login.screensaver`，移走/放回 bundle 观察。
每个脚本用 `trap restore EXIT` 保证还原。

## 待办

- **E10**：机制如何在手机在场时向授权 context 注入凭据以静默通过 `builtin:authenticate`；
  口令如何存储与保护（Keychain？Secure Enclave 包裹？）。这是免密+安全的可行性核心。
- **E9**：bundle 在位但签名失效/无法加载时，是走 E8（fail-open）还是 E8b（fail-closed）分支。
- 健康检查实现（断言 bundle 存在且 `codesign -v` 通过，否则移除子规则）。
