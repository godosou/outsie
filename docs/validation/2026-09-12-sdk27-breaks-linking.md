# 构建要指定 SDK 26.5，因为 xcrun 会挑到一个链接不了的 27.0

2026-09-12 · 日常机（Apple Silicon, macOS 26.6.2 / 25G83）

## 症状

`cargo build` / `npx tauri build` 在链接阶段失败，报的不是我们的符号：

```
MacOSX27.0.sdk/.../CoreFoundation.tbd:4:20: error: unknown architecture
                   arm64e.x1-macos, arm64e.x1-maccatalyst ]
                   ^~~~~~~~~~~~~~~
  tapi error: malformed file
clang: error: linker command failed with exit code 1
```

AppKit / Foundation / CoreGraphics 同样。

## 原因

这台机器的 Command Line Tools 里有三个 SDK：

```
MacOSX15.4.sdk
MacOSX26.5.sdk     <- MacOSX.sdk 指向它
MacOSX27.0.sdk     <- 8/31 装进来的，MacOSX27.sdk 指向它
```

`xcrun --show-sdk-path` 返回 **27.0**（它取版本号最高的，不是 `MacOSX.sdk` 指向的那个）。
而 27.0 的 `.tbd` 里声明了 `arm64e.x1`，本机的链接器不认识：

```
$ ld -v
@(#)PROGRAM:ld  PROJECT:ld-1267   BUILD Jun  8 2026
configured to support archs: ... arm64 arm64e arm64_32 ...      # 没有 arm64e.x1
```

新 SDK 配旧链接器。**和本仓库的代码无关**，换个分支、回退提交都一样会失败。

## 做法

构建前指定 SDK：

```sh
export SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk
```

没有写进 `.cargo/config.toml`：那是一个绝对路径，加进去会让**没有这个 SDK 的机器**
构建失败——把一台机器的临时问题变成所有机器的永久问题。

## 这条记录什么时候失效

CLT 更新到链接器认识 `arm64e.x1` 的版本之后，就不需要它了。
判断方法是把 `SDKROOT` 去掉再构建一次——能过就删掉这条。

（`ui-conventions.md` §6.2：一条带版本号的事实，版本变了就不再是事实。）
