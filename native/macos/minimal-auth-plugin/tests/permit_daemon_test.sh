#!/bin/bash
#
# Sandbox tests for repose-permitd (the root consume-daemon, permit IPC design
# gap #3) and its client repose_permit_client. No VM, no lock screen, no real
# Apple hosts. See docs/plans/2026-09-09-permit-ipc-design.md sections 4, 5, 8.1.
#
# WHAT THIS EXERCISES
# -------------------
# The daemon and the mechanism talk a fixed 40-byte frame over a unix socket
# (native/macos/permit-daemon/repose_permit_wire.h). This test drives BOTH
# sides against scratch sockets in a temp dir:
#
#   - the daemon: a present presence-file yields ALLOW and is CONSUMED; a replay
#     against the same touch is DENIED; a newer touch re-authorises; absent /
#     stale / not-root-owned presence DENIES; a short or bad-magic request is
#     rejected and does NOT consume; a request that never closes its write half
#     is timed out (the daemon never wedges the lock screen); peer verification
#     rejects a non-Apple caller even at an allowed uid.
#
#   - the client (repose-permit-probe): a well-formed ALLOW reply is accepted; a
#     wrong-nonce or garbage reply is rejected; a missing server or a black-hole
#     server that never answers both DENY within a bounded time (never hangs).
#
# TWO TIERS, BY PRIVILEGE
# -----------------------
# Tier U (unprivileged) always runs: the client fail-closed / bounded matrix,
# and the daemon's own fail-closed startup guard (it must refuse to run non-root
# by default, i.e. with the test seam OFF).
#
# Tier R (the daemon's serve path -- present->ALLOW->consume, replay->DENY,
# stale->DENY, malformed->DENY, bounded timeout, peer-verify reject) now runs in
# BOTH privilege modes:
#   - as root: exactly as before, against a scratch socket + root-owned scratch
#     presence file, with --insecure-skip-peer-verify.
#   - unprivileged: via the daemon's OFF-by-default test seam
#     REPOSE_PERMIT_ALLOW_NONROOT=1, which relaxes ONLY the euid==0 startup guard
#     and the presence root-owner check (repose-permitd.c) so the same
#     consume/replay/stale/malformed matrix can be exercised in a sandbox. Every
#     other property -- the 40-byte frame validation, freshness/skew, the
#     single-consume watermark, nonce echo -- is unchanged and fully exercised.
# It is NOT a VM test -- it uses --insecure-skip-peer-verify on a scratch socket
# and a scratch presence file. The one case that stays root-only is the
# non-root-owned-presence DENY (R6): under the seam that check is deliberately
# relaxed, and an unprivileged runner cannot chown a file off itself anyway, so
# R6 is skipped without root.
#
# The end-to-end proof that the REAL SecurityAgent / authorizationhost peer is
# ACCEPTED (SecCodeCheckValidity against the pinned designated requirement) can
# only happen at a live lock screen and stays in design section 8.2 (the VM).
# Tier R here proves the complementary half: an allowed uid that is NOT the
# signed host is DENIED.

set -uo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DAEMON_DIR="$(cd "${HERE}/../../permit-daemon" 2>/dev/null && pwd || true)"

pass=0; fail=0; skip=0
ok()   { printf '  ok   %s\n' "$1"; pass=$((pass+1)); }
bad()  { printf '  FAIL %s\n' "$1"; fail=$((fail+1)); }
skp()  { printf '  skip %s\n' "$1"; skip=$((skip+1)); }

# 40-byte request wire constants (mirror repose_permit_wire.h). Hex fixtures for
# the malformed-frame cases fed to the real daemon.
#   valid 8-byte header = magic 'R','P','U','1' (52 50 55 31) version 01
#                         op 01 (request) verdict 00 reserved 00
GOOD_HEADER_HEX="5250553101010000"                  # 8 bytes: a well-formed header
SHORT_FRAME_HEX="${GOOD_HEADER_HEX}0000"            # 10 bytes: header + 2 nonce bytes, truncated
BADMAGIC_FRAME_HEX="$(printf '00%.0s' $(seq 1 40))" # 40 zero bytes: full length, wrong magic

TMP=""
MOCKD_PID=""
cleanup() {
    # Stop any daemon we launched (matched on our unique temp path) and any mock.
    [ -n "${MOCKD_PID}" ] && kill "${MOCKD_PID}" 2>/dev/null
    if [ -n "${TMP}" ]; then
        pkill -TERM -f "repose-permitd --socket ${TMP}" 2>/dev/null
        sleep 0.2
        rm -rf "${TMP}" 2>/dev/null
    fi
}
trap cleanup EXIT

