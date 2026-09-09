# Repose 现状事实基础

调研范围：`feat/phone-unlock-walking-skeleton` 分支，`src/`、`src-tauri/`、`docs/`。以下全部为读代码所得，不含推测；不确定处已标注。

---

## 0. 一句话结构

Repose 是一个 **Tauri v2 + React 19 的 macOS 单窗口应用**，前端只有一个 404 行的 `App.tsx` 单体组件（无路由、无状态库、无 UI 框架），Rust 侧是一个 1081 行的 `lib.rs`（含单测），中间用 7 个 Tauri command + 3 个事件通道通信。另有一个**独立的非 React 页面** `break.html`（原生 TS）用于全屏强制休息遮罩。

版本 0.6.3（`package.json` / `src-tauri/tauri.conf.json` / `src/App.tsx:15` 三处各写一份）。

---

## 1. src/ 的 React 结构

### 1.1 文件清单（全部）

```
src/main.tsx                       17 行  入口，先 initializeDesktopBridge() 再 render
src/App.tsx                       404 行  整个主窗口 UI（5 个页面 + 4 个 Modal + toast）
src/tauriBridge.ts                158 行  window.repose 门面 + 事件订阅 + payload 校验
src/hooks/useBreakTimer.ts        200 行  唯一的 hook，包住 lib/timer.ts 的纯函数
src/components/StretchTrainer3D.tsx 96 行 唯一被抽出的组件（长休息 3D 跟练）
src/break.ts                      197 行  break.html 的逻辑，原生 DOM，不用 React
src/lib/timer.ts                  578 行  计时状态机，纯函数，有 773 行单测
src/lib/reposeVoice.ts             78 行  短休息文案库（50 条）
src/lib/eyeCareTips.ts             21 行  护眼知识 6 条，带 NIH 出处注释
src/lib/stretch*.ts                       3D 拉伸编排 / Three.js 场景
src/lib/activityChart.ts           46 行  分时图数据
src/styles.css                    101 行  主样式（每行 = 一个区块，极长单行）
src/activity-chart.css            359 行  正常多行格式
src/break.css / eye-care.css / stretch-anatomy.css
```

**没有的东西**：没有 router、没有 Redux/Zustand/Context、没有 CSS-in-JS、没有 Tailwind、没有组件库、没有 i18n 框架、没有 Storybook。lucide-react 是唯一的 UI 依赖（只用图标）。

### 1.2 页面模型

`src/App.tsx:10`：

```ts
type Page = 'overview' | 'schedule' | 'ideas' | 'activity' | 'settings'
```

用 `useState<Page>('overview')` 切换，`navigate()` 同时关闭移动端菜单并滚到顶（`App.tsx:156`）。5 个页面各自是 `{page === 'xxx' && <div className="page-enter">…</div>}` 的内联 JSX 块：

| Page | 侧栏标签 | 图标 | JSX 位置 | 内容 |
|---|---|---|---|---|
| `overview` | 今日概览 | LayoutDashboard | `App.tsx:292-313` | 大计时卡 + 呼吸卡 + 3 个 stat-card + 灵感网格 + 节奏时间线 |
| `schedule` | 休息计划 | SlidersHorizontal | `App.tsx:315-324` | 3 个预设卡 + 两个 range 面板 + 节奏预览条 + 保存栏 |
| `ideas` | 休息灵感 | Flower2 | `App.tsx:326` | banner + 筛选行 + 大卡网格 |
| `activity` | 我的记录 | BarChart3 | `App.tsx:328-383` | 日期切换 + 24 小时堆叠柱图 + 休息足迹列表 + 导出 CSV |
| `settings` | 偏好设置（在侧栏底部，不在 navigation 数组里） | Settings2 | `App.tsx:385-393` | 见下 |

每个页面的大标题/副标题/英文 eyebrow 集中在 `titles: Record<Page, {title, subtitle, eyebrow}>`（`App.tsx:28-34`）。

### 1.3 设置页长什么样（对手机解锁最相关）

`App.tsx:385-393`，四个 `<section className="panel preferences-panel">` 竖排，**第一块就是系统权限相关的安全面板**：

