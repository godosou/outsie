/*
 * repose-permitd -- the root daemon that replaces the plugin's direct file read
 * with a request/response over a unix-domain socket. It closes permit-design
 * gap #3 (a fresh permit could be replayed for any number of unlock attempts
 * within its freshness window) by consuming each distinct presence assertion
 * exactly once, and it verifies that the process asking is really the macOS
 * SecurityAgent / authorizationhost mechanism, not just any local process.
 *
 * What it is NOT: it does not talk BLE and does not decide, on its own, whether
 * the phone is near. That signal still comes from the unmodified
 * permit-bridge.sh, which re-touches /var/run/repose-spike/permit while the
 * phone is in range. The daemon reads that file's freshness as "phone here
 * now", so the bridge needs no change and the crash => stale => deny property
 * (gap #4) is preserved end to end.
 *
 * Self-contained C: clang + CoreFoundation + Security + libbsm. No Rust, no
 * python. See build.sh.
 *
 * Fail closed everywhere. Anything unexpected -- cannot become root, cannot
 * own the socket directory, peer unverified, request malformed, presence stale
 * or already spent -- ends in a DENY (or the daemon refusing to start), never
 * an accidental allow.
 *
 * Throwaway spike. Run only on a disposable, rollback-capable test machine.
 */
#include "repose_permit_wire.h"
#include "repose_peer_verify.h"

#include <errno.h>
#include <fcntl.h>
#include <signal.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <sys/un.h>

/* A presence assertion counts only if it is at most this old. Must match the
 * writer's re-touch cadence with margin, exactly as PERMIT_FRESHNESS_S does in
 * plugin.c today. */
#define PRESENCE_FRESHNESS_S_DEFAULT 15
/* Tolerance for a permit stamped a moment ago against a slightly-behind clock. */
#define PRESENCE_SKEW_S_DEFAULT 5
/* Per-connection I/O budget. A client that connects but will not send its 40
 * bytes, or will not read its reply, is cut loose after this. The daemon is
 * single-threaded, so an unbounded client would wedge the lock screen. */
#define CONN_IO_TIMEOUT_MS 800
#define LISTEN_BACKLOG 16

/* -------- configuration (argv / env), resolved once in main -------- */
static const char *g_sock_path;
static const char *g_presence_path;
static long g_freshness_s;
static long g_skew_s;
static int g_skip_verify; /* debug only; loudly refused unless flagged */

/* -------- single-consume state (single-threaded => naturally atomic) -------- */
/* The mtime of the most recent presence assertion we have already spent on an
 * allow. A presence file only re-authorises once its mtime advances past this,
 * which happens each time the bridge re-touches it. So one touch => at most one
 * unlock; a replay against the same touch is denied. */
static struct timespec g_last_consumed = { 0, 0 };

static volatile sig_atomic_t g_stop = 0;

static void on_signal(int sig)
{
    (void)sig;
    g_stop = 1;
}

static void logf(const char *fmt, ...)
{
    struct timespec ts;
    clock_gettime(CLOCK_REALTIME, &ts);
    struct tm parts;
    localtime_r(&ts.tv_sec, &parts);
    char stamp[32];
    strftime(stamp, sizeof stamp, "%Y-%m-%dT%H:%M:%S", &parts);
    fprintf(stderr, "%s.%03ld repose-permitd[%d]: ", stamp,
            ts.tv_nsec / 1000000, (int)getpid());
    va_list args;
    va_start(args, fmt);
    vfprintf(stderr, fmt, args);
    va_end(args);
    fputc('\n', stderr);
    fflush(stderr);
}

static int timespec_gt(struct timespec a, struct timespec b)
{
    if (a.tv_sec != b.tv_sec) {
        return a.tv_sec > b.tv_sec;
    }
    return a.tv_nsec > b.tv_nsec;
}

