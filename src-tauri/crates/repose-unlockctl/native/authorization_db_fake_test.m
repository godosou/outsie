#include <assert.h>
#include <stdio.h>

#define AuthorizationRightGet FakeAuthorizationRightGet
#define AuthorizationCreate FakeAuthorizationCreate
#define AuthorizationRightSet FakeAuthorizationRightSet
#define AuthorizationRightRemove FakeAuthorizationRightRemove
#define AuthorizationFree FakeAuthorizationFree
#define CFRelease FakeCFRelease
#define geteuid FakeGeteuid
#define REPOSE_AUTHDB_PRODUCTION_MUTATION_ENABLED 1
#include "authorization_db.m"
#undef AuthorizationRightGet
#undef AuthorizationCreate
#undef AuthorizationRightSet
#undef AuthorizationRightRemove
#undef AuthorizationFree
#undef CFRelease
#undef geteuid
#undef REPOSE_AUTHDB_PRODUCTION_MUTATION_ENABLED

extern void CFRelease(CFTypeRef value);

enum FakeGetMode {
    kFakeGetDictionary,
    kFakeGetMissing,
    kFakeGetError,
    kFakeGetWrongType,
    kFakeGetOversize,
};

static enum FakeGetMode gGetMode = kFakeGetDictionary;
static const char *gExpectedGetName = NULL;
static OSStatus gSetStatus = errAuthorizationSuccess;
static OSStatus gCreateStatus = errAuthorizationSuccess;
static int gCreateNull = 0;
static uid_t gEffectiveUid = 0;
static int gGetCalls = 0;
static int gCreateCalls = 0;
static int gSetCalls = 0;
static int gRemoveCalls = 0;
static int gFreeCalls = 0;
static int gReleaseCalls = 0;
static AuthorizationRef const kFakeAuthorization =
    (AuthorizationRef)(uintptr_t)0x1234U;

static CFDictionaryRef MakeDictionary(void) {
    const void *keys[] = {CFSTR("class")};
    const void *values[] = {CFSTR("rule")};
    return CFDictionaryCreate(kCFAllocatorDefault,
                              keys,
                              values,
                              1,
                              &kCFTypeDictionaryKeyCallBacks,
                              &kCFTypeDictionaryValueCallBacks);
}

static CFDictionaryRef MakeOversizeDictionary(void) {
    const CFIndex length = (CFIndex)(1024U * 1024U + 1U);
    UInt8 *bytes = calloc((size_t)length, 1U);
    assert(bytes != NULL);
    CFDataRef data = CFDataCreate(kCFAllocatorDefault, bytes, length);
    free(bytes);
    assert(data != NULL);
    const void *keys[] = {CFSTR("payload")};
    const void *values[] = {data};
    CFDictionaryRef dictionary = CFDictionaryCreate(
        kCFAllocatorDefault,
        keys,
        values,
        1,
        &kCFTypeDictionaryKeyCallBacks,
        &kCFTypeDictionaryValueCallBacks);
    CFRelease(data);
    return dictionary;
}

OSStatus FakeAuthorizationRightGet(const char *rightName,
                                   CFDictionaryRef *rightDefinition) {
    gGetCalls += 1;
    assert(gExpectedGetName != NULL);
    assert(strcmp(rightName, gExpectedGetName) == 0);
    assert(rightDefinition != NULL);
    switch (gGetMode) {
        case kFakeGetDictionary:
            *rightDefinition = MakeDictionary();
            return errAuthorizationSuccess;
        case kFakeGetMissing:
            *rightDefinition = NULL;
            return errAuthorizationDenied;
        case kFakeGetError:
            *rightDefinition = NULL;
            return errAuthorizationInternal;
        case kFakeGetWrongType:
            *rightDefinition = (CFDictionaryRef)CFRetain(CFSTR("wrong"));
            return errAuthorizationSuccess;
        case kFakeGetOversize:
            *rightDefinition = MakeOversizeDictionary();
            return errAuthorizationSuccess;
    }
    abort();
}

OSStatus FakeAuthorizationCreate(const AuthorizationRights *rights,
                                 const AuthorizationEnvironment *environment,
                                 AuthorizationFlags flags,
                                 AuthorizationRef *authorization) {
    gCreateCalls += 1;
    assert(rights == NULL);
    assert(environment == kAuthorizationEmptyEnvironment);
    assert(flags == kAuthorizationFlagDefaults);
    assert(authorization != NULL);
    *authorization = gCreateNull ? NULL : kFakeAuthorization;
    return gCreateStatus;
}