```jsx
<section className="panel preferences-panel security-panel">
  <div className="section-heading">
    <div><h2>Mac 屏幕保护</h2><p>休息时专心休息，离开时安心离开。</p></div>
    <span className="subtle-badge"><Monitor size={13} />{window.repose ? 'Mac 桌面版' : '桌面版专属'}</span>
  </div>
  <div className="preference-row">
    <span className="preference-icon"><ShieldCheck size={21} /></span>
    <div><h3>强制休息</h3><p>覆盖全部显示器，屏蔽应用切换。每次可延迟一次；…</p></div>
    <Toggle label="强制休息" enabled={…} onChange={…} />
  </div>
  <div className="preference-row">
    <span className="preference-icon"><LockKeyhole size={21} /></span>
    <div><h3>30 秒无操作，安全锁屏<span className="security-tag">系统级锁屏</span></h3>
         <p>检测全局键盘和鼠标活动。连续 30 秒无操作后锁定 macOS，会话需正常认证解锁。…</p></div>
    <Toggle … />
  </div>
  <div className="security-permission">
    <LockKeyhole size={15} />
    <p>{window.repose ? '首次使用安全锁屏，请在系统设置中允许 Repose 使用辅助功能；… 锁屏只检测空闲时长，不读取或记录按键内容。' : '网页仅预览界面。…'}</p>
    {window.repose && <button className="text-button" onClick={() => window.repose?.openSecuritySettings()}>打开系统设置<ArrowUpRight size={14} /></button>}
  </div>
  <p className="security-limit">强制休息限制日常操作；系统级结束进程或关机仍由 macOS 管理。</p>
</section>
```

其余三块：**提醒与声音**（3 个 preference-row：提示音带「试听」text-button / 桌面通知 / 自动开启下一轮）、**你的空间，你的颜色**（`theme-grid`，3 个带缩略预览的 `theme-option`）、**about-panel**（BrandMark + 版本号 + 「使用指南」按钮）。页脚 `preferences-footer`：「偏好设置会自动保存到这台设备」+「恢复默认设置」。

**设置页的固定结构语法**是：`.panel` → `.section-heading`（h2 + p + 右侧 subtle-badge）→ 若干 `.preference-row`（`preference-icon` + `<div><h3/><p/></div>` + 右侧控件）→ 可选的 `.security-permission` 说明块 → 可选的 `.security-limit` 小字。

### 1.4 可复用的组件词汇（都在 App.tsx 内联定义或纯 CSS 类）

| 名字 | 定义位置 | 说明 |
|---|---|---|
| `Toggle` | `App.tsx:61` | `role="switch"` + `aria-checked`，35×20 药丸 |
| `Modal` | `App.tsx:64-86` | 自实现焦点陷阱 + Esc + body overflow 锁；`onClose` 传 `() => {}` 就变成不可关闭（休息弹窗就这么用） |
| `BrandMark` | `App.tsx:58` | 渲染 `./favicon.svg`（微笑小花） |
| `ExerciseCard` | `App.tsx:87` | 唯一的卡片组件 |
| `.button` / `.primary` `.light` `.outline` / `.full-width` | styles.css | 主按钮墨绿实心，min-height 36px |
| `.text-button` | styles.css | 无背景文字按钮，通常带 `ArrowRight`/`ArrowUpRight` |
| `.icon-button` | styles.css | 30×30 方形 |
| `.subtle-badge` | styles.css | sage 底小徽章 |
| `.toast` | `App.tsx:398` | 底部居中，`role="status"`，3.5 秒自动消失，单条（`setToast(string)`） |
| `.security-alert` | `App.tsx:291` | **跨页面顶部警告条**，见 §5 |
| `.empty-state` / `.chart-empty-state` | | 圆形图标 + 标题 + 说明 + text-button 出路 |

---

## 2. 视觉语言

### 2.1 设计 token：**存在但只覆盖一半**

`src/styles.css:1` 的 `:root` 里有 15 个变量：

```css
--bg:#fbfcf9; --sidebar:#f3f5ee; --panel:#fff; --text:#303b32; --muted:#8b9288;
--subtle:#677160; --line:#e9ece4; --green:#526a43; --dark-green:#3e5738;
--active:#e5ebdc; --sage:#eff3e9; --peach:#f7f0e9; --blue:#edf3f5;
--lavender:#f0eef5; --shadow:0 12px 35px #23352406;
```

深色主题在 `styles.css:8` 用 `[data-theme=dark]{…}` 重定义同一批变量，再加约 30 条针对性覆盖（`[data-theme=dark] .timer-card{…}` 之类）。

