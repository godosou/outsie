# Issue 0003 · 要 root 的那个密码框,署名是 "osascript"

- **状态:** open
- **优先级:** 中(信任问题,不是安全漏洞)
- **发现:** 2026-09-11,在真机上走安装流程时,由用户指出

## 现象

点「开启手机钥匙」后,macOS 弹出管理员密码框,**标题是 `osascript`**。

用户的原话:「弹出框叫 oa script,有点不明所以」。

## 为什么

macOS 把授权请求归属到**发起请求的可执行文件**。App 是这样提权的:

```rust
Command::new("/usr/bin/osascript")
    .args(["-e", "do shell script \"…\" with administrator privileges"])
```

所以系统看到的请求者是 `/usr/bin/osascript`,不是 Repose。

## 为什么这件事重要

这是整个产品里**唯一一次**要用户交出管理员密码的时刻。而弹窗上署的名字,
用户没有任何理由认识。

「看到不认识的程序要密码就别给」是一条好习惯。我们正在训练用户违反它。

## 正解

让 Repose 进程**自己**发起授权,而不是借道 osascript:

- `AuthorizationCreate` + `AuthorizationExecuteWithPrivileges`(已废弃但可用),或
- `SMJobBless` 装一个特权 helper(Apple 推荐,工作量大得多)

两者都会让弹窗显示 **Repose**。

## 现在的权宜

安装说明书在按钮正上方提前说明:标题会显示 osascript,那是 Repose 用的工具。

**这只是把困惑换成了预告,没有解决信任问题** —— 一个训练用户"给不认识的程序密码"
的流程,不会因为我们提前打了招呼就变得可接受。
