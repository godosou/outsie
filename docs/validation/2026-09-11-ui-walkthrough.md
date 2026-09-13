# 走界面流程时抓到的东西

日期：2026-09-11 · 未走完(见末尾「卡在哪」)

## 抓到一个真 bug:同意书读不完,也按不到确认

在真 Mac 上打开「手机钥匙 → 回车解锁」开关,安装说明书弹出来 ——
**只显示得出标题和第一条,后面全部在窗口下沿之外,而且滚不到。**

也就是说「我了解了,继续」这个按钮**够不到**,我刚做的第三方插件披露
(这台机器上恰好有一个)**用户根本看不见**。

**一份读不到末尾的同意书,出现在「用户正要同意改自己锁屏」这个屏幕上,
是这个 App 里最不该有这个 bug 的位置。**

### 原因是全局的,不是这个面板的

```css
.page-enter { animation: page-in .4s ease both }
@keyframes page-in { from{transform:translateY(7px)} to{transform:translateY(0)} }
```

`animation-fill-mode: both` 让最后一帧**永久生效**,而 `transform: translateY(0)`
**不是 `none`** —— 它会为后代的 `position: fixed` 建立包含块。于是:

- 遮罩层不再相对视口定位,而是相对这个页面容器
- `.modal` 上本来写好的 `max-height: calc(100dvh - 50px); overflow: auto` 失效,
  因为 `100dvh` 已经不对应可视区域
- 弹窗溢出到窗口外,没有滚动条

CSS 里该有的防御**本来就写了**,是被一个动画的残留 transform 架空的。
这类 bug 类型检查器看不见、单元测试看不见、`tauri dev` 里窗口够大时也看不见。

### 修法

弹窗改用 `createPortal` 挂到 `document.body`,彻底绕开祖先的 transform。
这比改那个动画更稳:动画是全局的,而 `transform: none` 在动画填充下是否
建立包含块,不同引擎的行为不值得赌。

## 卡在哪

**VM 里驱动不动 GUI。** 点击到不了 tart 窗口(试过 System Events `click at`,
苹果菜单都打不开),`Ctrl+F2` 这类修饰键组合也进不去 —— **只有普通按键能到 VM**。
所以 VM 里只能做到"登录、按回车",做不到"点界面走流程"。

**日常 Mac 上走到一半锁屏了。** 锁着的主机没有可注入的图形会话,
所以流程停在这里。**没有尝试解锁。**

## 这台 Mac 现在的状态:没动过任何东西

```
锁屏规则   ["com.openai.sky.CUAService.AuthorizationPlugin.remote","use-login-window-ui"]  (原样)
插件目录   只有 CodexComputerUseAuthorizationPlugin.bundle,没有我们的
permit 目录 不存在
```

安装说明书从未被确认,所以什么都没装。

独立备份留在 `~/repose-rollback/`(规则 plist + `RESTORE.md`),
和 Outsie 自己的备份无关 —— 万一以后装出问题,那份是兜底。

## 还没验的

- 从界面完成安装(卡在同意书之后)
- 从界面卸载并确认还原
- 界面里看到真实的在场状态(需要手机 + 已安装)
