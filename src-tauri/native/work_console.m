#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>
#import <Carbon/Carbon.h>
#include <stdbool.h>
#include <stdlib.h>
#include <string.h>
#import <UniformTypeIdentifiers/UniformTypeIdentifiers.h>

static void onMainSync(dispatch_block_t block) {
  if ([NSThread isMainThread]) block(); else dispatch_sync(dispatch_get_main_queue(), block);
}

static bool sessionAvailable(void) {
  NSDictionary *session = CFBridgingRelease(CGSessionCopyCurrentDictionary());
  return session && [session[(__bridge NSString *)kCGSessionOnConsoleKey] boolValue]
    && ![session[@"CGSSessionScreenIsLocked"] boolValue];
}

bool repose_console_trusted(bool prompt) {
  return AXIsProcessTrustedWithOptions((__bridge CFDictionaryRef)@{(__bridge NSString *)kAXTrustedCheckOptionPrompt: @(prompt)});
}

static NSURL *consoleCanonicalURL(NSURL *url) {
  return url.URLByResolvingSymlinksInPath.URLByStandardizingPath;
}

static BOOL consoleMatchesTarget(NSRunningApplication *target, NSString *identifier, NSURL *chosenURL) {
  return [target.bundleIdentifier isEqualToString:identifier] &&
    (!chosenURL || [consoleCanonicalURL(target.bundleURL) isEqual:chosenURL]);
}

static NSURL *consoleChosenURL(const char *path, NSString *identifier) {
  if (!path || !path[0]) return nil;
  NSString *value = [NSString stringWithUTF8String:path];
  if (!value.isAbsolutePath || ![value.pathExtension.lowercaseString isEqual:@"app"]) return nil;
  NSURL *url = consoleCanonicalURL([NSURL fileURLWithPath:value]);
  NSBundle *bundle = [NSBundle bundleWithURL:url];
  return [bundle.bundleIdentifier isEqualToString:identifier] && [bundle.infoDictionary[@"CFBundlePackageType"] isEqual:@"APPL"] ? url : nil;
}

// Called only on the execution worker, never from the UI thread.
typedef bool (*ReposeConsoleGuard)(void *context);

int repose_console_activate(const char *bundle, const char *path, ReposeConsoleGuard valid, void *context) {
  NSString *identifier = [NSString stringWithUTF8String:bundle];
  NSURL *chosenURL = consoleChosenURL(path, identifier);
  if (path && path[0] && !chosenURL) return 1;
  __block int result = 2;
  onMainSync(^{
    if (!valid(context) || !sessionAvailable()) return;
    NSWorkspace *workspace = [NSWorkspace sharedWorkspace];
    NSURL *url = chosenURL ?: [workspace URLForApplicationWithBundleIdentifier:identifier];
    if (!url) { result = 1; return; }
    NSRunningApplication *target = nil;
    for (NSRunningApplication *candidate in [NSRunningApplication runningApplicationsWithBundleIdentifier:identifier]) {
      if (consoleMatchesTarget(candidate, identifier, chosenURL)) { target = candidate; break; }
    }
    if (!target) {
      NSError *error = nil;
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
      target = [workspace launchApplicationAtURL:url options:(NSWorkspaceLaunchWithoutActivation | (chosenURL ? NSWorkspaceLaunchNewInstance : 0)) configuration:@{} error:&error];
#pragma clang diagnostic pop
      if (!target || error) return;
    }
    if (valid(context) && [target activateWithOptions:0]) result = 0;
  });
  if (result) return result;
  // Activation acknowledgement is asynchronous. Do not send keys before it settles.
  for (int attempt=0; attempt<60; attempt++) {
    if (!valid(context)) return 2;
    __block bool front = false;
    onMainSync(^{ front = sessionAvailable() && consoleMatchesTarget([[NSWorkspace sharedWorkspace] frontmostApplication], identifier, chosenURL); });
    if (front) return 0;
    [NSThread sleepForTimeInterval:0.025];
  }
  return 2;
}

static int namedCode(NSString *key) {
  NSDictionary *names = @{@"Enter":@36,@"Tab":@48,@"Space":@49,@"Backspace":@51,@"Escape":@53,
    @"Delete":@117,@"Home":@115,@"End":@119,@"PageUp":@116,@"PageDown":@121,
    @"ArrowLeft":@123,@"ArrowRight":@124,@"ArrowDown":@125,@"ArrowUp":@126,
    @"F1":@122,@"F2":@120,@"F3":@99,@"F4":@118,@"F5":@96,@"F6":@97,@"F7":@98,@"F8":@100,
    @"F9":@101,@"F10":@109,@"F11":@103,@"F12":@111,@"F13":@105,@"F14":@107,@"F15":@113,
    @"F16":@106,@"F17":@64,@"F18":@79,@"F19":@80,@"F20":@90};
  NSNumber *code=names[key];return code?code.intValue:-1;
}

