#if REPOSE_TASK9_RELEASE_BLOCKED
#error("Repose Unlock release is blocked until Task 10 production signing is configured.")
#endif

import Flutter
import UIKit

@main
@objc class AppDelegate: FlutterAppDelegate {
  override func application(
    _ application: UIApplication,
    didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
  ) -> Bool {
    GeneratedPluginRegistrant.register(with: self)
    if let controller = window?.rootViewController as? FlutterViewController {
      let settings = FlutterMethodChannel(name: "ai.repose/system_settings", binaryMessenger: controller.binaryMessenger)
      settings.setMethodCallHandler { call, result in
        switch call.method {
        case "getStatus":
          result([
            "platform": "ios", "manufacturer": "Apple",
            "nearby": "notRequired", "notifications": "notRequired",
            "background": UIApplication.shared.backgroundRefreshStatus == .available ? "unrestricted" : "restricted",
            "powerSaver": ProcessInfo.processInfo.isLowPowerMode
          ])
        case "openAppSettings":
          guard let url = URL(string: UIApplication.openSettingsURLString) else { result(false); return }
          UIApplication.shared.open(url, options: [:]) { opened in result(opened) }
        default: result(FlutterMethodNotImplemented)
        }
      }
    }
    return super.application(application, didFinishLaunchingWithOptions: launchOptions)
  }
}