**重要事实：token 不是全量的。** 大量颜色是硬编码十六进制（`#789457`、`#8b9482`、`#94ad7d`、`#a8bc90`…），深色模式靠逐条 `[data-theme=dark] .xxx` 补。**新增界面如果只用变量，深色模式会缺一半；照现有习惯必须自己补 `[data-theme=dark]` 覆盖。**

主题写入方式（`App.tsx:160-165`）：

```ts
document.documentElement.dataset.theme = theme === 'system' ? (media.matches ? 'dark' : 'light') : theme
```
持久化在 `localStorage['repose-theme']`，默认 `'light'`（不是 system）。

### 2.2 配色气质

森林绿 + 米白。主色 `--green:#526a43`，强调点缀 `#789261`/`#8a9b6a`。四个柔和底色用于图标块和卡片：sage（绿）、peach（暖橙）、blue（青）、lavender（紫）。警告色系不是红，是**暖棕**（`.security-alert{background:#f7efe2;border:#ecdfc8;color:#aa8b58}`）。红色在整个应用里只出现一次：拉伸区域示意点 `#c74943`（`stretch-anatomy.css`）。**没有任何 error red / destructive 样式存在**——需要「撤销设备」这类破坏性动作时，现有语言里没有现成的视觉。

### 2.3 排版

- 字体：`@fontsource-variable/manrope` 本地打包，`'Manrope Variable', -apple-system, …, 'PingFang SC'`。装饰性文字用 `Georgia, serif` + `italic`（`.art-caption`、`.help-step>span` 的 01/02/03、break.css 的 `.brand`）。
- **字号非常小**：正文 10–11px，说明文字 8–10px，`.section-heading h2` 14px（≥1500px 时 16px），页面 h1 30px，倒计时 77px。字重 400–650，`letter-spacing` 普遍加宽（0.2–2px），行高 1.6–1.9。英文全大写 eyebrow 用 8px + `letter-spacing:2px`。
- `font-variant-numeric:tabular-nums` 用在所有数字上。

### 2.4 间距、圆角、阴影

- 圆角：面板 11–13px，按钮/输入 6–8px，Modal 20px，药丸 999px，break.html 里更大（22–34px）。
- 阴影极淡：`--shadow:0 12px 35px #23352406`（透明度 6/255）。悬浮卡片 `0 9px 22px #3c4c3110`。
- 布局：`.sidebar` 固定 218px，`.main-content{margin-left:218px;padding:0 42px;max-width:1720px}`。断点 **1500 / 1200 / 1000 / 760 / 420**，每档都手工重排（styles.css 第 9–13 行各是一整档）。窗口默认 1440×940，最小 960×720（`tauri.conf.json`）。

### 2.5 已建立的可访问性约定

`:focus-visible{outline:3px solid #94ad7d;outline-offset:5px}`；`@media(prefers-reduced-motion:reduce)` 全局关动画；Toggle 用 `role="switch"`/`aria-checked`；倒计时 `role="timer"` + `aria-label`；toast `role="status"`；图表列是 `<button aria-pressed>` 且 `aria-label` 念出完整数值；Modal 自实现 Tab 循环。**新界面被期待遵守这一套。**

### 2.6 CSS 文件的两种写法并存

`styles.css`、`break.css`、`eye-care.css` 是**每行一个区块的超长单行**（第 1 行就是整个 `:root`，第 12 行是整个 760px 断点）。较新的 `activity-chart.css` 是正常多行格式并用变量。两种风格都在仓库里，新文件按 `activity-chart.css` 的写法更安全。

---

## 3. src-tauri/src/ 的命令层

### 3.1 结构

`src-tauri/src/main.rs` 3 行，全部逻辑在 `lib.rs`（1081 行，末尾 `#[cfg(test)] mod tests` 有 8 个单测）。原生 ObjC 在 `src-tauri/native/macos.m`（127 行），通过 `unsafe extern "C"` 声明（`lib.rs:206-214`）：

```rust
fn repose_set_strict(enabled: bool);
fn repose_configure_cover(window: *mut c_void);
fn repose_idle_seconds() -> f64;
fn repose_notify(title: *const c_char, body: *const c_char);
fn repose_continuous_seconds() -> f64;
fn repose_observe_lifecycle(callback: extern "C" fn(i32));
```

### 3.2 全部 7 个 command（`lib.rs:876-884` 注册）

