#import <AppKit/AppKit.h>
#import <ApplicationServices/ApplicationServices.h>
#import <Carbon/Carbon.h>
#include <stdbool.h>

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

// Called only on the execution worker, never from the UI thread.
typedef bool (*ReposeConsoleGuard)(void *context);

int repose_console_activate(const char *bundle, ReposeConsoleGuard valid, void *context) {
  NSString *identifier = [NSString stringWithUTF8String:bundle];
  __block int result = 2;
  onMainSync(^{
    if (!valid(context) || !sessionAvailable()) return;
    NSWorkspace *workspace = [NSWorkspace sharedWorkspace];
    NSURL *url = [workspace URLForApplicationWithBundleIdentifier:identifier];
    if (!url) { result = 1; return; }
    NSRunningApplication *target = [[NSRunningApplication runningApplicationsWithBundleIdentifier:identifier] firstObject];
    if (!target) {
      NSError *error = nil;
#pragma clang diagnostic push
#pragma clang diagnostic ignored "-Wdeprecated-declarations"
      target = [workspace launchApplicationAtURL:url options:NSWorkspaceLaunchWithoutActivation configuration:@{} error:&error];
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
    onMainSync(^{ front = sessionAvailable() && [[[NSWorkspace sharedWorkspace] frontmostApplication].bundleIdentifier isEqualToString:identifier]; });
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

int repose_console_key(const char *bundle,const char *keyValue,unsigned int modifiers, ReposeConsoleGuard valid, void *context) {
  if (!repose_console_trusted(false)) return 1;
  NSString *identifier=[NSString stringWithUTF8String:bundle];
  NSString *key=[NSString stringWithUTF8String:keyValue];
  __block int result=2;
  onMainSync(^{
    NSRunningApplication *target=[[NSWorkspace sharedWorkspace] frontmostApplication];
    if (!valid(context) || !sessionAvailable() || ![target.bundleIdentifier isEqualToString:identifier]) return;
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