// Resolve printable keys against the active hardware layout; punctuation such as
// tmux '%' receives Shift when required. Input-method text composition is excluded.
static int printableCode(NSString *key, bool *shift) {
  if (key.length != 1) return -1;
  TISInputSourceRef source=TISCopyCurrentKeyboardLayoutInputSource();
  if (!source) return -1;
  CFDataRef data=TISGetInputSourceProperty(source,kTISPropertyUnicodeKeyLayoutData);
  if (!data) {CFRelease(source);return -1;}
  const UCKeyboardLayout *layout=(const UCKeyboardLayout *)CFDataGetBytePtr(data);
  int result=-1;
  for (int useShift=0;useShift<2 && result<0;useShift++) {
    for (UInt16 code=0;code<128;code++) {
      UInt32 dead=0;UniChar chars[4];UniCharCount count=0;
      OSStatus status=UCKeyTranslate(layout,code,kUCKeyActionDown,useShift?(shiftKey>>8):0,LMGetKbdType(),kUCKeyTranslateNoDeadKeysBit,&dead,4,&count,chars);
      if (status==noErr && count==1 && chars[0]==[key characterAtIndex:0]) {result=code;*shift=*shift||useShift;break;}
    }
  }
  CFRelease(source);return result;
}

int repose_console_key(const char *bundle,const char *path,const char *keyValue,unsigned int modifiers, ReposeConsoleGuard valid, void *context) {
  if (!repose_console_trusted(false)) return 1;
  NSString *identifier=[NSString stringWithUTF8String:bundle];
  NSURL *chosenURL = consoleChosenURL(path, identifier);
  if (path && path[0] && !chosenURL) return 2;
  NSString *key=[NSString stringWithUTF8String:keyValue];
  __block int result=2;
  onMainSync(^{
    NSRunningApplication *target=[[NSWorkspace sharedWorkspace] frontmostApplication];
    if (!valid(context) || !sessionAvailable() || !consoleMatchesTarget(target, identifier, chosenURL)) return;
    bool shifted=(modifiers&8)!=0;
    int code=namedCode(key);
    if (code<0) code=printableCode(key,&shifted);
    if (code<0) {result=3;return;}
    CGEventFlags flags=0;
    if(modifiers&1) flags|=kCGEventFlagMaskCommand;
    if(modifiers&2) flags|=kCGEventFlagMaskControl;
    if(modifiers&4) flags|=kCGEventFlagMaskAlternate;
    if(shifted) flags|=kCGEventFlagMaskShift;
    CGEventRef down=CGEventCreateKeyboardEvent(NULL,(CGKeyCode)code,true);
    CGEventRef up=CGEventCreateKeyboardEvent(NULL,(CGKeyCode)code,false);
    if (!down || !up) {if(down)CFRelease(down);if(up)CFRelease(up);result=3;return;}
    CGEventSetFlags(down,flags);CGEventSetFlags(up,flags);
    // Target a PID as well as checking the foreground. Never route via a global tap.
    if (!valid(context)) {CFRelease(down);CFRelease(up);return;}
    CGEventPostToPid(target.processIdentifier,down);
    CGEventPostToPid(target.processIdentifier,up);
    CFRelease(down);CFRelease(up);result=0;
  });
  return result;
}

// Metadata discovery never loads executable code from an application bundle.
static NSDictionary *consoleAppRecord(NSURL *input, BOOL withIcon) {
  NSURL *url = consoleCanonicalURL(input);
  if (![url.pathExtension.lowercaseString isEqual:@"app"]) return nil;
  NSBundle *bundle = [NSBundle bundleWithURL:url];
  NSString *identifier = bundle.bundleIdentifier;
  if (!identifier.length || ![bundle.infoDictionary[@"CFBundlePackageType"] isEqual:@"APPL"]) return nil;
  NSString *name = [[NSFileManager defaultManager] displayNameAtPath:url.path];
  if ([name.pathExtension.lowercaseString isEqual:@"app"]) name = name.stringByDeletingPathExtension;
  NSMutableDictionary *record = [@{@"name":name ?: url.lastPathComponent, @"bundleId":identifier, @"path":url.path} mutableCopy];
  if (withIcon) onMainSync(^{
    NSImage *icon = [[NSWorkspace sharedWorkspace] iconForFile:url.path];
    NSBitmapImageRep *rep = [[NSBitmapImageRep alloc] initWithBitmapDataPlanes:NULL pixelsWide:48 pixelsHigh:48 bitsPerSample:8 samplesPerPixel:4 hasAlpha:YES isPlanar:NO colorSpaceName:NSDeviceRGBColorSpace bytesPerRow:0 bitsPerPixel:0];
    [NSGraphicsContext saveGraphicsState];
    NSGraphicsContext.currentContext = [NSGraphicsContext graphicsContextWithBitmapImageRep:rep];
    [icon drawInRect:NSMakeRect(0, 0, 48, 48) fromRect:NSZeroRect operation:NSCompositingOperationCopy fraction:1];
    [NSGraphicsContext restoreGraphicsState];
    NSData *png = [rep representationUsingType:NSBitmapImageFileTypePNG properties:@{}];
    if (png) record[@"icon"] = [@"data:image/png;base64," stringByAppendingString:[png base64EncodedStringWithOptions:0]];
  });
  return record;
}