| command | 签名 | 返回 |
|---|---|---|
| `set_status` | `(app, shared, value: TimerStatus)` | `()` |
| `set_preferences` | `(shared, value: Preferences)` | `()` |
| `get_lifecycle_snapshot` | `(shared)` | `LifecycleSnapshot` |
| `acknowledge_lifecycle_interval` | `(shared, interval_id: String)` | `bool` |
| `postpone_break` | `(app, shared)` | `bool` |
| `notify_user` | `(value: NotificationValue)` | `()` |
| `open_security_settings` | `()` | `()` |

定义惯例：

```rust
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct TimerStatus { running: bool, phase: String, remaining: f64, break_id: Option<String>, … }

#[tauri::command]
fn set_status(app: AppHandle, shared: State<'_, Arc<SharedState>>, value: TimerStatus) { … }
```

前端调用一律传单个名为 `value` 的对象参数：`invoke('set_status', { value: status })`。共享状态是 `State<'_, Arc<SharedState>>`，内部 `Mutex<RuntimeState>`，`.lock().expect("state poisoned")`。

### 3.3 错误怎么表达 —— **这是最需要设计者知道的一条**

**现有代码里没有一个 command 返回 `Result`，也没有任何错误字符串跨越边界。** 三种表达方式：

1. **静默拒绝**。非法输入直接 `return`，前端拿不到任何信号。`set_status` 开头（`lib.rs:568-578`）：

```rust
if value.phase.len() > 40 || value.remaining < 0.0 || value.remaining > 604_800.0
    || value.break_id.as_ref().is_some_and(|id| id.is_empty() || id.len() > 200)
    || !matches!(value.postpone_seconds, 0 | 60 | 300) { return; }
```
`set_preferences` 同理：`if value.idle_lock_seconds != 30 { return; }`。

2. **`-> bool` 表示「做了 / 没做」**，不说为什么。`postpone_break` 有 5 个失败分支全部 `return false`。前端只能给一句笼统文案：

```ts
const accepted = await window.repose.postponeBreak()
if (!accepted) showToast('本次休息暂时无法延迟，请继续休息')
```

3. **用事件反向推送需要用户看见的失败**。这是唯一一条「后端告诉用户出事了」的通路（`lib.rs:806-808`）：

```rust
if !result.is_ok_and(|status| status.success()) {
    emit_command(&app, "idle-lock-failed");
}
```

**手机解锁的健康检查/安装失败要走哪条路，现有代码没有给出答案。** 三条都不够：静默拒绝无法排障，bool 说不出「插件在但系统没加载」和「授权规则被改」的区别，事件通道当前的 payload 类型 `CommandEvent { command: String, break_id: Option<String> }` 也装不下 remediation。这是需要新建的一层。

### 3.4 事件通道（Rust → JS）

| 事件名 | payload | 消费者 |
|---|---|---|
| `repose-command` | `CommandEvent { command, breakId }` | `App.tsx:175-186` 的大 switch |
| `repose-lifecycle` | `inactive-start` / `inactive-end` | `useBreakTimer.ts:80` |
| `repose-break-status` | `BreakSnapshot` | `break.ts` 全屏窗口 |

`repose-command` 目前 6 个取值（`tauriBridge.ts:4-10`）：`toggle-pause`、`start-short-break`、`start-long-break`、`postpone-break`、`strict-break-finished`、`idle-lock-failed`。前端有白名单校验（`commandNames` Set），未知 command 被丢弃——**新增命令必须同时改 `DesktopCommandName` 联合类型和 `commandNames` Set，否则静默失效。**

### 3.5 window.repose 门面

`src/tauriBridge.ts` 在 render 之前跑，`isTauri()` 为假就什么都不装。装上后暴露：

```ts
window.repose = {
  isDesktop: true,
  onCommand, onLifecycle, acknowledgeLifecycle,
  setStatus, setPreferences, notify, showBreak, postponeBreak, openSecuritySettings,
}
```

**`Boolean(window.repose)` 是全应用的「桌面版 vs 浏览器预览」开关**，在 App.tsx 里出现十余次，每次都配一段降级文案（例：点 Toggle 时 `showToast('全局键鼠检测与系统锁屏需要使用 Repose Mac App')`）。手机解锁面板必须遵守这个约定，否则网页预览和主页演示会崩。