echo "repose-permitd + client sandbox test"

# ---- preconditions ------------------------------------------------------- #
if ! command -v clang >/dev/null 2>&1; then
    skp "clang not available -- cannot build the daemon or helpers"
    echo; echo "${pass} passed, ${fail} failed, ${skip} skipped"
    exit 0
fi
if [ -z "${DAEMON_DIR}" ] || [ ! -f "${DAEMON_DIR}/repose-permitd.c" ]; then
    skp "permit-daemon sources not found (${HERE}/../../permit-daemon) -- nothing to test yet"
    echo; echo "${pass} passed, ${fail} failed, ${skip} skipped"
    exit 0
fi

# ---- build the daemon + probe -------------------------------------------- #
BUILD_LOG="$(mktemp "${TMPDIR:-/tmp}/permitd-build.XXXXXX.log")"
if ! ( cd "${DAEMON_DIR}" && ./build.sh ) >"${BUILD_LOG}" 2>&1; then
    bad "daemon build.sh failed -- see ${BUILD_LOG}"
    sed 's/^/      /' "${BUILD_LOG}"
    echo; echo "${pass} passed, ${fail} failed, ${skip} skipped"
    exit 1
fi
rm -f "${BUILD_LOG}"
DAEMON="${DAEMON_DIR}/build/repose-permitd"
PROBE="${DAEMON_DIR}/build/repose-permit-probe"
if [ ! -x "${DAEMON}" ] || [ ! -x "${PROBE}" ]; then
    bad "daemon build produced no repose-permitd / repose-permit-probe"
    echo; echo "${pass} passed, ${fail} failed, ${skip} skipped"
    exit 1
fi
ok "daemon and probe build with clang alone (no Rust, no python)"

TMP="$(mktemp -d "${TMPDIR:-/tmp}/permitd-test.XXXXXX")"
BIN="${TMP}/bin"; mkdir -p "${BIN}"

# ---- two tiny helpers, compiled against the REAL wire header ------------- #
# mockd: stand in for the daemon so the CLIENT's fail-closed logic can be tested
# unprivileged. Modes: allow | deny | wrongnonce | garbage | blackhole.
cat > "${TMP}/mockd.c" <<'EOF'
#include "repose_permit_wire.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <sys/un.h>

/* Best-effort read of the client's 40-byte request; capture its nonce. */
static void drain(int fd, unsigned char *nonce)
{
    unsigned char buf[REPOSE_PERMIT_FRAME_LEN];
    size_t got = 0;
    struct timeval tv = { 1, 0 };
    setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof tv);
    while (got < sizeof buf) {
        ssize_t r = read(fd, buf + got, sizeof buf - got);
        if (r <= 0) break;
        got += (size_t)r;
    }
    memset(nonce, 0, REPOSE_PERMIT_NONCE_LEN);
    if (got >= REPOSE_PERMIT_FRAME_LEN)
        memcpy(nonce, buf + REPOSE_PERMIT_OFF_NONCE, REPOSE_PERMIT_NONCE_LEN);
}

