# Repose 手机端品牌与权限体验

日期：2026-09-08

## 用户需求

手机端沿用 Mac Repose 的品牌、图标、排版和温和文案。用户应能理解手机钥匙当前状态、
每项权限的用途、拒绝后的影响，以及 realme 后台设置的入口。先在模拟器验证，再安装到
GT5 Pro 验收。此次交付是 UI 与系统设置体验，不开启尚未完成的生产 BLE 自动解锁链路。

## 设计与行为

- 暖白 `#fbfcf9`、鼠尾草绿 `#526a43`；森林深色 `#202b24`，跟随系统外观。
  Manrope 字体随应用离线打包，中文使用系统字体；复用 Mac 的四瓣花几何。
- Android 与 iOS 应用名称统一为 Repose；生成各密度图标，Android 使用 adaptive icon，
  配套启动画面。生成入口为 `node scripts/generate-mobile-brand.mjs`。
- 按系统语言提供中文、英文；设备名称保留原文。确认配对、移除设备、错误与恢复提示均
  本地化。一次性配对码不出现在日志，也不在打开页面时自动读取剪贴板。
- 首页保持真实状态。在当前 Android 构建中，未实现的解锁功能提示“版本尚未开放”，
  不再把缺少传输/伴生关联解释成用户没有允许后台。未开放状态提供使用与权限指南。
- 设置页分别显示附近设备、通知、后台运行与省电模式。当前构建没有附近设备或通知
  权限消费者，因此显示“此版本暂不申请”，首次启动不弹授权请求。
- Android 桥接仅在安装包声明需要的权限时允许由用户点击触发请求，区分首次拒绝与
  必须到设置中处理的拒绝。返回结果只代表请求完成，权限是否授予重新读取系统状态。
- 后台状态区分 ActivityManager 的后台限制、PowerManager 的电池优化与省电模式。
  电池优化不等于后台已被禁止；没有检测到限制也不保证厂商持续运行。
- 查看后台设置前说明用途、可能耗电与当前版本限制，用户可取消；确认后只打开本应用
  的系统详情页。不调用厂商私有 Activity、不修改白名单或系统开关、不自动请求忽略电池
  优化。realme / OPPO 文案提示查找后台活动、自启动并说明版本差异。
- 从系统设置回到应用时重新读取状态，读取失败清除旧的正常状态并提供重试。设置页
  关闭后刷新首页原生能力。无法打开设置时给出手动导航路径。
- iOS 使用独立系统说明，不展示 Android 电池优化指导；明确后台 App 刷新无法启用
  当前未支持的自动解锁。未添加蓝牙或通知隐私权限声明。

## 验证范围

TDD 覆盖中文品牌、权限未使用/拒绝/永久拒绝、先说明后请求、取消操作、返回刷新、读取
失败清除旧状态、设置失败手动路径、iOS 平台文案、大字体、设备名原样呈现和恢复能力。
原有配对确认、撤销和校准流程回归继续通过。Android 原生权限分类包含五项 JVM 测试。

模拟器可验证布局、系统设置跳转与状态读取，不证明真机 BLE、后台唤醒或距离精度。
iOS 仅修改共享 UI、品牌资源和系统设置桥接；本机未安装完整 Xcode，不能声称 iOS 编译
或真机测试通过。设备与最终自动化证据见 `docs/validation/mobile-ux-results.md`。

## 依据

- [Mac 设计源](../../src/styles.css)
- [Android 蓝牙权限](https://developer.android.com/develop/connectivity/bluetooth/bt-permissions)
- [Android 后台电池优化](https://developer.android.com/training/monitoring-device-state/doze-standby)
- [Android 设置接口](https://developer.android.com/reference/android/provider/Settings)
- [Google Fonts Manrope 源码与许可证](https://github.com/google/fonts/tree/main/ofl/manrope)
