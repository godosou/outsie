/*
 * repose_peer_verify.c -- see repose_peer_verify.h.
 *
 * Derived from the prior-art
 *   src-tauri/crates/repose-unlock-service/native/peer_identity.c
 * on branch codex/phone-proximity-unlock, generalised from "root only" to the
 * two-host {0, 92} allowlist the spike needs, with each uid bound to its own
 * designated requirement.
 *
 * Divergence from the prior art, deliberate: the prior art also hard-rejected
 * peers whose audit session id was <= AU_DEFAUDITSID. That check is dropped
 * here. The code-signing identity check (SecCodeCheckValidity against the exact
 * Apple-signed identifier) already proves the peer is the real host; the audit
 * session id adds no security beyond it and is the kind of thing Apple can
 * quietly change how they populate across releases -- keeping it only buys
 * false denials. euid + designated requirement are the hard gates.
 *
 * Build: clang ... -framework CoreFoundation -framework Security -lbsm
 */
#include "repose_peer_verify.h"

#include <CoreFoundation/CoreFoundation.h>
#include <Security/Security.h>       /* SecRequirementCreateWithString, errSec* */
#include <Security/SecCode.h>        /* SecCodeCopyGuestWithAttributes, kSecGuestAttributeAudit */
#include <Security/SecRequirement.h>
#include <bsm/audit.h>              /* audit_token_t */
#include <bsm/libbsm.h>            /* audit_token_to_euid / _to_pid */
#include <stdlib.h>
#include <sys/socket.h>
#include <sys/un.h>                /* SOL_LOCAL, LOCAL_PEERTOKEN */

/* The two pinned designated requirements. Verified on this machine with:
 *   codesign -dr- <path-to-authorizationhost>
 *   codesign -dr- <path-to-SecurityAgent>
 * Both print exactly these strings. */
#define REQ_AUTHORIZATIONHOST \
    "identifier \"com.apple.authorizationhost\" and anchor apple"
#define REQ_SECURITYAGENT \
    "identifier \"com.apple.SecurityAgent\" and anchor apple"

struct repose_peer_verifier {
    SecRequirementRef req_root;   /* matches euid 0  -> authorizationhost */
    SecRequirementRef req_agent;  /* matches euid 92 -> SecurityAgent     */
};

static SecRequirementRef make_requirement(const char *text)
{
    CFStringRef s = CFStringCreateWithCString(kCFAllocatorDefault, text,
                                              kCFStringEncodingUTF8);
    if (s == NULL) {
        return NULL;
    }
    SecRequirementRef req = NULL;
    OSStatus status = SecRequirementCreateWithString(s, kSecCSDefaultFlags, &req);
    CFRelease(s);
    if (status != errSecSuccess) {
        if (req != NULL) {
            CFRelease(req);
        }
        return NULL;
    }
    return req;
}

repose_peer_verifier_t *repose_peer_verifier_create(void)
{
    repose_peer_verifier_t *v = calloc(1, sizeof(*v));
    if (v == NULL) {
        return NULL;
    }
    v->req_root = make_requirement(REQ_AUTHORIZATIONHOST);
    v->req_agent = make_requirement(REQ_SECURITYAGENT);
    if (v->req_root == NULL || v->req_agent == NULL) {
        repose_peer_verifier_destroy(v);
        return NULL;
    }
    return v;
}

void repose_peer_verifier_destroy(repose_peer_verifier_t *v)
{
    if (v == NULL) {
        return;
    }
    if (v->req_root != NULL) {
        CFRelease(v->req_root);
    }
    if (v->req_agent != NULL) {
        CFRelease(v->req_agent);
    }
    free(v);
}

int repose_peer_verifier_check(const repose_peer_verifier_t *v, int fd,
                               uid_t *out_euid, pid_t *out_pid)
{
    if (v == NULL || fd < 0) {
        return -1;
    }

    /* (a)/(b): the peer's audit token. LOCAL_PEERTOKEN yields an audit_token_t
     * that identifies the exact process (pid + pid-generation), immune to the
     * pid-reuse race that plagues pid-only identification. It also carries the
     * euid, so this one call gives us both the uid and the handle we need for
     * the code-signing check below -- no separate getpeereid() required, though
     * getpeereid(fd,&euid,&egid) would give the same euid if preferred. */
    audit_token_t token;
    socklen_t token_len = (socklen_t)sizeof(token);
    if (getsockopt(fd, SOL_LOCAL, LOCAL_PEERTOKEN, &token, &token_len) != 0 ||
        token_len != (socklen_t)sizeof(token)) {
        return -2;
    }

    uid_t euid = audit_token_to_euid(token);
    pid_t pid = audit_token_to_pid(token);
    if (pid <= 0) {
        return -3;
    }

    /* Pick the requirement that must match THIS uid. Any uid outside {0,92} is
     * rejected outright -- no host runs the mechanism as anything else. */
    SecRequirementRef requirement;
    if (euid == 0) {            /* authorizationhost, privileged variant   */
        requirement = v->req_root;
    } else if (euid == 92) {    /* SecurityAgent, non-privileged variant   */
        requirement = v->req_agent;
    } else {
        return -4;
    }

    /* Turn the audit token into a SecCode for the live peer process. */
    CFDataRef audit_data = CFDataCreate(kCFAllocatorDefault,
                                        (const UInt8 *)&token,
                                        (CFIndex)sizeof(token));
    if (audit_data == NULL) {
        return -5;
    }
    const void *keys[] = { kSecGuestAttributeAudit };
    const void *values[] = { audit_data };
    CFDictionaryRef attrs = CFDictionaryCreate(kCFAllocatorDefault, keys, values,
                                               1, &kCFTypeDictionaryKeyCallBacks,
                                               &kCFTypeDictionaryValueCallBacks);
    if (attrs == NULL) {
        CFRelease(audit_data);
        return -6;
    }
    SecCodeRef code = NULL;
    OSStatus status = SecCodeCopyGuestWithAttributes(NULL, attrs,
                                                     kSecCSDefaultFlags, &code);
    CFRelease(attrs);
    CFRelease(audit_data);
    if (status != errSecSuccess || code == NULL) {
        if (code != NULL) {
            CFRelease(code);
        }
        return -7;
    }

    /* The real gate: is this live process validly signed AND does it satisfy
     * the designated requirement for the identity its uid claims to be? */
    status = SecCodeCheckValidity(code, kSecCSDefaultFlags, requirement);
    CFRelease(code);
    if (status != errSecSuccess) {
        return -8;
    }

    if (out_euid != NULL) {
        *out_euid = euid;
    }
    if (out_pid != NULL) {
        *out_pid = pid;
    }
    return 0;
}
