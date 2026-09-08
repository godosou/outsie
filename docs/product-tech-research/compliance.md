# 平台权限与数据边界

日期：2026-09-08。方法：查阅 Apple 平台权限文档、手机蓝牙说明与已有工程验证记录。仅用于产品可行性输入；未进行司法辖区法律、商店审核或认证评估。

## 事实发现

- 辅助功能控制需要用户授予对应访问权限。[Apple 辅助功能](https://support.apple.com/guide/mac-help/allow-accessibility-apps-to-access-your-mac-mh43185/mac)
- 自动化其他应用存在单独的 Automation 授权；发送 Apple Events 的应用需提供用途说明。[Automation](https://support.apple.com/guide/mac-help/allow-apps-to-automate-and-control-other-apps-mchl108e1718/mac)、[用途声明](https://developer.apple.com/documentation/bundleresources/information-property-list/nsappleeventsusagedescription)
- 手机蓝牙前后台权限与生命周期受各平台约束，不能因为配对过就宣称永久可控。[Android 后台 BLE](https://developer.android.com/develop/connectivity/bluetooth/ble/background)、[Apple 后台蓝牙](https://developer.apple.com/library/archive/documentation/NetworkingInternetWeb/Conceptual/CoreBluetooth_concepts/CoreBluetoothBackgroundProcessingForIOSApps/PerformingTasksWhileYourAppIsInTheBackground.html)

## 对我们的启示

<!-- HWPR -->
把“设备信任”“工作控制启用”“具体动作权限”分别展示。首版在本地已解锁会话执行 Mac 保存的动作，锁屏和强制休息时暂停；撤销设备立即停止后续控制。宏配置存本地，不在日志里记录提示词和回复正文。

<!-- HWPR -->
采用单一明确目标与结果回执降低误操作。关闭 tmux 窗格前明确它会终止其中程序，提供取消。正式分发前再按实际传输方案和目标商店检查权限声明、签名与审核要求；本轮不声称已满足发布要求。