/*
 * Prepare the socket's directory (/var/run/repose-permitd in production) as a
 * root-owned, _securityagent-traversable directory: 0750 root:_securityagent.
 * It is deliberately separate from the bridge's /var/run/repose-spike so the
 * bridge's periodic `chmod 755` cannot loosen it. This directory is the primary
 * filesystem gate. To connect() to the socket inside it a client must first
 * resolve the path, which requires search (+x) on this directory. An ordinary
 * uid is not in gid 92, gets no bits here, and is refused with EACCES at path
 * resolution -- before connect() ever reaches the socket. Only root (owner) and
 * uid 92 (group) can traverse. Returns 0 on success.
 */
static int prepare_dir(void)
{
    const int is_production = (strcmp(g_sock_path, REPOSE_PERMIT_SOCK_PATH) == 0);

    /* Derive the socket's parent directory from g_sock_path so a debug/scratch
     * --socket lands its directory too. */
    char dir[1024];
    snprintf(dir, sizeof(dir), "%s", g_sock_path);
    char *slash = strrchr(dir, '/');
    if (slash == NULL) {
        logf("socket path has no directory component: %s", g_sock_path);
        return -1;
    }
    if (slash == dir) {
        dir[1] = '\0'; /* socket directly under "/" */
    } else {
        *slash = '\0';
    }

    if (mkdir(dir, 0750) != 0 && errno != EEXIST) {
        logf("mkdir %s failed: %s", dir, strerror(errno));
        return -1;
    }

    if (!is_production) {
        /* Debug/scratch socket (e.g. under /tmp): do not try to take ownership
         * of a shared system directory. Peer verification is the security
         * boundary and scratch runs disable it deliberately. */
        logf("scratch socket dir %s (not hardened; production path is %s)",
             dir, REPOSE_PERMIT_SOCK_PATH);
        return 0;
    }

    /* Production: the directory is the primary filesystem gate.
     * 0750 root:_securityagent -- only root and uid 92 may traverse to the
     * socket; an ordinary uid is refused at path resolution, before connect(). */
    if (chown(dir, REPOSE_PERMIT_ROOT_UID, REPOSE_PERMIT_SECURITYAGENT_GID) != 0) {
        logf("chown %s to 0:%d failed: %s", dir,
             REPOSE_PERMIT_SECURITYAGENT_GID, strerror(errno));
        return -1;
    }
    if (chmod(dir, 0750) != 0) {
        logf("chmod 0750 %s failed: %s", dir, strerror(errno));
        return -1;
    }
    return 0;
}

/*
 * Bind the listening socket and set it to 0660 root:_securityagent. The socket
 * mode is belt-and-suspenders behind the directory gate above: on macOS the
 * kernel does enforce write permission on the socket node for connect(), but
 * that enforcement has been inconsistent across releases, so the daemon does
 * not rely on it alone -- the directory traversal gate (always enforced by the
 * VFS) plus in-process peer verification are the real boundaries. Returns the
 * listening fd, or -1.
 */
static int bind_listen(void)
{
    struct sockaddr_un addr;
    if (strlen(g_sock_path) >= sizeof(addr.sun_path)) {
        logf("socket path too long: %s", g_sock_path);
        return -1;
    }

    int fd = socket(AF_UNIX, SOCK_STREAM, 0);
    if (fd < 0) {
        logf("socket() failed: %s", strerror(errno));
        return -1;
    }

    /* A stale socket file from a previous run would make bind() fail with
     * EADDRINUSE even though nobody is listening. Remove it first. It lives in
     * a root-only-writable directory, so this unlink cannot be redirected. */
    if (unlink(g_sock_path) != 0 && errno != ENOENT) {
        logf("unlink stale %s failed: %s", g_sock_path, strerror(errno));
        close(fd);
        return -1;
    }

    memset(&addr, 0, sizeof(addr));
    addr.sun_family = AF_UNIX;
    strncpy(addr.sun_path, g_sock_path, sizeof(addr.sun_path) - 1);

    /* umask(0077) already narrowed the create mode; bind makes the node as
     * 0700 root, a strictly-safe starting point that we widen deliberately. */
    if (bind(fd, (struct sockaddr *)&addr, (socklen_t)sizeof(addr)) != 0) {
        logf("bind %s failed: %s", g_sock_path, strerror(errno));
        close(fd);
        return -1;
    }
    if (chown(g_sock_path, REPOSE_PERMIT_ROOT_UID,
              REPOSE_PERMIT_SECURITYAGENT_GID) != 0) {
        logf("chown socket to 0:%d failed: %s",
             REPOSE_PERMIT_SECURITYAGENT_GID, strerror(errno));
        close(fd);
        return -1;
    }
    if (chmod(g_sock_path, 0660) != 0) {
        logf("chmod 0660 socket failed: %s", strerror(errno));
        close(fd);
        return -1;
    }
    if (listen(fd, LISTEN_BACKLOG) != 0) {
        logf("listen failed: %s", strerror(errno));
        close(fd);
        return -1;
    }
    return fd;
}

