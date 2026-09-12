# 会议检测调研：开会时不进入强制休息

日期：2026-09-12　机器：MacBook Pro, macOS 26.6.2　探针：`tools/meeting-probe/run.sh`

## 结论

按**进程**判断，不按设备判断。会议软件的会议进程只要在收或放音频，就视为在会议中。
不依赖摄像头是否打开、麦克风是否静音，不需要任何 TCC 权限。

## 信号

| 信号 | API | 权限 | 作用 |
|---|---|---|---|
| 进程级音频活动 | CoreAudio `kAudioHardwarePropertyProcessObjectList` + `kAudioProcessPropertyIsRunningInput/Output` + `kAudioProcessPropertyBundleID`（macOS 14+） | 无 | 主信号 |
| 电源断言 | IOKit `IOPMCopyAssertionsByProcess`，会议进程持有 `PreventUserIdleDisplaySleep` | 无 | 第二道确认 |
| Zoom 子进程 | 会议中才存在的 `CptHost` | 无 | Zoom 兜底 |
| 设备级占用 | `kAudioDevicePropertyDeviceIsRunningSomewhere` / `kCMIODevicePropertyDeviceIsRunningSomewhere` | 无 | 仅参考，任何 app 用麦都会亮 |

bundle ID 白名单（本机确认）：`us.zoom.xos`、`com.microsoft.teams2`、`com.electron.lark`（会议进程是 `com.electron.lark.iron`，用前缀匹配）、`com.tencent.meeting`。

## 实测（飞书会议）

| 状态 | 会议进程音频 | 电源断言 | 摄像头 |
|---|---|---|---|
| 入会，不开视频 | 采集 1 / 播放 1 | 3 个 | 未占用 |
| 静音 | 采集 1 / 播放 1 | 3 个 | 未占用 |
| 退出会议 | 无 | 无 | 未占用 |

退会后信号立即清除，无残留。Zoom / Teams / 腾讯会议尚未用真实会议验证。

## 已知盲区

- 浏览器里开会：音频跑在浏览器 GPU 进程里，白名单分不清开会和看视频。
- 只用手机入会。

## 落地

- `src-tauri/native/macos.m` 增加 `repose_meeting_active()`，build.rs 追加 CoreAudio、IOKit framework。
- Rust 侧每秒轮询，形状同闲置锁屏的手机在场门控。
- 计时器复用 `deferredBreak`：会议中到点不进休息，计时继续，会议结束后补上完整休息。
- 防抖：连续命中 ≥5 s 才算在会议中；退会后等 ≥30 s 再弹休息。