`tauriBridge.ts` 还对每个入站 payload 做逐字段校验（`parseCommand` / `parseLifecycle`），非法直接丢弃——JS 侧把 Rust 侧当不可信来源对待。

### 3.6 权限与打包约束

`src-tauri/capabilities/default.json`：

```json
{ "windows": ["main", "break-*"],
  "permissions": ["core:default", "core:event:default", "core:window:default"] }
```

**新窗口 label 不匹配 `main` / `break-*` 就没有任何权限。** CSP（`tauri.conf.json`）：`script-src 'self'`，`img-src 'self' asset: data:`，无远程源。bundle target 只有 `app`，`minimumSystemVersion: 14.0`，identifier `ai.repose.lite`。

### 3.7 已有的两个后台线程与环境变量开关

- `run_break_monitor`：250ms 轮询，推 `repose-break-status`，到点关窗口并 emit `strict-break-finished`。
- `run_idle_monitor`：1s 轮询 `repose_idle_seconds()`，超过 30 秒执行 `osascript -e 'tell application "System Events" to key code 12 using {control down, command down}'`。
- `REPOSE_DISABLE_SESSION_LOCK`、`REPOSE_STRETCH_PREVIEW` 两个环境变量用于测试时绕开系统级行为（`lib.rs:761`、`lib.rs:902`）——测试可绕开的先例已存在。

### 3.8 已经在监听锁屏/解锁的原生层

`src-tauri/native/macos.m:44-67` 已经注册了 `com.apple.screenIsLocked` / `com.apple.screenIsUnlocked`（NSDistributedNotificationCenter）以及 sleep/wake/display-sleep/session-active 共 8 个事件，映射成 `event_code` 1–8 传回 Rust（`decode_native_lifecycle_event`，`lib.rs:40`）。**手机解锁要用到的「锁屏了 / 解锁了」信号，这一层已经有了，不需要新建。**

---

## 4. 现有功能与主流程

### 4.1 计时状态机

纯函数在 `src/lib/timer.ts`（`advanceTimerBy`、`startTimerBreak`、`postponeTimerBreak`、`completeTimerBreak`…），由 `useBreakTimer` 包住。持久化：`localStorage['repose.timer.v1']`，`version: 3`，内存每秒更新、每 15 秒落盘、生命周期边界与 pagehide 立即落盘。

默认设置（`timer.ts:70`）：

```ts
{ shortInterval: 20, shortDuration: 20, longEvery: 4, longDuration: 5,
  sound: true, notifications: false, autoStart: true }
```

即 **每 20 分钟短休息 20 秒，4 次后长休息 5 分钟**。

### 4.2 主流程

```
专注计时（主窗口可关，托盘继续跑）
  → 到点：播和弦音 + 系统通知 + 打开全部显示器的 break.html 全屏遮罩
    → 短休息：吉祥物 + 一条嘴欠文案 + 一条护眼知识 + 倒计时
    → 长休息：Three.js 3D 拉伸跟练，8 个动作轮播
  → 可「延迟一次」（短 60s / 长 300s），延迟后回来不能再延
  → 倒计时结束 → 关闭遮罩 → emit strict-break-finished → 前端记账
  → 记录写入 history / days / hourly，在「我的记录」页可查、可导出 CSV
```

并行存在**闲置锁屏**：与休息计时无关，30 秒无键鼠 → 锁 macOS。README 明确区分两者用途。

### 4.3 用户平时停在哪里

关键事实：**主窗口关掉后应用留在托盘继续跑**（`on_window_event` 里 `api.prevent_close(); window.hide()`）。所以日常高频出现的界面是：

1. **全屏休息遮罩 `break.html`** — 每 20 分钟出现一次，用户被迫看
2. **系统通知**（默认关，`notifications: false`）
3. **托盘菜单**（`lib.rs:825-833`）：`打开 Repose · 歇一会` / `暂停／继续提醒` / `现在小休息` / `现在大休息` / `退出 Repose`（强制休息期间除「打开」外全部被吞掉）

主窗口的 5 个页面里，**「今日概览」是默认落地页**，「偏好设置」属于低频、装配置时才去。手机解锁功能若只放在设置页，用户平时不会看见它的状态。

---

## 5. 有没有引导/向导？有没有既有的系统权限功能？

### 5.1 引导：**没有向导，只有一个手动打开的说明弹窗**