int main(int argc, char **argv)
{
    if (argc < 3) { fprintf(stderr, "usage: mockd <sock> <mode>\n"); return 2; }
    const char *path = argv[1], *mode = argv[2];
    unlink(path);
    int lf = socket(AF_UNIX, SOCK_STREAM, 0);
    if (lf < 0) { perror("socket"); return 1; }
    struct sockaddr_un a; memset(&a, 0, sizeof a);
    a.sun_family = AF_UNIX; strncpy(a.sun_path, path, sizeof a.sun_path - 1);
    if (bind(lf, (struct sockaddr *)&a, sizeof a) != 0) { perror("bind"); return 1; }
    if (listen(lf, 16) != 0) { perror("listen"); return 1; }
    for (;;) {
        int cf = accept(lf, NULL, NULL);
        if (cf < 0) continue;
        unsigned char nonce[REPOSE_PERMIT_NONCE_LEN];
        drain(cf, nonce);
        if (strcmp(mode, "blackhole") == 0) { sleep(2); close(cf); continue; }
        unsigned char r[REPOSE_PERMIT_FRAME_LEN]; memset(r, 0, sizeof r);
        if (strcmp(mode, "garbage") == 0) {
            for (size_t i = 0; i < sizeof r; i++) r[i] = (unsigned char)(i * 7 + 1);
        } else {
            r[REPOSE_PERMIT_OFF_MAGIC + 0] = REPOSE_PERMIT_MAGIC0;
            r[REPOSE_PERMIT_OFF_MAGIC + 1] = REPOSE_PERMIT_MAGIC1;
            r[REPOSE_PERMIT_OFF_MAGIC + 2] = REPOSE_PERMIT_MAGIC2;
            r[REPOSE_PERMIT_OFF_MAGIC + 3] = REPOSE_PERMIT_MAGIC3;
            r[REPOSE_PERMIT_OFF_VERSION] = REPOSE_PERMIT_VERSION;
            r[REPOSE_PERMIT_OFF_OP] = REPOSE_PERMIT_OP_VERDICT;
            memcpy(r + REPOSE_PERMIT_OFF_NONCE, nonce, REPOSE_PERMIT_NONCE_LEN);
            if (strcmp(mode, "allow") == 0) {
                r[REPOSE_PERMIT_OFF_VERDICT] = REPOSE_PERMIT_VERDICT_ALLOW;
            } else if (strcmp(mode, "deny") == 0) {
                r[REPOSE_PERMIT_OFF_VERDICT] = REPOSE_PERMIT_VERDICT_DENY;
            } else if (strcmp(mode, "wrongnonce") == 0) {
                r[REPOSE_PERMIT_OFF_VERDICT] = REPOSE_PERMIT_VERDICT_ALLOW;
                r[REPOSE_PERMIT_OFF_NONCE] ^= 0xFF; /* verdict says allow, nonce lies */
            }
        }
        (void)write(cf, r, sizeof r);
        close(cf);
    }
}
EOF

# framesend: send arbitrary raw bytes (hex) to a socket, optionally half-close
# the write side, then read one verdict frame under a timeout. Used to feed the
# real daemon malformed / truncated / never-closed requests.
cat > "${TMP}/framesend.c" <<'EOF'
#include "repose_permit_wire.h"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <sys/un.h>

static int hexval(int c)
{
    if (c >= '0' && c <= '9') return c - '0';
    if (c >= 'a' && c <= 'f') return c - 'a' + 10;
    if (c >= 'A' && c <= 'F') return c - 'A' + 10;
    return -1;
}

int main(int argc, char **argv)
{
    if (argc < 4) { fprintf(stderr, "usage: framesend <sock> <hex> <shutdown:0|1>\n"); return 2; }
    const char *path = argv[1], *hex = argv[2];
    int doshut = atoi(argv[3]);
    unsigned char buf[256]; size_t n = 0;
    for (size_t i = 0; hex[i] && hex[i + 1] && n < sizeof buf; i += 2) {
        int hi = hexval(hex[i]), lo = hexval(hex[i + 1]);
        if (hi < 0 || lo < 0) break;
        buf[n++] = (unsigned char)((hi << 4) | lo);
    }
    int fd = socket(AF_UNIX, SOCK_STREAM, 0);
    if (fd < 0) { perror("socket"); return 1; }
    struct sockaddr_un a; memset(&a, 0, sizeof a);
    a.sun_family = AF_UNIX; strncpy(a.sun_path, path, sizeof a.sun_path - 1);
    if (connect(fd, (struct sockaddr *)&a, sizeof a) != 0) { printf("DENY(connect)\n"); return 1; }
    struct timeval tv = { 3, 0 };
    setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof tv);
    if (n) (void)write(fd, buf, n);
    if (doshut) shutdown(fd, SHUT_WR);
    unsigned char resp[REPOSE_PERMIT_FRAME_LEN]; size_t got = 0;
    while (got < sizeof resp) {
        ssize_t r = read(fd, resp + got, sizeof resp - got);
        if (r <= 0) break;
        got += (size_t)r;
    }
    close(fd);
    if (got == REPOSE_PERMIT_FRAME_LEN &&
        resp[REPOSE_PERMIT_OFF_OP] == REPOSE_PERMIT_OP_VERDICT &&
        resp[REPOSE_PERMIT_OFF_VERDICT] == REPOSE_PERMIT_VERDICT_ALLOW) {
        printf("ALLOW\n"); return 0;
    }
    printf("DENY\n"); return 1;
}
EOF

