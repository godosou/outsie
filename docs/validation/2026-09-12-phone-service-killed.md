# 手机上的前台服务会被系统杀掉，钥匙随之静默失效

2026-09-12 · 真我 GT5 Pro（Android 16 / API 36，ColorOS）

## 观察到的

配对成功、解锁验证通过之后，手机放着不动。**大约五分钟后**：

```
$ adb shell dumpsys activity services ai.repose.blespike | grep -c ServiceRecord
0
```

Mac 这边：

```
permit-bridge: STALE (no sample for 45s) -> clearing permit
MechanismInvoke result=Deny
```

手机屏幕停在桌面，Outsie 不在最近任务里。**没有任何提示。**

## 为什么这条重要

它和「开关只存在内存里」（`9c23475`）是同一类失败，但那一类修完了这一类还在：

- 界面上没有任何东西是错的——手机上的开关本来就没在显示（App 被杀了）
- Mac 上没有任何东西是错的——它诚实地说「没有看到你的手机」
- 于是这个功能看起来只是**时灵时不灵**

一个「手机就是钥匙」的产品，钥匙会在你没注意的时候停止工作，而且是安静地停，
这比它根本不工作更糟：**你会带着一把你以为有效的钥匙走到电脑前。**

## 原因

`BleSpikeService` 是前台服务、`START_STICKY`，按 AOSP 的语义系统应当把它拉回来。
ColorOS 的后台管控不遵守这一点——这不是 bug，是 OEM 的既定行为，
小米 / 华为 / OPPO 系都有类似策略。

对照：

```
$ adb shell dumpsys deviceidle whitelist | grep -c blespike
0
```

**这个 App 不在电池优化白名单里。** 加进去要用户自己点，
`ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS` 只能弹系统对话框，不能代劳。

## 做法

`BootReceiver`（`9c23475`）只覆盖开机与更新，不覆盖「被系统杀掉」——
系统杀掉之后不会广播任何东西给被杀的那个 App。

所以唯一可靠的办法是**一开始就别被杀**：主屏上加一行，说明现在会被杀、
按钮直接打开那个系统对话框。已实现，见同日提交。

**不做自动弹窗**：一个 App 启动就要「不受电池优化限制」，看起来像流氓软件，
而且用户没有上下文判断该不该给。先让他遇到问题，再解释为什么。

## 还没验的

加进白名单之后能撑多久。**没有验过**——需要放置几个小时，
而这台手机今晚一直在被反复重装。这条要在一个完整的工作日里重测。