OSStatus FakeAuthorizationRightSet(AuthorizationRef authorization,
                                   const char *rightName,
                                   CFTypeRef rightDefinition,
                                   CFStringRef descriptionKey,
                                   CFBundleRef bundle,
                                   CFStringRef localeTableName) {
    gSetCalls += 1;
    assert(authorization == kFakeAuthorization);
    assert(descriptionKey == NULL);
    assert(bundle == NULL);
    assert(localeTableName == NULL);
    assert(CFGetTypeID(rightDefinition) == CFDictionaryGetTypeID());
    CFDictionaryRef dictionary = (CFDictionaryRef)rightDefinition;
    if (strcmp(rightName, "ai.repose.unlock") == 0) {
        assert(CFDictionaryGetCount(dictionary) == 2);
        CFStringRef className = CFDictionaryGetValue(dictionary, CFSTR("class"));
        assert(className != NULL);
        assert(CFEqual(className, CFSTR("evaluate-mechanisms")));
        CFArrayRef mechanisms = CFDictionaryGetValue(dictionary, CFSTR("mechanisms"));
        assert(mechanisms != NULL);
        assert(CFGetTypeID(mechanisms) == CFArrayGetTypeID());
        assert(CFArrayGetCount(mechanisms) == 1);
        assert(CFEqual(CFArrayGetValueAtIndex(mechanisms, 0),
                       CFSTR("ReposeUnlock:unlock,privileged")));
    } else {
        assert(strcmp(rightName, "system.login.screensaver") == 0);
    }
    return gSetStatus;
}

OSStatus FakeAuthorizationRightRemove(AuthorizationRef authorization,
                                      const char *rightName) {
    gRemoveCalls += 1;
    assert(authorization == kFakeAuthorization);
    assert(strcmp(rightName, "ai.repose.unlock") == 0);
    return gSetStatus;
}

OSStatus FakeAuthorizationFree(AuthorizationRef authorization,
                               AuthorizationFlags flags) {
    gFreeCalls += 1;
    assert(authorization == kFakeAuthorization);
    assert(flags == kAuthorizationFlagDefaults);
    return errAuthorizationSuccess;
}

void FakeCFRelease(CFTypeRef value) {
    gReleaseCalls += 1;
    CFRelease(value);
}

uid_t FakeGeteuid(void) {
    return gEffectiveUid;
}

static void TestInvalidAndErrorOutputs(void) {
    size_t length = 99U;
    int found = 99;
    assert(repose_authdb_copy_screensaver(NULL, &length, &found) ==
           kReposeAdapterInvalid);
    assert(gGetCalls == 0);

    uint8_t *bytes = (uint8_t *)(uintptr_t)0x1U;
    gExpectedGetName = "system.login.screensaver";
    gGetMode = kFakeGetError;
    assert(repose_authdb_copy_screensaver(&bytes, &length, &found) ==
           errAuthorizationInternal);
    assert(bytes == NULL);
    assert(length == 0U);
    assert(found == 0);
}

static void TestReads(void) {
    uint8_t *bytes = NULL;
    size_t length = 0U;
    int found = 0;
    gExpectedGetName = "ai.repose.unlock";
    gGetMode = kFakeGetMissing;
    assert(repose_authdb_copy_named_v1(&bytes, &length, &found) == 0);
    assert(bytes == NULL && length == 0U && found == 0);

    gExpectedGetName = "system.login.screensaver";
    gGetMode = kFakeGetDictionary;
    assert(repose_authdb_copy_screensaver(&bytes, &length, &found) == 0);
    assert(bytes != NULL && length > 0U && found == 1);
    repose_authdb_free(bytes);

    gGetMode = kFakeGetWrongType;
    assert(repose_authdb_copy_screensaver(&bytes, &length, &found) ==
           kReposeAdapterWrongType);
    assert(bytes == NULL && length == 0U && found == 0);

    gGetMode = kFakeGetOversize;
    assert(repose_authdb_copy_screensaver(&bytes, &length, &found) ==
           kReposeAdapterTooLarge);
    assert(bytes == NULL && length == 0U && found == 0);
}

static void TestFixedMutationsAndCleanup(void) {
    gEffectiveUid = 501;
    int creates = gCreateCalls;
    assert(repose_authdb_set_named_v1() == errAuthorizationDenied);
    assert(repose_authdb_remove_named_v1() == errAuthorizationDenied);
    assert(gCreateCalls == creates);

    gEffectiveUid = 0;
    gCreateStatus = errAuthorizationSuccess;
    gCreateNull = 0;
    gSetStatus = errAuthorizationSuccess;
    int frees = gFreeCalls;
    assert(repose_authdb_set_named_v1() == 0);
    assert(gSetCalls == 1);
    assert(gFreeCalls == frees + 1);

    const uint8_t arrayPlist[] =
        "<?xml version=\"1.0\"?><plist version=\"1.0\"><array/></plist>";
    assert(repose_authdb_set_screensaver(arrayPlist,
                                         sizeof(arrayPlist) - 1U) ==
           kReposeAdapterWrongType);

    gSetStatus = errAuthorizationInternal;
    frees = gFreeCalls;
    assert(repose_authdb_remove_named_v1() == errAuthorizationInternal);
    assert(gRemoveCalls == 1);
    assert(gFreeCalls == frees + 1);

    gSetStatus = errAuthorizationSuccess;
    gCreateNull = 1;
    frees = gFreeCalls;
    assert(repose_authdb_remove_named_v1() == errAuthorizationInternal);
    assert(gFreeCalls == frees);
    gCreateNull = 0;
}

int main(void) {
    TestInvalidAndErrorOutputs();
    TestReads();
    TestFixedMutationsAndCleanup();
    assert(gReleaseCalls > 0);
    return 0;
}
