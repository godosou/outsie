#import <AppKit/AppKit.h>
#import <CoreGraphics/CoreGraphics.h>
#import <UserNotifications/UserNotifications.h>
#import <mach/mach_time.h>
#include <stdbool.h>

typedef void (*ReposeLifecycleCallback)(int event_code);

enum {
  ReposeScreenLocked = 1,
  ReposeScreenUnlocked = 2,
  ReposeSystemWillSleep = 3,
  ReposeSystemDidWake = 4,
  ReposeSessionInactive = 5,
  ReposeSessionActive = 6,
};

static ReposeLifecycleCallback repose_lifecycle_callback = NULL;
static NSMutableArray *repose_lifecycle_observers = nil;

static void repose_emit_lifecycle(int event_code) {
  if (repose_lifecycle_callback) repose_lifecycle_callback(event_code);
}

double repose_continuous_seconds(void) {
  mach_timebase_info_data_t timebase;
  if (mach_timebase_info(&timebase) != KERN_SUCCESS || timebase.denom == 0) return 0;
  long double nanos = (long double)mach_continuous_time() * timebase.numer / timebase.denom;
  return (double)(nanos / 1000000000.0L);
}

void repose_observe_lifecycle(ReposeLifecycleCallback callback) {
  dispatch_async(dispatch_get_main_queue(), ^{
    repose_lifecycle_callback = callback;
    if (repose_lifecycle_observers) return;

    repose_lifecycle_observers = [[NSMutableArray alloc] init];
    NSDistributedNotificationCenter *distributed = [NSDistributedNotificationCenter defaultCenter];
    NSNotificationCenter *workspace = [[NSWorkspace sharedWorkspace] notificationCenter];
    NSOperationQueue *main_queue = [NSOperationQueue mainQueue];

    [repose_lifecycle_observers addObject:[distributed
      addObserverForName:@"com.apple.screenIsLocked" object:nil queue:main_queue
      usingBlock:^(__unused NSNotification *note) { repose_emit_lifecycle(ReposeScreenLocked); }]];
    [repose_lifecycle_observers addObject:[distributed
      addObserverForName:@"com.apple.screenIsUnlocked" object:nil queue:main_queue
      usingBlock:^(__unused NSNotification *note) { repose_emit_lifecycle(ReposeScreenUnlocked); }]];
    [repose_lifecycle_observers addObject:[workspace
      addObserverForName:NSWorkspaceWillSleepNotification object:nil queue:main_queue
      usingBlock:^(__unused NSNotification *note) { repose_emit_lifecycle(ReposeSystemWillSleep); }]];
    [repose_lifecycle_observers addObject:[workspace
      addObserverForName:NSWorkspaceDidWakeNotification object:nil queue:main_queue
      usingBlock:^(__unused NSNotification *note) { repose_emit_lifecycle(ReposeSystemDidWake); }]];
    [repose_lifecycle_observers addObject:[workspace
      addObserverForName:NSWorkspaceSessionDidResignActiveNotification object:nil queue:main_queue
      usingBlock:^(__unused NSNotification *note) { repose_emit_lifecycle(ReposeSessionInactive); }]];
    [repose_lifecycle_observers addObject:[workspace
      addObserverForName:NSWorkspaceSessionDidBecomeActiveNotification object:nil queue:main_queue
      usingBlock:^(__unused NSNotification *note) { repose_emit_lifecycle(ReposeSessionActive); }]];

    CFDictionaryRef session = CGSessionCopyCurrentDictionary();
    if (session) {
      CFBooleanRef locked = CFDictionaryGetValue(session, CFSTR("CGSSessionScreenIsLocked"));
      if (locked == kCFBooleanTrue) repose_emit_lifecycle(ReposeScreenLocked);
      CFRelease(session);
    }
  });
}

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
