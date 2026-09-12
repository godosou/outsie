# 开会时不打扰：设计

日期：2026-09-12　调研：[会议检测调研](2026-09-12-meeting-detection-research.md)　探针：`tools/meeting-probe/run.sh`

## 一句话

Zoom、Teams、飞书、腾讯会议正在通话时，到点不进入休息，倒计时停在 00:00 等会议结束；
会议结束 30 秒后补上完整休息。开会的分钟是统计里独立的第三类：专注 / 会议 / 休息。

## 分工

| 层 | 文件 | 职责 |
|---|---|---|
| 原生 | `src-tauri/native/macos.m` `repose_activity_json` | 每秒报告：哪些进程持有音频流（CoreAudio 进程对象，macOS 14+），哪些应用在跑。只报告，不判断。 |
| Rust | `src-tauri/src/meeting.rs` | 判断：白名单 bundle 前缀的进程有输入或输出流，或 Zoom 的 CptHost 在跑 → 在会议中。连续 5 个样本同值才翻转。翻转时发 `repose-meeting`，`get_meeting_state` 供启动时同步。 |
| 桥 | `src/tauriBridge.ts` | `onMeeting` / `getMeetingState` |
| 计时器 | `src/lib/timer.ts` | `meeting`（实时，不持久化）、`meetingHold`（持久化）、`meetingEndedAt`；`advanceTimerBy` 做门控与 30 秒宽限；`recordTime` 按类别记秒。 |
| 界面 | `src/App.tsx` | 状态文案、首页「会议」卡片、活动图第三色、导出多一列、偏好设置开关「开会时不打扰」（默认开）。 |

强制休息的覆盖层由渲染层上报的 `phase` 驱动，Rust 从不自行进入休息，所以门控只在 TS 里做一次。

## 计时器语义

- 会议中专注倒计时照常走，秒数记入 `meetingSeconds`，不记入 `focusSeconds`。
- 倒计时归零且在会议中：不进休息，`meetingHold = {type, duration}`，`remaining = 0`，生成 `breakId`。
  若此前延迟过一次（`deferredBreak`），hold 接管它，`postponeUsed` 保持，之后不能再延迟。
- hold 期间：会议中记会议，会议结束后的等待记专注；不能暂停、不能重置、改设置不影响；
  手动「提前开始休息」直接进入该次休息；长时间离开（生命周期间隔 ≥ 休息时长）被动满足。
- 会议信号安静满 `MEETING_GRACE_SECONDS`（30 秒）→ 进入完整时长的休息，休息完成后周期数正常推进。
  宽限期内会议重新开始则继续扣住。
- 重启：hold 可恢复，`meeting` 恢复为 false，从恢复时刻起算宽限；桌面端会在 5 秒内重新宣告会议状态。
- 开关关闭：视同不在会议，既不扣休息也不记会议时间。

## 已知盲区

- 浏览器里开会：音频在浏览器进程里，白名单分不清开会和看视频。
- 只用手机入会。
- Zoom、Teams、腾讯会议只验证了 bundle ID，没有跑过真实会议；飞书验证过入会、静音、退会三态。

## 验证

`npm test`、`cargo test --lib`、`npm run build`；真机按调研文档的步骤：入会（不开视频、静音）→ 约 5 秒显示「会议中」→ 到点停在「会议中，结束后休息」→ 退会约 30 秒后弹出完整休息。
