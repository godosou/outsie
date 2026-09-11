# 在日常 Mac 上跑通了,双向

日期：2026-09-11 · 机器：使用者的日常工作 Mac(不是 VM)

## 结果

| 条件 | 动作 | 插件判定 | 屏幕 |
|---|---|---|---|
| 手机在旁边 | 锁屏 → **空密码**回车 | `result=Allow` | **进入** |
| 手机关掉(信标停) | 等 46 秒 → 空密码回车 | `result=Deny` ×2 | **保持锁定** |
| 手机不在 | 输**真密码** | `Deny` | **进入** |

插件自己的记录:

```
11:01:17  permit: present, root-owned and fresh after 0ms
11:01:17  MechanismInvoke result=Allow          ← 手机在

11:12:22  permit: absent after 1600ms, giving up
11:12:22  MechanismInvoke result=Deny           ← 手机不在
```

跑的是 **App bundle 里的**管线,不是仓库里的脚本:
`Repose.app/Contents/Resources/scripts/presence-pipeline.sh`。

## 这一轮抓到的四个 bug,全部来自"真的去用它"

| bug | 为什么测试看不见 |
|---|---|
| **同意书读不完、确认按钮够不到** | `.page-enter` 动画的残留 `transform` 架空了 `position:fixed`;CSS 里的 `max-height` 本来是对的。类型检查看不见,`tauri dev` 里窗口够大也看不见 |
| **`install` 没传 `ASSUME_YES`** | 脚本卡在自己的 `Proceed? [y/N]` 上 EOF 退出。`uninstall` 一直有,装的这条漏了 |
| **授权了却报"没有拿到管理员授权"** | `run_status` 把所有非零退出压成一个含义 |
| **打包后找不到 `run-root.sh`** | 仓库是 `../../lib`,bundle 是 `../lib`。`source` 不存在的文件不报错,一百行后才以 `command not found` 爆出来 |
| **`STALE` 在生产里从不触发** | 管线用 `sh` 起桥,**所有测试用 `bash`** ——同样输入,bash 1 条 STALE,sh 0 条 |

最后一条最值得记:**不是断言写错了,是断言瞄在了别处。** 脚本自己的测试永远看不到自己是被谁怎么启动的,所以现在有一条断言去检查**调用方**。

## 一个设计缺陷(已改,但只改了一半)

装完插件后 App **立刻弹第二个密码框**(启动在场监测用),没有任何解释。
它被关掉之后:扫描器照常跑、日志照常写、进程列表看着正常,
**而没有任何东西在验证** —— 功能是关的,每个界面都说它是开的。

现在管线会检查特权那一半到底起没起,发布 `noauth`,面板有专门的一句话。
**但"装完立刻弹第二个框"这件事本身还没改**,只是不再静默失败。

## 还没做的

- **真正的配对(SAS)** —— `K` 仍走 USB 下发,拿到数据线的人可以给自己发一把钥匙
- **从界面卸载并验证还原** —— 装是从界面走的,卸载还没验
- **8 小时耐久 / Doze** —— 一直排在最后
- **密码框署名 `osascript`** —— [issue 0003](../issues/0003-authorization-prompt-identity.md)

## 回滚

`~/repose-rollback/` 里有安装前的锁屏规则 plist 和 `RESTORE.md`,
独立于 Repose 自己的备份。
