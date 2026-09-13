// meetingprobe: prints every signal that could mean "a meeting is going on".
// Build: clang -isysroot $SDKROOT -fobjc-arc -framework Foundation -framework CoreAudio -framework CoreMediaIO -framework IOKit -framework AppKit meetingprobe.m -o meetingprobe
#import <Foundation/Foundation.h>
#import <AppKit/AppKit.h>
#import <CoreAudio/CoreAudio.h>
#import <CoreMediaIO/CMIOHardware.h>
#import <IOKit/pwr_mgt/IOPMLib.h>

static NSString *nameOf(AudioObjectID id, AudioObjectPropertySelector sel) {
  CFStringRef s = NULL; UInt32 size = sizeof(s);
  AudioObjectPropertyAddress a = { sel, kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain };
  if (AudioObjectGetPropertyData(id, &a, 0, NULL, &size, &s) != noErr || !s) return @"?";
  return (__bridge_transfer NSString *)s;
}
static UInt32 u32Of(AudioObjectID id, AudioObjectPropertySelector sel, AudioObjectPropertyScope scope) {
  UInt32 v = 0, size = sizeof(v);
  AudioObjectPropertyAddress a = { sel, scope, kAudioObjectPropertyElementMain };
  AudioObjectGetPropertyData(id, &a, 0, NULL, &size, &v);
  return v;
}

// 1. Per-process audio activity (macOS 14+). Works even if the app is muted, as
//    long as it keeps an input or output stream open.
static void probe_processes(void) {
  AudioObjectPropertyAddress addr = { kAudioHardwarePropertyProcessObjectList, kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain };
  UInt32 size = 0;
  AudioObjectGetPropertyDataSize(kAudioObjectSystemObject, &addr, 0, NULL, &size);
  UInt32 n = size / sizeof(AudioObjectID);
  AudioObjectID ids[n];
  AudioObjectGetPropertyData(kAudioObjectSystemObject, &addr, 0, NULL, &size, ids);
  for (UInt32 i = 0; i < n; i++) {
    UInt32 in = u32Of(ids[i], kAudioProcessPropertyIsRunningInput, kAudioObjectPropertyScopeGlobal);
    UInt32 out = u32Of(ids[i], kAudioProcessPropertyIsRunningOutput, kAudioObjectPropertyScopeGlobal);
    if (!in && !out) continue;
    pid_t pid = (pid_t)u32Of(ids[i], kAudioProcessPropertyPID, kAudioObjectPropertyScopeGlobal);
    NSString *bundle = nameOf(ids[i], kAudioProcessPropertyBundleID);
    NSRunningApplication *app = [NSRunningApplication runningApplicationWithProcessIdentifier:pid];
    printf("AUDIO-PROC pid=%d bundle=%s name=%s input=%u output=%u\n", pid, bundle.UTF8String,
           (app.localizedName ?: @"-").UTF8String, in, out);
  }
}

// 2. Device-level "running somewhere" (what the menu-bar orange/green dot shows).
static void probe_devices(void) {
  AudioObjectPropertyAddress addr = { kAudioHardwarePropertyDevices, kAudioObjectPropertyScopeGlobal, kAudioObjectPropertyElementMain };
  UInt32 size = 0;
  AudioObjectGetPropertyDataSize(kAudioObjectSystemObject, &addr, 0, NULL, &size);
  UInt32 n = size / sizeof(AudioObjectID);
  AudioObjectID ids[n];
  AudioObjectGetPropertyData(kAudioObjectSystemObject, &addr, 0, NULL, &size, ids);
  for (UInt32 i = 0; i < n; i++) {
    UInt32 running = u32Of(ids[i], kAudioDevicePropertyDeviceIsRunningSomewhere, kAudioObjectPropertyScopeGlobal);
    if (running) printf("AUDIO-DEV  %s running\n", nameOf(ids[i], kAudioObjectPropertyName).UTF8String);
  }
  CMIOObjectPropertyAddress caddr = { kCMIOHardwarePropertyDevices, kCMIOObjectPropertyScopeGlobal, kCMIOObjectPropertyElementMain };
  UInt32 csize = 0, used = 0;
  CMIOObjectGetPropertyDataSize(kCMIOObjectSystemObject, &caddr, 0, NULL, &csize);
  UInt32 cn = csize / sizeof(CMIOObjectID);
  CMIOObjectID cids[cn];
  CMIOObjectGetPropertyData(kCMIOObjectSystemObject, &caddr, 0, NULL, csize, &used, cids);
  for (UInt32 i = 0; i < cn; i++) {
    UInt32 running = 0, rsize = sizeof(running);
    CMIOObjectPropertyAddress ra = { kCMIODevicePropertyDeviceIsRunningSomewhere, kCMIOObjectPropertyScopeGlobal, kCMIOObjectPropertyElementMain };
    CMIOObjectGetPropertyData(cids[i], &ra, 0, NULL, rsize, &used, &running);
    CFStringRef name = NULL; UInt32 nsize = sizeof(name);
    CMIOObjectPropertyAddress na = { kCMIOObjectPropertyName, kCMIOObjectPropertyScopeGlobal, kCMIOObjectPropertyElementMain };
    CMIOObjectGetPropertyData(cids[i], &na, 0, NULL, nsize, &used, &name);
    if (running) printf("CAMERA     %s running\n", ((__bridge NSString *)name).UTF8String);
  }
}

// 3. Power assertions held by user apps (meeting apps keep the display awake).
static void probe_assertions(void) {
  CFDictionaryRef byPid = NULL;
  if (IOPMCopyAssertionsByProcess(&byPid) != kIOReturnSuccess || !byPid) return;
  NSDictionary *d = (__bridge_transfer NSDictionary *)byPid;
  for (NSNumber *pid in d) {
    NSRunningApplication *app = [NSRunningApplication runningApplicationWithProcessIdentifier:pid.intValue];
    if (!app) continue; // skip daemons like coreaudiod/powerd
    for (NSDictionary *a in d[pid]) {
      printf("ASSERTION  pid=%d bundle=%s type=%s name=%s\n", pid.intValue, app.bundleIdentifier.UTF8String,
             [a[@"AssertionTrueType"] ?: a[@"AssertType"] description].UTF8String, [a[@"AssertName"] description].UTF8String);
    }
  }
}

// 4. App-specific helper processes (Zoom spawns CptHost only inside a meeting).
static void probe_helpers(void) {
  for (NSRunningApplication *app in NSWorkspace.sharedWorkspace.runningApplications) {
    NSString *b = app.bundleIdentifier ?: @"";
    NSString *n = app.localizedName ?: @"";
    if ([n isEqualToString:@"CptHost"] || [b hasPrefix:@"us.zoom."] || [b hasPrefix:@"com.microsoft.teams"] ||
        [b hasPrefix:@"com.electron.lark"] || [b hasPrefix:@"com.larksuite"] || [b hasPrefix:@"com.tencent.meeting"] ||
        [b isEqualToString:@"com.apple.FaceTime"]) {
      printf("APP        pid=%d bundle=%s name=%s\n", app.processIdentifier, b.UTF8String, n.UTF8String);
    }
  }
}

int main(int argc, char **argv) {
  BOOL loop = argc > 1 && strcmp(argv[1], "--loop") == 0;
  do { @autoreleasepool {
    printf("===== %s\n", [NSDate date].description.UTF8String);
    probe_processes(); probe_devices(); probe_assertions(); probe_helpers();
    fflush(stdout);
    if (loop) sleep(3);
  } } while (loop);
  return 0;
}