`App.tsx:399` 的 help Modal「认识 Repose」：BrandMark → 英文 eyebrow → h2「嗨，这里是 Repose.」→ 一段 intro → **三个 `.help-step`（Georgia 斜体的 01/02/03 + h3 + p）** → `.help-platform` 平台提示块 → 全宽主按钮「好的，慢慢来」。

它**不是首次启动自动弹的**，只能从侧栏「认识 Repose」或设置页「使用指南」打开。**仓库里不存在任何多步骤 wizard、进度指示器、stepper 组件。** 手机解锁的安装引导需要新造形态；`.help-step` 的编号步骤视觉是唯一可借的参照。

### 5.2 既有系统权限功能：**辅助功能（Accessibility），且它的完整降级模式是最好的先例**

这是目前唯一需要系统授权的功能，值得逐环拆开看：

1. **开关本身乐观翻转**：Toggle 一点就 on，不预检权限。
2. **就地说明**：`.security-permission` 块解释要什么权限，并**主动声明不做什么**——「锁屏只检测空闲时长，不读取或记录按键内容」。
3. **一键直达系统设置**：`invoke('open_security_settings')` → Rust 执行
   `open x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility`（`lib.rs:708-713`）。
4. **运行时才发现失败**：Rust 侧 osascript 返回非零 → emit `idle-lock-failed`。
5. **失败后自愈式降级**（`App.tsx:181-185`）：

```ts
if (command === 'idle-lock-failed') {
  setDesktopPreferences(previous => ({ ...previous, idleLockEnabled: false }))
  setSecurityError(true)
  showToast('安全锁屏未生效：请在系统设置中授予 Repose 辅助功能权限，再重新开启')
}
```
即：**自动把开关关掉** + 持久化一个错误标记（`localStorage['repose-security-error']`）+ toast。

6. **跨页面常驻横幅**（`App.tsx:291`）——这是「让用户平时看得见」的现成机制：

```jsx
{window.repose && !desktopPreferences.idleLockEnabled && <div className="security-alert" role="alert">
  <ShieldCheck size={19} />
  <div><strong>{securityError ? '安全锁屏需要系统授权' : '安全锁屏尚未开启'}</strong>
       <p>{securityError ? '当前自动锁屏未生效。请在系统设置 → 隐私与安全性 → 辅助功能中允许 Repose，然后重新开启 30 秒安全锁屏。'
                          : '目前离开电脑后不会自动锁屏。…'}</p></div>
  <button className="text-button" onClick={() => navigate('settings')}>前往设置<ArrowRight size={15} /></button>
</div>}
```

它渲染在 `page-heading` 之下、所有页面之上，**且区分「没开」和「开了但坏了」两种文案**，永远带一个具体去处。这套「Toggle + 说明 + 打开系统设置 + 运行时失败自动降级 + 全局横幅带出路」的组合，和 `docs/plans/2026-09-08-unlock-state-model.md` 里要求的「每个失败态都要有出路」是同一个思路，且已经有实现。

### 5.3 桌面偏好的存储位置

`localStorage['repose-desktop-preferences']`（`App.tsx:112-115`），类型：

```ts
type DesktopPreferences = { strictBreaks: boolean; idleLockEnabled: boolean; idleLockSeconds: 30 }
```

每次变更 `useEffect` 里既写 localStorage 又 `window.repose?.setPreferences(...)` 同步给 Rust。**Rust 侧不持久化任何偏好**，重启后以前端的推送为准。手机解锁的配对信息显然不能这样存（前端 localStorage + 明文），这是一个需要新建的持久化层。

---

## 6. 文案风格

### 6.1 语言

**简体中文为唯一功能语言**（`<html lang="zh-CN">`）。英文只作装饰：全大写 + 宽字距的 eyebrow，如 `A LITTLE PAUSE, A BETTER DAY`、`MAKE ROOM FOR YOURSELF`、`JUST BREATHE`、`MOVE SLOWLY, BREATHE EASILY`、`REPOSE HAS ENTERED THE CHAT`。**没有 i18n 框架，文案全部硬编码在 tsx/ts 字面量里。**

### 6.2 三个并存的语域

**(a) 产品外壳 —— 温柔、留白、不催促**

> 「给日常，留一点空白」 / 「让休息，自然发生。」 / 「你不必时刻满格，休息也是前进的一部分。」 / 「没有唯一正确的频率，舒服的节奏就是好节奏。」 / 「所有动作都以舒适为准。你也可以什么都不做，只是安静地待一会。」