/*
 * Is the phone here right now, and is THIS presence assertion one we have not
 * already spent? Reads the bridge's presence file with the same anti-symlink /
 * anti-FIFO hardening plugin.c uses (O_NOFOLLOW | O_NONBLOCK, fstat, regular
 * file, root-owned, fresh). On the winning path it advances g_last_consumed so
 * the same touch cannot authorise a second unlock. Returns 1 to allow, 0 deny.
 */
static int consume_presence(void)
{
    int fd = open(g_presence_path, O_RDONLY | O_NOFOLLOW | O_NONBLOCK);
    if (fd < 0) {
        logf("presence: %s absent (%s); deny", g_presence_path, strerror(errno));
        return 0;
    }
    struct stat st;
    int rc = fstat(fd, &st);
    close(fd);
    if (rc != 0 || !S_ISREG(st.st_mode)) {
        logf("presence: %s is not a regular file; deny", g_presence_path);
        return 0;
    }
    if (st.st_uid != 0) {
        logf("presence: %s not root-owned (uid=%d); deny", g_presence_path,
             (int)st.st_uid);
        return 0;
    }
    double age = difftime(time(NULL), st.st_mtime);
    if (age > (double)g_freshness_s) {
        logf("presence: %s stale (%.0fs > %lds); deny", g_presence_path, age,
             g_freshness_s);
        return 0;
    }
    if (age < -(double)g_skew_s) {
        logf("presence: %s mtime %.0fs in the future; deny", g_presence_path,
             -age);
        return 0;
    }

    struct timespec mtime = st.st_mtimespec;
    if (!timespec_gt(mtime, g_last_consumed)) {
        /* Fresh, but this exact assertion (or an older one) was already spent.
         * This is the gap-#3 fix: a replay within the freshness window is denied
         * until the bridge re-touches and the mtime advances. */
        logf("presence: assertion mtime=%ld.%09ld already consumed; deny",
             (long)mtime.tv_sec, (long)mtime.tv_nsec);
        return 0;
    }
    g_last_consumed = mtime;
    logf("presence: consumed assertion mtime=%ld.%09ld; allow",
         (long)mtime.tv_sec, (long)mtime.tv_nsec);
    return 1;
}

/* Read exactly n bytes or fail. SO_RCVTIMEO on the socket bounds each read, so
 * a peer that stalls returns EAGAIN and we deny. */
static int read_exact(int fd, uint8_t *buf, size_t n)
{
    size_t got = 0;
    while (got < n) {
        ssize_t r = read(fd, buf + got, n - got);
        if (r == 0) {
            return -1; /* EOF before a full frame => truncated => deny */
        }
        if (r < 0) {
            if (errno == EINTR) {
                continue;
            }
            return -1; /* EAGAIN (timeout) or hard error => deny */
        }
        got += (size_t)r;
    }
    return 0;
}

static int write_exact(int fd, const uint8_t *buf, size_t n)
{
    size_t sent = 0;
    while (sent < n) {
        ssize_t w = write(fd, buf + sent, n - sent);
        if (w < 0) {
            if (errno == EINTR) {
                continue;
            }
            return -1;
        }
        sent += (size_t)w;
    }
    return 0;
}

