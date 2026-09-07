#import <AppKit/AppKit.h>
#import <CoreGraphics/CoreGraphics.h>
#import <UserNotifications/UserNotifications.h>
#include <stdbool.h>

void repose_set_strict(bool enabled) {
  dispatch_async(dispatch_get_main_queue(), ^{
    NSApplicationPresentationOptions options = NSApplicationPresentationDefault;
    if (enabled) {
      options = NSApplicationPresentationHideDock |
        NSApplicationPresentationHideMenuBar |
        NSApplicationPresentationDisableProcessSwitching |
        NSApplicationPresentationDisableForceQuit |
        NSApplicationPresentationDisableSessionTermination |
        NSApplicationPresentationDisableHideApplication;
    }
    [NSApp setPresentationOptions:options];
  });
}

void repose_configure_cover(void *raw_window) {
  dispatch_async(dispatch_get_main_queue(), ^{
    NSWindow *window = (__bridge NSWindow *)raw_window;
    [window setLevel:NSScreenSaverWindowLevel];
    [window setCollectionBehavior:NSWindowCollectionBehaviorCanJoinAllSpaces |
      NSWindowCollectionBehaviorFullScreenAuxiliary |
      NSWindowCollectionBehaviorStationary];
    [window setMovable:NO];
    [window setMovableByWindowBackground:NO];
  });
}

double repose_idle_seconds(void) {
  return CGEventSourceSecondsSinceLastEventType(
    kCGEventSourceStateCombinedSessionState,
    kCGAnyInputEventType
  );
}

void repose_notify(const char *raw_title, const char *raw_body) {
  NSString *title = [NSString stringWithUTF8String:raw_title];
  NSString *body = [NSString stringWithUTF8String:raw_body];
  UNUserNotificationCenter *center = [UNUserNotificationCenter currentNotificationCenter];
  [center requestAuthorizationWithOptions:(UNAuthorizationOptionAlert | UNAuthorizationOptionSound)
    completionHandler:^(BOOL granted, NSError *error) {
    if (!granted || error) return;
    UNMutableNotificationContent *content = [[UNMutableNotificationContent alloc] init];
    content.title = title;
    content.body = body;
    content.sound = [UNNotificationSound defaultSound];
    NSString *identifier = [[NSUUID UUID] UUIDString];
    UNNotificationRequest *request = [UNNotificationRequest requestWithIdentifier:identifier content:content trigger:nil];
    [center addNotificationRequest:request withCompletionHandler:nil];
  }];
}