句子短，多用「一点」「慢慢」「轻轻」「就好」。按钮也软：「保存我的节奏」「好的，慢慢来」「带着平静，继续」「现在，歇一会」。

**(b) 打断时刻 —— 故意嘴欠，但不恐吓不羞辱**

`src/lib/reposeVoice.ts` 有 50 条，分 5 组（`enter` / `notification` / `postpone` / `return` / `complete`），按 `breakId` 做 FNV hash 稳定选一条（同一次休息暂停/恢复不会换词）：

> 「电脑没意见，眼睛有。」「还在盯？我都替你眨累了。」「延期申请通过。我会在六十秒后准时回来。非常准时。」「一分钟到了。你的『马上』，现在归我管。」「休息完成，批准返岗。这次我就不盯着你了。暂时。」

README 对这套语气有明确边界：**「语气会逐步变得更『嘴欠』，但不恐吓、不羞辱，也不虚构健康风险。」**

**(c) 安全 / 系统 —— 严肃、具体、主动交代限度**

这一档最贴近手机解锁要写的东西：

> 「检测全局键盘和鼠标活动。连续 30 秒无操作后锁定 macOS，会话需正常认证解锁。暂停休息提醒不会关闭此保护。」
> 「锁屏只检测空闲时长，不读取或记录按键内容。」
> 「强制休息限制日常操作；系统级结束进程或关机仍由 macOS 管理。」

最后一句是**已有的、主动承认能力边界的先例**——不是免责声明的口吻，是把边界当事实陈述。README 里也有同一姿态：「无法承诺所有系统操作都不可绕过」。

### 6.3 事实类内容标出处

护眼知识底部 `<small>参考：美国国立卫生研究院 NIH · 读完就看远处吧</small>`，源链接以注释写在 `src/lib/eyeCareTips.ts` 文件头。3D 素材许可写在 `src/assets/STRETCH-HUMAN-LICENSE.md` 并在 README 指明。

### 6.4 Toast 惯例

单行、一句话、不带句号或只带一个句号，常是「已完成 + 温柔补充」：「已重新开始这一轮专注」「新节奏已保存，从这一刻开始」「已跳过这次休息，记得稍后照顾一下自己」「最近 7 天的记录已导出」。同时只显示一条（`useState<string>`），3.5 秒消失。

---

## 7. 对后续设计有直接约束的杂项事实

- **`break.html` 是独立入口**（`vite.config.ts` 的 rollup `input.break`），用原生 DOM，不能用 React 组件。全屏休息期间的任何解锁相关提示要写在这里就得手写 DOM。
- **窗口 label 约定**：全屏遮罩是 `break-{index}`，每个显示器一个（`lib.rs:484-523`），配 `repose_configure_cover` 做原生层级提升。
- **强制休息期间应用拒绝退出**（`RunEvent::ExitRequested` 里 `api.prevent_exit()`），托盘菜单项也被吞。
- **`docs/plans/2026-09-08-unlock-state-model.md` 已明确「在拿到 A1 结果之前不画安装引导的界面」**（153 行末）——A1 结果现已是「ad-hoc + SIP 开启即可加载」，对应表里的「一次管理员授权，体验接近普通 App / 可行性：好」。
- **`AGENTS.md` 是硬要求**：每次公开发布必须同步更新 `website/`（Next.js 主页）的版本、下载、功能与演示，中英文一致，并区分「浏览器模拟」与「安装包里的原生功能」。README 当前明确写着「手机钥匙与手机工作台不包含在此安装包中」——功能上线时这行必须改。
- **测试基线**：`npm test`（`node --test src/lib/*.test.ts`，纯逻辑层）+ `tests/run-all.sh`。后者的注释已经写明「一次全绿意味着什么」的教训：`# 项目已经学过一次，在端到端从未被证明时，全绿套件值多少钱。`
- `demos/phone-work-console/` 与 `.worktrees/phone-work-console/` 是另一条线（手机工作台）的产物，与解锁功能不是同一件事。旧解锁 UI 在 `codex/phone-proximity-unlock` 分支的 `src-tauri/src/unlock.rs`，本工作树内没有任何解锁相关代码（grep `unlock|proximity|bluetooth|ble|phone` 在 `src/` 与 `src-tauri/src/` 零命中）。