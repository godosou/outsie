#import <AppKit/AppKit.h>
#import <CoreAudio/CoreAudio.h>
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
  ReposeDisplaysDidSleep = 7,
  ReposeDisplaysDidWake = 8,
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
      addObserverForName:NSWorkspaceScreensDidSleepNotification object:nil queue:main_queue
      usingBlock:^(__unused NSNotification *note) { repose_emit_lifecycle(ReposeDisplaysDidSleep); }]];
    [repose_lifecycle_observers addObject:[workspace
      addObserverForName:NSWorkspaceScreensDidWakeNotification object:nil queue:main_queue
      usingBlock:^(__unused NSNotification *note) { repose_emit_lifecycle(ReposeDisplaysDidWake); }]];
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

// Which processes hold an audio stream open, and which applications are
// running. The Rust side (meeting.rs) decides what counts as a meeting; this
// only reports. Reading these properties needs no microphone or camera grant.
static NSString *repose_audio_string(AudioObjectID object, AudioObjectPropertySelector selector) {
  CFStringRef value = NULL;
  UInt32 size = sizeof(value);
  AudioObjectPropertyAddress address = { selector, kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain };
  if (AudioObjectGetPropertyData(object, &address, 0, NULL, &size, &value) != noErr || value == NULL) return @"";
  return (__bridge_transfer NSString *)value;
}

static UInt32 repose_audio_u32(AudioObjectID object, AudioObjectPropertySelector selector) {
  UInt32 value = 0;
  UInt32 size = sizeof(value);
  AudioObjectPropertyAddress address = { selector, kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain };
  if (AudioObjectGetPropertyData(object, &address, 0, NULL, &size, &value) != noErr) return 0;
  return value;
}

char *repose_activity_json(void) {
  NSMutableArray *audio = [NSMutableArray array];
  AudioObjectPropertyAddress list = {
    kAudioHardwarePropertyProcessObjectList, kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain
  };
  UInt32 size = 0;
  if (AudioObjectGetPropertyDataSize(kAudioObjectSystemObject, &list, 0, NULL, &size) == noErr && size > 0) {
    UInt32 count = size / sizeof(AudioObjectID);
    AudioObjectID *ids = calloc(count, sizeof(AudioObjectID));
    if (ids && AudioObjectGetPropertyData(kAudioObjectSystemObject, &list, 0, NULL, &size, ids) == noErr) {
      count = size / sizeof(AudioObjectID);
      for (UInt32 i = 0; i < count; i++) {
        UInt32 input = repose_audio_u32(ids[i], kAudioProcessPropertyIsRunningInput);
        UInt32 output = repose_audio_u32(ids[i], kAudioProcessPropertyIsRunningOutput);
        if (!input && !output) continue;
        pid_t pid = (pid_t)repose_audio_u32(ids[i], kAudioProcessPropertyPID);
        NSRunningApplication *app = [NSRunningApplication runningApplicationWithProcessIdentifier:pid];
        [audio addObject:@{
          @"bundle": repose_audio_string(ids[i], kAudioProcessPropertyBundleID),
          @"name": app.localizedName ?: @"",
          @"input": @(input != 0),
          @"output": @(output != 0),
        }];
      }
    }
    free(ids);
  }
  NSMutableArray *running = [NSMutableArray array];
  for (NSRunningApplication *app in NSWorkspace.sharedWorkspace.runningApplications) {
    [running addObject:@{ @"bundle": app.bundleIdentifier ?: @"", @"name": app.localizedName ?: @"" }];
  }
  NSData *data = [NSJSONSerialization dataWithJSONObject:@{ @"audio": audio, @"running": running }
                                                 options:0 error:nil];
  if (!data) return strdup("{}");
  NSString *json = [[NSString alloc] initWithData:data encoding:NSUTF8StringEncoding];
  return strdup(json.UTF8String ?: "{}");
}

void repose_free_json(char *value) { free(value); }
