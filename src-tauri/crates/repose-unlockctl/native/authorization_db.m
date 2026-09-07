#include <CoreFoundation/CoreFoundation.h>
#include <Security/Authorization.h>
#include <Security/AuthorizationDB.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>

#ifndef REPOSE_AUTHDB_PRODUCTION_MUTATION_ENABLED
#define REPOSE_AUTHDB_PRODUCTION_MUTATION_ENABLED 0
#endif

// These are the only two rights this translation unit can name. The boot/login
// authorization surface is intentionally outside this adapter.
static const char kScreenSaverRight[] = "system.login.screensaver";
static const char kNamedReposeRule[] = "ai.repose.unlock";
static const uint8_t kNamedReposeDefinition[] =
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>"
    "<plist version=\"1.0\"><dict>"
    "<key>class</key><string>evaluate-mechanisms</string>"
    "<key>mechanisms</key><array>"
    "<string>ReposeUnlock:unlock,privileged</string>"
    "</array></dict></plist>";
static const size_t kMaximumDefinitionBytes = 1024U * 1024U;

enum {
    kReposeAdapterInvalid = -70001,
    kReposeAdapterTooLarge = -70002,
    kReposeAdapterAllocation = -70003,
    kReposeAdapterSerialization = -70004,
    kReposeAdapterWrongType = -70005,
    kReposeAdapterProductionGateClosed = -70006,
};

static int CopyRight(const char *name,
                     int missing_is_success,
                     uint8_t **bytes_out,
                     size_t *length_out,
                     int *found_out) {
    if (bytes_out == NULL || length_out == NULL || found_out == NULL) {
        return kReposeAdapterInvalid;
    }
    *bytes_out = NULL;
    *length_out = 0;
    *found_out = 0;

    CFDictionaryRef definition = NULL;
    OSStatus status = AuthorizationRightGet(name, &definition);
    if (status != errAuthorizationSuccess) {
        if (missing_is_success && status == errAuthorizationDenied) {
            return 0;
        }
        return (int)status;
    }
    if (definition == NULL ||
        CFGetTypeID(definition) != CFDictionaryGetTypeID()) {
        if (definition != NULL) {
            CFRelease(definition);
        }
        return kReposeAdapterWrongType;
    }

    CFErrorRef serialization_error = NULL;
    CFDataRef data = CFPropertyListCreateData(kCFAllocatorDefault,
                                               definition,
                                               kCFPropertyListBinaryFormat_v1_0,
                                               0,
                                               &serialization_error);
    CFRelease(definition);
    if (serialization_error != NULL) {
        CFRelease(serialization_error);
    }
    if (data == NULL) {
        return kReposeAdapterSerialization;
    }
    CFIndex length = CFDataGetLength(data);
    if (length < 0 || (size_t)length > kMaximumDefinitionBytes) {
        CFRelease(data);
        return kReposeAdapterTooLarge;
    }
    uint8_t *copy = NULL;
    if (length > 0) {
        copy = malloc((size_t)length);
        if (copy == NULL) {
            CFRelease(data);
            return kReposeAdapterAllocation;
        }
        memcpy(copy, CFDataGetBytePtr(data), (size_t)length);
    }
    CFRelease(data);
    *bytes_out = copy;
    *length_out = (size_t)length;
    *found_out = 1;
    return 0;
}

static int SetRight(const char *name, const uint8_t *bytes, size_t length) {
    if (!REPOSE_AUTHDB_PRODUCTION_MUTATION_ENABLED) {
        return kReposeAdapterProductionGateClosed;
    }
    if (geteuid() != 0) {
        return (int)errAuthorizationDenied;
    }
    if ((bytes == NULL && length != 0) || length == 0 ||
        length > kMaximumDefinitionBytes) {
        return kReposeAdapterInvalid;
    }
    CFDataRef data = CFDataCreate(kCFAllocatorDefault, bytes, (CFIndex)length);
    if (data == NULL) {
        return kReposeAdapterAllocation;
    }
    CFErrorRef parse_error = NULL;
    CFPropertyListRef value = CFPropertyListCreateWithData(kCFAllocatorDefault,
                                                           data,
                                                           kCFPropertyListImmutable,
                                                           NULL,
                                                           &parse_error);
    CFRelease(data);
    if (parse_error != NULL) {
        CFRelease(parse_error);
    }
    if (value == NULL) {
        return kReposeAdapterSerialization;
    }
    if (CFGetTypeID(value) != CFDictionaryGetTypeID()) {
        CFRelease(value);
        return kReposeAdapterWrongType;
    }

    AuthorizationRef authorization = NULL;
    OSStatus status = AuthorizationCreate(NULL,
                                           kAuthorizationEmptyEnvironment,
                                           kAuthorizationFlagDefaults,
                                           &authorization);
    if (status == errAuthorizationSuccess && authorization != NULL) {
        status = AuthorizationRightSet(authorization,
                                       name,
                                       value,
                                       NULL,
                                       NULL,
                                       NULL);
    } else if (status == errAuthorizationSuccess) {
        status = errAuthorizationInternal;
    }
    if (authorization != NULL) {
        AuthorizationFree(authorization, kAuthorizationFlagDefaults);
    }
    CFRelease(value);
    return (int)status;
}

int repose_authdb_copy_screensaver(uint8_t **bytes_out,
                                   size_t *length_out,
                                   int *found_out) {
    return CopyRight(kScreenSaverRight, 0, bytes_out, length_out, found_out);
}

int repose_authdb_set_screensaver(const uint8_t *bytes, size_t length) {
    return SetRight(kScreenSaverRight, bytes, length);
}

int repose_authdb_copy_named_v1(uint8_t **bytes_out,
                                size_t *length_out,
                                int *found_out) {
    return CopyRight(kNamedReposeRule, 1, bytes_out, length_out, found_out);
}

int repose_authdb_set_named_v1(void) {
    return SetRight(kNamedReposeRule,
                    kNamedReposeDefinition,
                    sizeof(kNamedReposeDefinition) - 1U);
}

int repose_authdb_remove_named_v1(void) {
    if (!REPOSE_AUTHDB_PRODUCTION_MUTATION_ENABLED) {
        return kReposeAdapterProductionGateClosed;
    }
    if (geteuid() != 0) {
        return (int)errAuthorizationDenied;
    }
    AuthorizationRef authorization = NULL;
    OSStatus status = AuthorizationCreate(NULL,
                                           kAuthorizationEmptyEnvironment,
                                           kAuthorizationFlagDefaults,
                                           &authorization);
    if (status == errAuthorizationSuccess && authorization != NULL) {
        status = AuthorizationRightRemove(authorization, kNamedReposeRule);
    } else if (status == errAuthorizationSuccess) {
        status = errAuthorizationInternal;
    }
    if (authorization != NULL) {
        AuthorizationFree(authorization, kAuthorizationFlagDefaults);
    }
    return (int)status;
}

void repose_authdb_free(void *bytes) {
    free(bytes);
}