if ! clang -O1 -Wall -Wextra -Werror -I"${DAEMON_DIR}" -o "${BIN}/mockd" "${TMP}/mockd.c" 2>"${TMP}/cc.log" \
   || ! clang -O1 -Wall -Wextra -Werror -I"${DAEMON_DIR}" -o "${BIN}/framesend" "${TMP}/framesend.c" 2>>"${TMP}/cc.log"; then
    bad "could not compile test helpers"
    sed 's/^/      /' "${TMP}/cc.log"
    echo; echo "${pass} passed, ${fail} failed, ${skip} skipped"
    exit 1
fi
MOCKD="${BIN}/mockd"; FRAMESEND="${BIN}/framesend"

# Run "$@" but hard-kill it after LIMIT seconds. Returns the command's own exit
# status, or 137 if the watchdog had to kill it (i.e. it hung). The whole point
# of the bounded cases is that this never returns 137.
run_bounded() {
    local limit="$1"; shift
    "$@" & local p=$!
    ( sleep "${limit}"; kill -9 "${p}" 2>/dev/null ) & local w=$!
    wait "${p}"; local rc=$?
    kill "${w}" 2>/dev/null; wait "${w}" 2>/dev/null
    return "${rc}"
}

is_root() { [ "$(id -u)" = "0" ]; }

# ======================================================================== #
# Tier U -- unprivileged, always runs
# ======================================================================== #
echo
echo "Tier U (unprivileged): client fail-closed + daemon startup guard"

# U1. The daemon must refuse to start when it is not root (fail-closed startup,
#     design 8.1.F). Only meaningful when WE are not root.
if is_root; then
    skp "'daemon refuses to start as non-root' -- running as root; covered implicitly by Tier R start"
else
    run_bounded 5 "${DAEMON}" --socket "${TMP}/nostart.sock" --presence "${TMP}/none" \
        --insecure-skip-peer-verify >"${TMP}/u1.log" 2>&1
    rc=$?
    if [ "${rc}" -ne 0 ] && [ "${rc}" -ne 137 ] && [ ! -S "${TMP}/nostart.sock" ]; then
        ok "daemon refuses to start as non-root (exit ${rc}, no socket created)"
    else
        bad "daemon should refuse non-root start (exit ${rc}, socket present=$([ -S "${TMP}/nostart.sock" ] && echo yes || echo no))"
    fi
fi

# U2. Client denies, bounded, when there is no server at all (socket missing).
run_bounded 5 env REPOSE_PERMIT_SOCK_PATH="${TMP}/absent.sock" "${PROBE}" >"${TMP}/u2.out" 2>&1
rc=$?
if [ "${rc}" -eq 137 ]; then
    bad "client hung when the socket was missing (had to be killed)"
elif grep -qx "DENY" "${TMP}/u2.out"; then
    ok "client denies (bounded) when the daemon socket is missing"
else
    bad "client should DENY when the socket is missing (got: $(cat "${TMP}/u2.out"))"
fi

# U3. Client accepts a well-formed ALLOW (magic/version/op ok, nonce echoed).
"${MOCKD}" "${TMP}/mock.sock" allow >/dev/null 2>&1 & MOCKD_PID=$!
for _ in $(seq 1 30); do [ -S "${TMP}/mock.sock" ] && break; sleep 0.1; done
run_bounded 5 env REPOSE_PERMIT_SOCK_PATH="${TMP}/mock.sock" "${PROBE}" >"${TMP}/u3.out" 2>&1
grep -qx "ALLOW" "${TMP}/u3.out" \
    && ok "client accepts a well-formed ALLOW with echoed nonce" \
    || bad "client should ALLOW on a valid reply (got: $(cat "${TMP}/u3.out"))"
kill "${MOCKD_PID}" 2>/dev/null; wait "${MOCKD_PID}" 2>/dev/null; MOCKD_PID=""

# U4. Client rejects an ALLOW whose nonce does not match the request it sent.
"${MOCKD}" "${TMP}/mock.sock" wrongnonce >/dev/null 2>&1 & MOCKD_PID=$!
for _ in $(seq 1 30); do [ -S "${TMP}/mock.sock" ] && break; sleep 0.1; done
run_bounded 5 env REPOSE_PERMIT_SOCK_PATH="${TMP}/mock.sock" "${PROBE}" >"${TMP}/u4.out" 2>&1
grep -qx "DENY" "${TMP}/u4.out" \
    && ok "client denies an ALLOW with a mismatched nonce" \
    || bad "client should DENY on wrong nonce (got: $(cat "${TMP}/u4.out"))"
