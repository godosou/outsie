#include <CoreFoundation/CoreFoundation.h>
#include <Security/Security.h>
#include <bsm/audit.h>
#include <bsm/libbsm.h>
#include <limits.h>
#include <stdint.h>
#include <stdlib.h>
#include <sys/socket.h>
#include <sys/un.h>

struct repose_peer_verifier {
    SecRequirementRef requirement;
};

struct repose_peer_claims {
    uint32_t effective_uid;
    uint32_t audit_session_id;
    int32_t process_id;
    uint32_t process_version;
};

void *repose_peer_verifier_create(const uint8_t *requirement_text, size_t length) {
    if (requirement_text == NULL || length == 0 || length > (size_t)LONG_MAX) {
        return NULL;
    }
    CFStringRef text = CFStringCreateWithBytes(kCFAllocatorDefault,
                                               requirement_text,
                                               (CFIndex)length,
                                               kCFStringEncodingUTF8,
                                               false);
    if (text == NULL) {
        return NULL;
    }
    SecRequirementRef requirement = NULL;
    OSStatus status = SecRequirementCreateWithString(text,
                                                     kSecCSDefaultFlags,
                                                     &requirement);
    CFRelease(text);
    if (status != errSecSuccess || requirement == NULL) {
        if (requirement != NULL) {
            CFRelease(requirement);
        }
        return NULL;
    }
    struct repose_peer_verifier *verifier = calloc(1, sizeof(*verifier));
    if (verifier == NULL) {
        CFRelease(requirement);
        return NULL;
    }
    verifier->requirement = requirement;
    return verifier;
}

int repose_peer_verifier_verify(void *opaque,
                                int socket_fd,
                                struct repose_peer_claims *out_claims) {
    if (opaque == NULL || socket_fd < 0 || out_claims == NULL) {
        return -1;
    }
    struct repose_peer_verifier *verifier = opaque;
    audit_token_t token;
    socklen_t token_length = (socklen_t)sizeof(token);
    if (getsockopt(socket_fd,
                   SOL_LOCAL,
                   LOCAL_PEERTOKEN,
                   &token,
                   &token_length) != 0 ||
        token_length != (socklen_t)sizeof(token)) {
        return -2;
    }

    uid_t effective_uid = audit_token_to_euid(token);
    au_asid_t audit_session_id = audit_token_to_asid(token);
    pid_t process_id = audit_token_to_pid(token);
    int process_version = audit_token_to_pidversion(token);
    if (effective_uid != 0 || audit_session_id <= AU_DEFAUDITSID ||
        process_id <= 0 || process_version < 0) {
        return -3;
    }

    CFDataRef audit_data = CFDataCreate(kCFAllocatorDefault,
                                        (const UInt8 *)&token,
                                        (CFIndex)sizeof(token));
    if (audit_data == NULL) {
        return -4;
    }
    const void *keys[] = {kSecGuestAttributeAudit};
    const void *values[] = {audit_data};
    CFDictionaryRef attributes = CFDictionaryCreate(
        kCFAllocatorDefault,
        keys,
        values,
        1,
        &kCFTypeDictionaryKeyCallBacks,
        &kCFTypeDictionaryValueCallBacks);
    if (attributes == NULL) {
        CFRelease(audit_data);
        return -5;
    }

    SecCodeRef code = NULL;
    OSStatus status = SecCodeCopyGuestWithAttributes(NULL,
                                                     attributes,
                                                     kSecCSDefaultFlags,
                                                     &code);
    CFRelease(attributes);
    CFRelease(audit_data);
    if (status != errSecSuccess || code == NULL) {
        if (code != NULL) {
            CFRelease(code);
        }
        return -6;
    }
    status = SecCodeCheckValidity(code,
                                  kSecCSDefaultFlags,
                                  verifier->requirement);
    CFRelease(code);
    if (status != errSecSuccess) {
        return -7;
    }

    struct repose_peer_claims claims = {
        .effective_uid = (uint32_t)effective_uid,
        .audit_session_id = (uint32_t)audit_session_id,
        .process_id = (int32_t)process_id,
        .process_version = (uint32_t)process_version,
    };
    *out_claims = claims;
    return 0;
}

void repose_peer_verifier_destroy(void *opaque) {
    if (opaque == NULL) {
        return;
    }
    struct repose_peer_verifier *verifier = opaque;
    if (verifier->requirement != NULL) {
        CFRelease(verifier->requirement);
        verifier->requirement = NULL;
    }
    free(verifier);
}