static void set_conn_timeouts(int fd)
{
    struct timeval tv;
    tv.tv_sec = CONN_IO_TIMEOUT_MS / 1000;
    tv.tv_usec = (CONN_IO_TIMEOUT_MS % 1000) * 1000;
    setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof(tv));
    setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &tv, sizeof(tv));
    int on = 1;
    setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &on, sizeof(on));
}

/*
 * One connection, one request, one verdict. Order matters: verify the peer
 * BEFORE reading anything it sends, so an unverified caller never influences
 * the daemon and simply gets its connection closed (which the client reads as a
 * deny). Only a verified host's request is parsed and answered.
 */
static void handle_conn(int fd, const repose_peer_verifier_t *verifier)
{
    set_conn_timeouts(fd);

    uid_t euid = (uid_t)-1;
    pid_t pid = -1;
    if (!g_skip_verify) {
        int vr = repose_peer_verifier_check(verifier, fd, &euid, &pid);
        if (vr != 0) {
            logf("peer verification failed (rc=%d); closing (deny)", vr);
            return; /* client sees EOF => deny */
        }
        logf("peer verified euid=%d pid=%d", (int)euid, (int)pid);
    } else {
        logf("WARNING: peer verification SKIPPED (--insecure-skip-peer-verify)");
    }

    uint8_t req[REPOSE_PERMIT_FRAME_LEN];
    if (read_exact(fd, req, sizeof(req)) != 0) {
        logf("request read incomplete; closing (deny)");
        return;
    }
    if (req[REPOSE_PERMIT_OFF_MAGIC + 0] != REPOSE_PERMIT_MAGIC0 ||
        req[REPOSE_PERMIT_OFF_MAGIC + 1] != REPOSE_PERMIT_MAGIC1 ||
        req[REPOSE_PERMIT_OFF_MAGIC + 2] != REPOSE_PERMIT_MAGIC2 ||
        req[REPOSE_PERMIT_OFF_MAGIC + 3] != REPOSE_PERMIT_MAGIC3 ||
        req[REPOSE_PERMIT_OFF_VERSION] != REPOSE_PERMIT_VERSION ||
        req[REPOSE_PERMIT_OFF_OP] != REPOSE_PERMIT_OP_REQUEST) {
        logf("request malformed (magic/version/op); closing (deny)");
        return;
    }

    int allow = consume_presence();

    uint8_t resp[REPOSE_PERMIT_FRAME_LEN];
    memset(resp, 0, sizeof(resp));
    resp[REPOSE_PERMIT_OFF_MAGIC + 0] = REPOSE_PERMIT_MAGIC0;
    resp[REPOSE_PERMIT_OFF_MAGIC + 1] = REPOSE_PERMIT_MAGIC1;
    resp[REPOSE_PERMIT_OFF_MAGIC + 2] = REPOSE_PERMIT_MAGIC2;
    resp[REPOSE_PERMIT_OFF_MAGIC + 3] = REPOSE_PERMIT_MAGIC3;
    resp[REPOSE_PERMIT_OFF_VERSION] = REPOSE_PERMIT_VERSION;
    resp[REPOSE_PERMIT_OFF_OP] = REPOSE_PERMIT_OP_VERDICT;
    resp[REPOSE_PERMIT_OFF_VERDICT] =
        allow ? REPOSE_PERMIT_VERDICT_ALLOW : REPOSE_PERMIT_VERDICT_DENY;
    /* Echo the client's nonce verbatim so it can prove this verdict answers the
     * request it just sent, not a stale or injected reply. */
    memcpy(resp + REPOSE_PERMIT_OFF_NONCE, req + REPOSE_PERMIT_OFF_NONCE,
           REPOSE_PERMIT_NONCE_LEN);

    if (write_exact(fd, resp, sizeof(resp)) != 0) {
        logf("verdict write failed: %s", strerror(errno));
    }
}