kill "${MOCKD_PID}" 2>/dev/null; wait "${MOCKD_PID}" 2>/dev/null; MOCKD_PID=""

# U5. Client rejects a garbage reply.
"${MOCKD}" "${TMP}/mock.sock" garbage >/dev/null 2>&1 & MOCKD_PID=$!
for _ in $(seq 1 30); do [ -S "${TMP}/mock.sock" ] && break; sleep 0.1; done
run_bounded 5 env REPOSE_PERMIT_SOCK_PATH="${TMP}/mock.sock" "${PROBE}" >"${TMP}/u5.out" 2>&1
grep -qx "DENY" "${TMP}/u5.out" \
    && ok "client denies a garbage reply" \
    || bad "client should DENY on garbage (got: $(cat "${TMP}/u5.out"))"
kill "${MOCKD_PID}" 2>/dev/null; wait "${MOCKD_PID}" 2>/dev/null; MOCKD_PID=""

# U6. Client denies, bounded, against a server that accepts but never replies.
"${MOCKD}" "${TMP}/mock.sock" blackhole >/dev/null 2>&1 & MOCKD_PID=$!
for _ in $(seq 1 30); do [ -S "${TMP}/mock.sock" ] && break; sleep 0.1; done
run_bounded 6 env REPOSE_PERMIT_SOCK_PATH="${TMP}/mock.sock" "${PROBE}" >"${TMP}/u6.out" 2>&1
rc=$?
if [ "${rc}" -eq 137 ]; then
    bad "client hung against a black-hole server (had to be killed)"
elif grep -qx "DENY" "${TMP}/u6.out"; then
    ok "client denies (bounded) against a black-hole server that never answers"
else
    bad "client should DENY against a black hole (got: $(cat "${TMP}/u6.out"))"
fi
kill "${MOCKD_PID}" 2>/dev/null; wait "${MOCKD_PID}" 2>/dev/null; MOCKD_PID=""

# ======================================================================== #
# Tier R -- daemon serve path; needs root (see header). Skipped otherwise.
# ======================================================================== #
echo
if is_root; then
    echo "Tier R (root): daemon presence / consume / malformed / peer-verify"
else
    echo "Tier R (unprivileged via REPOSE_PERMIT_ALLOW_NONROOT test seam):"
    echo "         daemon presence / consume / malformed / peer-verify"
fi

# The daemon serve path runs unprivileged through its OFF-by-default test seam;
# as root the seam is not needed. SEAM_ENV is an env-assignment token that is
# empty for root and "REPOSE_PERMIT_ALLOW_NONROOT=1" otherwise. Launching the
# daemon through `env ${SEAM_ENV}` works in both cases (env with no assignment
# just runs the command). The seam relaxes ONLY the euid==0 startup guard and
# the presence root-owner check -- nothing else in the consume path.
SEAM_ENV=""
if ! is_root; then
    SEAM_ENV="REPOSE_PERMIT_ALLOW_NONROOT=1"
fi

