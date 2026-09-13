# macOS 第三方 Authorization Plugin 加载对代码签名的要求

调研日期：2026-09-08
调研人：auth-plugin（AI Agent）
目标机器：macOS 14.6.1 (23G93)、Apple M3、SIP 开启、`security find-identity -v -p codesigning` 返回 0 个身份
调研方式：Apple Developer Forums（Apple DTS 工程师 Quinn 的一手回答）、开源实现与安全研究者分析、真实 issue 案例

---

## 结论先行（我们的 spike 该怎么配）

**在 macOS 14.6.1、SIP 开启、连 Developer ID 都没有的情况下，我们的 spike 大概率能直接跑通，不需要花 $99 买 Developer ID，也不需要关 SIP。**

关键机制：加载第三方 Authorization Plugin 的宿主进程（`authorizationhosthelper.arm64.xpc` 和 `SecurityAgentHelper-arm64.xpc`）被 Apple 用私有 entitlement `com.apple.private.security.clear-library-validation` **显式关闭了 library validation**。这意味着宿主可以加载"非平台二进制、非同 Team"的插件——这正是 Jamf Connect / NoMAD Login AD / Okta Verify 等第三方插件能工作的根本原因。你对这条推理的判断是对的，并且有一手证据佐证。

Apple DTS 工程师 Quinn 的明确表态（对我们最重要的三句话）：

1. **不需要公证（notarization）**："Your plug-in doesn't need to be notarised during development."
2. **日常开发不要用 Developer ID**："I recommend that you not use Developer ID signing for day-to-day work" —— 也就是说本地/自签（ad-hoc 或免费的 Apple Development 证书）就够加载。
3. **SIP 不构成限制、entitlement 不 gate 插件加载**：被问到 SIP 或 entitlement 是否影响加载时，回答是 "No"；真正决定能否加载的是 **bundle 打包结构是否正确 + 文件权限**，而不是签名策略。

因此我们的 spike 建议：**用 ad-hoc 签名（`codesign -s -`）先试**，插件放到 `/Library/Security/SecurityAgentPlugins/`（用 `sudo cp` 保证属主/权限正确），SIP 保持开启。这是 Quinn 本人的标准测试环境（"my standard test environment is indeed a VM with SIP enabled"，且他测的就是 Sonoma 14.6）。

**Developer ID + 公证的作用域是"分发"（Gatekeeper 首次启动检查），跟"能不能被系统进程加载"无关。** 只有当我们要把插件安装到别人的机器上、走正规安装包分发时，才需要 Developer ID + 公证。spike 阶段完全不需要。

一个需要盯住的风险点见"未解问题"：macOS 26 上有第三方插件（OpenAI Codex 的锁屏插件）被 `SecurityAgentHelper` 拒绝的真实案例。但那是在**最新的 macOS 26** 上、且集中在**锁屏（screensaver）UI 这条 mechanism 路径**，与我们 14.6.1 的目标环境不同，且 Quinn 明确说那条 library-validation 日志"具有误导性、不要被它带偏"。

---

## 证据表格