static char *consoleJSON(id object) {
  NSData *data = [NSJSONSerialization dataWithJSONObject:object options:0 error:nil];
  return data ? strndup(data.bytes, data.length) : NULL;
}

char *repose_console_list_apps(const char *configured) {
  @autoreleasepool {
    NSMutableDictionary<NSString *, NSDictionary *> *records = [NSMutableDictionary dictionary];
    void (^add)(NSURL *) = ^(NSURL *url) {
      if (!url || records.count >= 1024) return;
      NSDictionary *record = consoleAppRecord(url, NO);
      if (record) records[record[@"path"]] = record;
    };
    NSArray *saved = [NSJSONSerialization JSONObjectWithData:[[NSString stringWithUTF8String:configured ?: "[]"] dataUsingEncoding:NSUTF8StringEncoding] options:0 error:nil];
    for (NSDictionary *app in saved) {
      if ([app[@"appPath"] isKindOfClass:NSString.class]) add([NSURL fileURLWithPath:app[@"appPath"]]);
      NSString *identifier = app[@"bundleId"];
      if ([identifier isKindOfClass:NSString.class]) onMainSync(^{ add([[NSWorkspace sharedWorkspace] URLForApplicationWithBundleIdentifier:identifier]); });
    }
    for (NSString *root in @[@"/Applications", [NSHomeDirectory() stringByAppendingPathComponent:@"Applications"], @"/System/Applications", @"/System/Library/CoreServices/Applications"]) {
      NSDirectoryEnumerator *enumerator = [[NSFileManager defaultManager] enumeratorAtURL:[NSURL fileURLWithPath:root] includingPropertiesForKeys:@[NSURLIsPackageKey] options:(NSDirectoryEnumerationSkipsHiddenFiles | NSDirectoryEnumerationSkipsPackageDescendants) errorHandler:^BOOL(__unused NSURL *url, __unused NSError *error) { return YES; }];
      NSUInteger visited = 0;
      for (NSURL *url in enumerator) {
        if (++visited > 10000 || records.count >= 1024) break;
        if (enumerator.level > 3) { [enumerator skipDescendants]; continue; }
        if ([url.pathExtension.lowercaseString isEqual:@"app"]) { add(url); [enumerator skipDescendants]; }
      }
    }
    NSArray *ordered = [records.allValues sortedArrayUsingComparator:^NSComparisonResult(NSDictionary *a, NSDictionary *b) {
      NSComparisonResult name = [a[@"name"] localizedStandardCompare:b[@"name"]];
      return name == NSOrderedSame ? [a[@"path"] compare:b[@"path"]] : name;
    }];
    NSMutableArray *result = [NSMutableArray array];
    for (NSDictionary *entry in ordered) {
      NSDictionary *full = consoleAppRecord([NSURL fileURLWithPath:entry[@"path"]], YES);
      if (full) [result addObject:full];
    }
    return consoleJSON(result);
  }
}

char *repose_console_pick_app(void) {
  __block NSDictionary *selection = @{@"app":NSNull.null};
  onMainSync(^{
    NSOpenPanel *panel = [NSOpenPanel openPanel];
    panel.title = @"选择要配置的 App";
    panel.message = @"选择 Mac 上的应用程序，Repose 会自动识别它。";
    panel.prompt = @"选择 App";
    panel.directoryURL = [NSURL fileURLWithPath:@"/Applications"];
    panel.allowedContentTypes = @[UTTypeApplicationBundle];
    panel.canChooseFiles = YES;
    panel.canChooseDirectories = NO;
    panel.treatsFilePackagesAsDirectories = NO;
    panel.allowsMultipleSelection = NO;
    if ([panel runModal] == NSModalResponseOK) {
      NSDictionary *app = consoleAppRecord(panel.URL, YES);
      selection = app ? @{@"app":app} : @{@"error":@"请选择有效的 Mac 应用程序。"};
    }
  });
  return consoleJSON(selection);
}

void repose_console_free_json(char *value) { free(value); }