# Start the daemon on a scratch socket + scratch presence file. $1 socket, $2
# presence path, $3 extra flags. Returns 0 once the socket is listening.
DAEMON_LOG=""
start_daemon() {
    local s="$1" p="$2" extra="$3"
    # The R-cases reuse one socket path. Remove any lingering socket from the
    # previous case FIRST, so the readiness loop below can only observe THIS
    # daemon's socket -- which appears after it has seeded its single-consume
    # watermark. Otherwise we could return on a stale socket, touch the presence
    # file, and only then have the new daemon start and seed a watermark that is
    # already newer than the touch (making a fresh presence look already-spent).
    rm -f "${s}" 2>/dev/null
    DAEMON_LOG="${TMP}/daemon.$$.$RANDOM.log"
    # shellcheck disable=SC2086
    env ${SEAM_ENV} "${DAEMON}" --socket "${s}" --presence "${p}" --freshness 15 ${extra} >"${DAEMON_LOG}" 2>&1 &
    local i
    for i in $(seq 1 40); do [ -S "${s}" ] && return 0; sleep 0.1; done
    return 1
}
stop_daemon() {
    local s="$1"
    pkill -TERM -f "repose-permitd --socket ${s}" 2>/dev/null
    sleep 0.2
}
# Run the probe against a socket and echo ALLOW/DENY.
probe_verdict() {
    local s="$1"
    run_bounded 5 env REPOSE_PERMIT_SOCK_PATH="${s}" "${PROBE}" 2>/dev/null
}

    S="${TMP}/ipc/permit.sock"     # daemon mkdir's the parent 0750 in scratch mode
    P="${TMP}/presence"

    # R1/R2/R3: consume-once and re-authorise on a newer touch.
    if start_daemon "${S}" "${P}" "--insecure-skip-peer-verify"; then
        : > "${P}"                                  # fresh, root-owned (we are root)
        v1="$(probe_verdict "${S}")"
        v2="$(probe_verdict "${S}")"                # no re-touch between the two
        sleep 1; : > "${P}"                          # newer generation (mtime advances)
        v3="$(probe_verdict "${S}")"
        [ "${v1}" = "ALLOW" ] && ok "present presence -> ALLOW (consumed)" \
            || bad "present presence should ALLOW (got ${v1})"
        [ "${v2}" = "DENY" ]  && ok "replay of the same touch -> DENY (single-consume, gap #3)" \
            || bad "replay should DENY -- gap #3 not closed (got ${v2})"
        [ "${v3}" = "ALLOW" ] && ok "a newer touch -> ALLOW again" \
            || bad "a re-touch should re-authorise (got ${v3})"
        stop_daemon "${S}"
    else
        bad "daemon did not come up on scratch socket ${S}"; [ -n "${DAEMON_LOG}" ] && sed 's/^/      /' "${DAEMON_LOG}"
    fi

    # R4: absent presence -> DENY.
    if start_daemon "${S}" "${P}" "--insecure-skip-peer-verify"; then
        rm -f "${P}"
        v="$(probe_verdict "${S}")"
        [ "${v}" = "DENY" ] && ok "absent presence -> DENY" || bad "absent should DENY (got ${v})"
        stop_daemon "${S}"
    else
        bad "daemon did not come up (absent case)"
    fi

    # R5: stale presence (mtime far in the past) -> DENY.
    if start_daemon "${S}" "${P}" "--insecure-skip-peer-verify"; then
        : > "${P}"; touch -t 202001010101 "${P}"     # ~5+ years old, well past freshness
        v="$(probe_verdict "${S}")"
        [ "${v}" = "DENY" ] && ok "stale presence -> DENY" || bad "stale should DENY (got ${v})"
        stop_daemon "${S}"
    else
        bad "daemon did not come up (stale case)"
    fi

    # R6: fresh but NOT root-owned -> DENY (the anyone-could-have-written guard).
    # This one is inherently root-only: the test seam DELIBERATELY relaxes the
    # presence root-owner check (that is how the rest of the matrix runs
    # unprivileged), so under the seam the check is not in force and there is
    # nothing to assert; and an unprivileged runner cannot chown a file off
    # itself to set the case up. Exercised only when we are actually root.
    if ! is_root; then
        skp "non-root-owned presence -> DENY -- root-only (the owner check is relaxed under REPOSE_PERMIT_ALLOW_NONROOT; run as: sudo ${BASH_SOURCE[0]})"
    elif start_daemon "${S}" "${P}" "--insecure-skip-peer-verify"; then
        : > "${P}"
        # Hand ownership to a non-root uid. Prefer 'nobody'; fall back to uid 1.
        chown nobody "${P}" 2>/dev/null || chown 1 "${P}" 2>/dev/null
        owner="$(stat -f '%u' "${P}" 2>/dev/null)"
        if [ "${owner}" = "0" ]; then
            skp "non-root-owned presence -> DENY (could not chown the scratch file off root)"
        else
            v="$(probe_verdict "${S}")"
            [ "${v}" = "DENY" ] && ok "non-root-owned presence -> DENY" \
                || bad "a non-root-owned permit must DENY (got ${v})"
        fi
        stop_daemon "${S}"
    else
        bad "daemon did not come up (ownership case)"
    fi

    # R7: malformed requests are rejected AND do not consume the presence. After
    #     a fresh touch, feed a truncated frame and a bad-magic frame (both must
    #     DENY); a well-formed probe on the SAME, un-re-touched presence must
    #     still ALLOW -- proving neither malformed request spent the generation.
    if start_daemon "${S}" "${P}" "--insecure-skip-peer-verify"; then
        : > "${P}"
        short="$("${FRAMESEND}" "${S}" "${SHORT_FRAME_HEX}" 1 2>/dev/null)"
        badm="$("${FRAMESEND}" "${S}" "${BADMAGIC_FRAME_HEX}" 1 2>/dev/null)"
        after="$(probe_verdict "${S}")"
        [ "${short}" != "ALLOW" ] && ok "short request -> DENY" || bad "short frame must not ALLOW (got ${short})"
        [ "${badm}"  != "ALLOW" ] && ok "bad-magic request -> DENY" || bad "bad-magic frame must not ALLOW (got ${badm})"
        [ "${after}" = "ALLOW" ]  && ok "malformed requests did NOT consume (fresh probe still ALLOWs)" \
            || bad "a malformed request wrongly consumed the presence (follow-up got ${after})"
        stop_daemon "${S}"
    else
        bad "daemon did not come up (malformed case)"
    fi

    # R8: a client that writes a partial frame and never half-closes must be cut
    #     loose by the daemon's per-connection timeout, not wedge it. framesend
    #     returns once the daemon closes; it must finish well within the bound.
    if start_daemon "${S}" "${P}" "--insecure-skip-peer-verify"; then
        : > "${P}"
        run_bounded 5 "${FRAMESEND}" "${S}" "${SHORT_FRAME_HEX}" 0 >"${TMP}/r8.out" 2>&1
        rc=$?
        if [ "${rc}" -eq 137 ]; then
            bad "daemon wedged on a never-closed request (framesend had to be killed)"
        elif [ "$(cat "${TMP}/r8.out")" != "ALLOW" ]; then
            ok "request that never half-closes -> daemon times out (bounded), DENY"
        else
            bad "a never-completed request must not ALLOW (got $(cat "${TMP}/r8.out"))"
        fi
        stop_daemon "${S}"
    else
        bad "daemon did not come up (timeout case)"
    fi

    # R9: peer verification ON. The probe is a permitted uid when we run as root
    #     (uid 0 is on the allowlist) but it is NOT com.apple.authorizationhost,
    #     so SecCodeCheckValidity must reject it -> DENY. This is the local half
    #     of peer verification; the ACCEPT half (real signed hosts) is VM-only.
    if start_daemon "${S}" "${P}" ""; then           # no --insecure-skip-peer-verify
        : > "${P}"
        v="$(probe_verdict "${S}")"
        [ "${v}" = "DENY" ] && ok "peer verification rejects an allowed-uid non-Apple caller" \
            || bad "peer verification should reject a non-mechanism caller (got ${v})"
        stop_daemon "${S}"
    else
        skp "peer-verify reject case -- daemon could not start with verification ON (see log)"
        [ -n "${DAEMON_LOG}" ] && sed 's/^/      /' "${DAEMON_LOG}"
    fi

