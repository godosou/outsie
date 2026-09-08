#import "../native/work_console.m"
#include <assert.h>
@interface FixtureApp : NSObject
@property NSString *bundleIdentifier;
@property NSURL *bundleURL;
@end
@implementation FixtureApp
@end
int main(int argc, const char **argv) {
  @autoreleasepool {
    assert(argc == 2);
    [NSApplication sharedApplication];
    NSString *root = [NSString stringWithUTF8String:argv[1]];
    NSFileManager *fm = NSFileManager.defaultManager;
    for (NSString *name in @[@"First.app", @"Second.app"]) {
      NSString *contents = [[root stringByAppendingPathComponent:name] stringByAppendingPathComponent:@"Contents"];
      assert([fm createDirectoryAtPath:contents withIntermediateDirectories:YES attributes:nil error:nil]);
      assert(([@{@"CFBundleIdentifier":@"test.same.identifier", @"CFBundleName":@"Fixture", @"CFBundlePackageType":@"APPL"} writeToFile:[contents stringByAppendingPathComponent:@"Info.plist"] atomically:YES]));
    }
    NSURL *first = [NSURL fileURLWithPath:[root stringByAppendingPathComponent:@"First.app"]];
    NSURL *second = [NSURL fileURLWithPath:[root stringByAppendingPathComponent:@"Second.app"]];
    assert(consoleAppRecord(first, NO));
    assert(!consoleAppRecord([NSURL fileURLWithPath:root], NO));
    assert(consoleChosenURL(first.path.UTF8String, @"test.same.identifier"));
    assert(!consoleChosenURL(first.path.UTF8String, @"wrong.identifier"));
    assert(!consoleChosenURL("relative.app", @"test.same.identifier"));
    FixtureApp *candidate = [FixtureApp new];
    candidate.bundleIdentifier = @"test.same.identifier";
    candidate.bundleURL = second;
    assert(!consoleMatchesTarget((NSRunningApplication *)candidate, @"test.same.identifier", first));
    assert(consoleMatchesTarget((NSRunningApplication *)candidate, @"test.same.identifier", second));
    char *json = repose_console_list_apps("[]");
    assert(json);
    NSArray *apps = [NSJSONSerialization JSONObjectWithData:[[NSString stringWithUTF8String:json] dataUsingEncoding:NSUTF8StringEncoding] options:0 error:nil];
    assert(apps.count > 0);
    BOOL found = NO;
    for (NSDictionary *app in apps) {
      assert([app[@"path"] hasPrefix:@"/"]);
      assert([app[@"icon"] hasPrefix:@"data:image/png;base64,"]);
      if ([app[@"bundleId"] isEqual:@"com.apple.Terminal"]) found = YES;
    }
    assert(found);
    repose_console_free_json(json);
    printf("Native App catalog, icons and exact-path matching passed (%lu apps).\n", (unsigned long)apps.count);
  }
}