| # | 结论 | 证据 / 出处 | 可信度 |
|---|------|-------------|--------|
| 1 | 加载插件的宿主 XPC 服务带有 `com.apple.private.security.clear-library-validation`，从而对第三方插件关闭 library validation | Apple DTS Quinn 在讨论签名失败时明确说明宿主用此 entitlement "explicitly opts out of this implicit library validation"，并点名 `SecurityAgentHelper-arm64.xpc` 和 `authorizationhosthelper.arm64.xpc` 都带此 entitlement（[Apple Forums 776111](https://developer.apple.com/forums/thread/776111)）；安全研究者 theevilbit 独立逆向确认 `authorizationhosthelper` 的 entitlement 集合正是 `com.apple.security.smartcard` + `com.apple.private.security.clear-library-validation`（[theevilbit blog #0028](https://theevilbit.github.io/beyond/beyond_0028/)） | **高**（一手 Apple + 独立逆向互相印证） |
| 2 | 开发/测试阶段**不需要公证** | Quinn："Your plug-in doesn't need to be notarised during development."（[Apple Forums 805295](https://developer.apple.com/forums/thread/805295)） | **高**（Apple DTS 一手） |
| 3 | 日常开发**不建议用 Developer ID**，本地签名即可 | Quinn："I recommend that you not use Developer ID signing for day-to-day work."（[805295](https://developer.apple.com/forums/thread/805295)）；配套的签名指南建议日常用免费的 Apple Development 证书、Developer ID 只用于对外分发（[Apple Forums 732320 "Care and Feeding of Developer ID"](https://developer.apple.com/forums/thread/732320)） | **高** |
| 4 | **SIP 开启**不阻止第三方插件加载 | Quinn 被直接问到 SIP 是否有限制，回答 "No"；且其标准测试环境就是 SIP 开启的 VM，测试 OS 为 Sonoma 14.6（[805295](https://developer.apple.com/forums/thread/805295) / [776111](https://developer.apple.com/forums/thread/776111)） | **高** |
| 5 | 真正 gate 加载的是 **bundle 打包结构 + 文件权限**，不是签名 | Quinn 指出典型的 "unable to load bundle executable" 是打包问题；entitlement 对插件无意义（"Entitlements only make sense on a main executable, and plug-ins are not that"）；建议用 `QAuthHostSimulator` 调试加载（[805295](https://developer.apple.com/forums/thread/805295)） | **高** |
| 6 | Developer ID / 公证是**分发（Gatekeeper）**概念，不是本地运行/加载的前置条件 | 综合 [732320](https://developer.apple.com/forums/thread/732320)：Developer ID 认证"谁签的"、notarization 是分发前的恶意软件扫描，二者都是"ship to end users"环节，本地开发运行不需要 | **高** |
| 7 | Developer ID 签名（带 Team ID）的第三方插件在 SIP 开启的 stock 系统上能正常加载；那条 library-validation 报错日志本身**具有误导性/非致命** | Quinn："I've never seen this fail; my authorisation plug-ins always load just fine on stock systems"，并提醒开发者不要被该错误日志带偏（[776111](https://developer.apple.com/forums/thread/776111)） | **高** |
| 8 | 第三方 Authorization Plugin 在真实产品里可用（存在性证明） | Jamf Connect、NoMAD Login AD（开源，社区常自行重编译/重签）、Okta Verify 均以 `/Library/Security/SecurityAgentPlugins/` + authorizationdb mechanism 的方式工作（[jamf/NoMAD-ADAuth](https://github.com/jamf/NoMAD-ADAuth)、[nomad.menu 安全说明](https://nomad.menu/help/security-in-nomad/)） | **高**（生态事实） |
| 9 | ad-hoc（`codesign -s -`）签名的插件应可被加载 | **推理，非直接证据**：结论 1（LV 已关）+ 结论 3（本地签名即可）+ Apple Silicon 上 ad-hoc 二进制本身可执行。没有找到一句"ad-hoc 插件加载成功"的原话；Quinn 举的能加载的例子带 Team ID（即 Development/Developer ID 签名，非 ad-hoc）。需实验验证。 | **中**（强推理，缺一句直接原话） |
| 10 | macOS 26 上存在第三方锁屏插件被 `SecurityAgentHelper` library-validation 拒绝 + 挂起/keychain 锁死的真实案例 | OpenAI Codex 的 Locked Computer Use 插件在 macOS 26.5 报 "mapping process is a platform binary, but mapped file is not" 并 DENY，且有 17 分钟解锁卡顿、login keychain 锁死等副作用（[openai/codex #24013](https://github.com/openai/codex/issues/24013)、#39534、#40226、#29616） | **中高**（有据，但根因未被 Apple/维护者官方确认；且与我们目标 OS 不同） | 

---

## 推荐的实验梯度（先试什么，失败退到什么）

全程在**可回滚快照**的 macOS VM 里做（每一步前打快照）。我们的目标真机是 14.6.1，VM 也尽量用 14.6.x。

**梯度 0：不碰 login window，先证明"能加载"**
- 用 Apple 官方示例思路 / `QAuthHostSimulator`（Quinn 推荐）在**不改 authorizationdb、不动登录窗口**的前提下，验证我们的 bundle 能被宿主加载并调用。
- 这一步把"能否加载"和"登录流程是否被搞挂"解耦，风险最低。**SIP 保持开启。**

**梯度 1：ad-hoc 签名 + 真实 authorizationdb mechanism**
- `codesign -s - --deep` 我们的 `.bundle`，`sudo cp -R` 到 `/Library/Security/SecurityAgentPlugins/`（确认属主 root、权限正确）。
- 用 `authchanger` 或 `security authorizationdb` 把 mechanism 挂到一个**低风险的 right**（例如自定义 right，或先挂 `system.login.console` 之外、可安全测试的 right），观察 `log stream --predicate 'process == "authorizationhost" OR process CONTAINS "SecurityAgent"'`。
- 预期：能加载。若日志出现 library-validation 那行，先别慌——按结论 7 它可能是非致命的，看插件是否实际被调用。

**梯度 2：若 ad-hoc 被拒 → 退到免费 Apple Development 证书**
- 在 Xcode 里用免费 Apple ID 生成 Personal Team 的 "Apple Development" 证书（**仍然 $0，不需要 $99**），重签插件重试。
- 这对应 Quinn "日常用 Apple Development 证书" 的建议。

**梯度 3：若仍失败，且日志明确指向 library validation → 才怀疑签名/平台策略**
- 先核对 bundle 结构（Info.plist、`Contents/MacOS/` 可执行名、`CFBundleExecutable`）和文件权限——按 Quinn，这才是最常见真凶。
- 再核对我们是否错误地把 mechanism 挂到了会经过一个**没有** clear-library-validation 的宿主的路径上。

**梯度 4（仅在有据表明必须时）：动 SIP**
- 目前**没有任何一手证据**表明加载第三方 Authorization Plugin 必须关 SIP。除非梯度 0–3 明确失败且日志指向 SIP 策略，否则**不要关 SIP**。若要试，也只在 VM 里 `csrutil disable` 后对比，验证完立刻回滚快照。

**是否买 $99 Developer ID：** spike 阶段**不买**。只有当我们要把插件分发/安装到我们自己开发机以外的机器、走正规安装包时，再评估 Developer ID + 公证（那是 Gatekeeper 分发问题，不是加载问题）。

---

## 未解问题（诚实标注不确定）

1. **ad-hoc 具体能不能加载，缺一句直接原话（结论 9，可信度中）。** 逻辑链很强（LV 已关 + 本地签名即可 + Apple Silicon 可跑 ad-hoc），但 Quinn 举的正例带 Team ID。**必须用梯度 1 亲自验证**，这也是实验第一优先级。

2. **macOS 26 的锁屏路径回归（结论 10）。** Codex 案例显示在 macOS 26.5 上，锁屏（screensaver）mechanism 的第三方插件被拒 + 挂起 + keychain 锁死。根因未被 Apple/维护者官方确认，可能是：(a) macOS 26 对锁屏 UI 宿主的真实收紧；(b) 只是那条误导性日志 + Codex 自身实现问题。**如果我们的产品最终要走锁屏解锁路径、且要支持 macOS 26，这是必须单独验证的高风险项。** 对当前 14.6.1 目标不构成阻塞。

3. **不同 mechanism/right 是否都由带 clear-library-validation 的宿主加载。** 我们确认了 `authorizationhosthelper` 和 `SecurityAgentHelper` 两个宿主带此 entitlement，但 authorizationdb 里不同 right 会把 mechanism 分派到"privileged（非 UI）"还是"SecurityAgent（UI）"上下文。需在实验中确认我们实际用的 right 走的是带 entitlement 的宿主。

4. **macOS 15 (Sequoia) 的具体差异未逐版本证实。** 有一条线索是 `SFAuthorizationPluginView` 在 macOS 15 行为有变（[Apple Forums 771114](https://developer.apple.com/forums/thread/771114)），涉及**带自定义 UI** 的插件。若我们的插件要画自己的登录 UI，需单独查这条；纯逻辑 mechanism 不受影响。我们目标是 14.6.1，暂不阻塞。

5. **`security find-identity` 返回 0 个身份对 ad-hoc 无影响**（ad-hoc 不需要任何 keychain 身份），但若退到梯度 2 需要先在 Xcode 里生成免费 Apple Development 证书。

---

## 参考来源

- [Apple Developer Forums 805295 — How to debug SecurityAgentPlugins?](https://developer.apple.com/forums/thread/805295)（一手：不需公证、SIP 无限制、打包才是关键、推荐 QAuthHostSimulator）
- [Apple Developer Forums 776111 — Authorization Plugin code signing issue](https://developer.apple.com/forums/thread/776111)（一手：clear-library-validation entitlement、SIP-on VM 上正常加载、报错日志误导）
- [Apple Developer Forums 732320 — The Care and Feeding of Developer ID](https://developer.apple.com/forums/thread/732320)（一手：日常用 Apple Development、Developer ID 仅用于分发）
- [Apple Developer Forums 771114 — Change in behaviour of SFAuthorizationPluginView in macOS 15](https://developer.apple.com/forums/thread/771114)（macOS 15 自定义 UI 变化线索）
- [theevilbit — Beyond LaunchAgents #28: Authorization Plugins](https://theevilbit.github.io/beyond/beyond_0028/)（独立逆向：宿主 entitlement 集合、加载架构）
- [openai/codex issue #24013](https://github.com/openai/codex/issues/24013)（macOS 26.5 锁屏插件被拒真实案例；关联 #39534 / #40226 / #29616）
- [jamf/NoMAD-ADAuth](https://github.com/jamf/NoMAD-ADAuth) 与 [nomad.menu 安全说明](https://nomad.menu/help/security-in-nomad/)（第三方插件生态存在性证明）