# The uid->requirement SELECTION function (design 8.1.E) is internal to
# repose_peer_verify.c and not separately exported, so it cannot be unit-tested
# in isolation from here; R9 exercises the reject decision end to end instead.
skp "uid->designated-requirement selector unit test -- not exported by repose_peer_verify.c (R9 covers the reject path e2e; accept path is VM-only, design 8.2)"

echo
echo "${pass} passed, ${fail} failed, ${skip} skipped"
if ! is_root; then
    cat <<'NOTE'

NOTE ON TIER R WITHOUT ROOT:
  The daemon serve path above (present->ALLOW->consume, replay->DENY,
  stale->DENY, malformed->DENY, bounded timeout, peer-verify reject) ran
  unprivileged through the daemon's OFF-by-default test seam
  REPOSE_PERMIT_ALLOW_NONROOT=1, which relaxes ONLY the euid==0 startup guard and
  the presence root-owner check (repose-permitd.c). The frame validation,
  freshness/skew, single-consume watermark and nonce echo were all exercised
  unchanged. Two things still require real root and are skipped here:
    - the non-root-owned-presence DENY (R6): that check is exactly what the seam
      relaxes, so it can only be asserted with the seam OFF, i.e. as root; and
    - the SecurityAgent/authorizationhost ACCEPT half of peer verification, which
      is VM-only (design 8.2). R9 covers the local REJECT half here.
  To also cover R6, run as root on a disposable Mac:
      sudo native/macos/minimal-auth-plugin/tests/permit_daemon_test.sh
NOTE
fi
[ "${fail}" = 0 ]