static long env_long(const char *name, long fallback)
{
    const char *s = getenv(name);
    if (s == NULL || s[0] == '\0') {
        return fallback;
    }
    char *end = NULL;
    long v = strtol(s, &end, 10);
    if (end == s || *end != '\0') {
        return fallback;
    }
    return v;
}

int main(int argc, char **argv)
{
    g_sock_path = REPOSE_PERMIT_SOCK_PATH;
    g_presence_path = REPOSE_PERMIT_PRESENCE_PATH;
    g_freshness_s = env_long("REPOSE_PERMIT_FRESHNESS_S", PRESENCE_FRESHNESS_S_DEFAULT);
    g_skew_s = env_long("REPOSE_PERMIT_SKEW_S", PRESENCE_SKEW_S_DEFAULT);
    g_skip_verify = 0;

    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--socket") == 0 && i + 1 < argc) {
            g_sock_path = argv[++i];
        } else if (strcmp(argv[i], "--presence") == 0 && i + 1 < argc) {
            g_presence_path = argv[++i];
        } else if (strcmp(argv[i], "--freshness") == 0 && i + 1 < argc) {
            g_freshness_s = strtol(argv[++i], NULL, 10);
        } else if (strcmp(argv[i], "--insecure-skip-peer-verify") == 0) {
            g_skip_verify = 1;
        } else {
            fprintf(stderr,
                    "usage: %s [--socket PATH] [--presence PATH] "
                    "[--freshness SECS] [--insecure-skip-peer-verify]\n",
                    argv[0]);
            return 2;
        }
    }

    /* Must be root: to own the socket root:_securityagent, to read the
     * root-owned presence file, and because SecCodeCheckValidity on another
     * process is a privileged operation. If we are not root, refuse to start
     * rather than come up in a half-secure state. */
    if (geteuid() != 0) {
        logf("must run as root (euid=%d); refusing to start", (int)geteuid());
        return 1;
    }

    /* SIGPIPE would kill us if a client vanished mid-write; SO_NOSIGPIPE covers
     * the socket but ignore it globally too. SIGTERM/SIGINT stop the loop so we
     * can unlink the socket on the way out. */
    signal(SIGPIPE, SIG_IGN);
    signal(SIGTERM, on_signal);
    signal(SIGINT, on_signal);

    /* Narrow the create mode so bind()/mkdir() never momentarily expose a
     * world-accessible node before the explicit chmod below. */
    umask(0077);

    /* Seed the single-consume watermark with the current wall clock. Any
     * presence assertion whose mtime predates daemon start is treated as
     * already spent, so a daemon restart (crash + KeepAlive) cannot let a
     * presence assertion that an earlier run already consumed be replayed --
     * only a fresh re-touch (mtime after startup) re-authorises. The bridge
     * re-touches every few seconds, so a legitimate unlock is delayed by at
     * most one re-touch interval after a restart. */
    clock_gettime(CLOCK_REALTIME, &g_last_consumed);

    if (prepare_dir() != 0) {
        return 1;
    }

    repose_peer_verifier_t *verifier = NULL;
    if (!g_skip_verify) {
        verifier = repose_peer_verifier_create();
        if (verifier == NULL) {
            logf("could not compile peer requirements; refusing to start");
            return 1;
        }
    }

    int listen_fd = bind_listen();
    if (listen_fd < 0) {
        repose_peer_verifier_destroy(verifier);
        return 1;
    }

    logf("listening on %s (presence=%s freshness=%lds verify=%s)",
         g_sock_path, g_presence_path, g_freshness_s,
         g_skip_verify ? "OFF-INSECURE" : "on");

    while (!g_stop) {
        int cfd = accept(listen_fd, NULL, NULL);
        if (cfd < 0) {
            if (errno == EINTR) {
                continue; /* signal: re-check g_stop */
            }
            logf("accept failed: %s", strerror(errno));
            continue;
        }
        handle_conn(cfd, verifier);
        close(cfd);
    }

    logf("stopping; removing socket");
    close(listen_fd);
    unlink(g_sock_path);
    repose_peer_verifier_destroy(verifier);
    return 0;
}
