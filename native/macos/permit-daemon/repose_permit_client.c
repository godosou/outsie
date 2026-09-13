/*
 * repose_permit_client.c -- see repose_permit_client.h.
 *
 * One request per connection. The client:
 *   1. connect()s to the socket (non-blocking, bounded),
 *   2. writes one 40-byte request frame carrying a fresh random nonce,
 *   3. shutdown(SHUT_WR) to signal "that's the whole request",
 *   4. reads exactly one 40-byte verdict frame,
 *   5. accepts ALLOW iff magic/version/op are right, the nonce is echoed back
 *      verbatim, and the verdict byte is ALLOW.
 *
 * Everything is on a single monotonic deadline; a short read, a wrong nonce, a
 * refused connection, or a daemon that never answers all end in DENY. A small
 * retry loop re-asks every REPOSE_PERMIT_POLL_MS until the deadline, which
 * preserves the "wait a beat for the phone" feel of the old poll loop -- and is
 * safe against the single-consume rule because only an ALLOW consumes; denied
 * probes spend nothing.
 */
#include "repose_permit_client.h"
#include "repose_permit_wire.h"

#include <errno.h>
#include <fcntl.h>
#include <poll.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#include <sys/socket.h>
#include <sys/un.h>

#define TIMEOUT_MS_DEFAULT 1200
#define POLL_MS_DEFAULT 200
#define ATTEMPT_MS_CAP 800 /* a single round trip should never need more */

static long now_ms(void)
{
    struct timespec t;
    clock_gettime(CLOCK_MONOTONIC, &t);
    return (long)t.tv_sec * 1000L + t.tv_nsec / 1000000L;
}

static long env_ms(const char *name, long fallback)
{
    const char *s = getenv(name);
    if (s == NULL || s[0] == '\0') {
        return fallback;
    }
    char *end = NULL;
    long v = strtol(s, &end, 10);
    if (end == s || *end != '\0' || v < 0) {
        return fallback;
    }
    return v;
}

/* Wait until fd is ready for `events`, or the deadline passes. Returns 1 ready,
 * 0 timeout, -1 error. */
static int wait_ready(int fd, short events, long deadline)
{
    for (;;) {
        long remaining = deadline - now_ms();
        if (remaining <= 0) {
            return 0;
        }
        struct pollfd pfd;
        pfd.fd = fd;
        pfd.events = events;
        pfd.revents = 0;
        int r = poll(&pfd, 1, (int)remaining);
        if (r < 0) {
            if (errno == EINTR) {
                continue;
            }
            return -1;
        }
        if (r == 0) {
            return 0;
        }
        if (pfd.revents & (POLLERR | POLLNVAL)) {
            return -1;
        }
        return 1;
    }
}

static int connect_deadline(int fd, const struct sockaddr_un *addr, long deadline)
{
    int rc = connect(fd, (const struct sockaddr *)addr, (socklen_t)sizeof(*addr));
    if (rc == 0) {
        return 0;
    }
    if (errno != EINPROGRESS) {
        return -1;
    }
    if (wait_ready(fd, POLLOUT, deadline) != 1) {
        return -1;
    }
    int err = 0;
    socklen_t len = (socklen_t)sizeof(err);
    if (getsockopt(fd, SOL_SOCKET, SO_ERROR, &err, &len) != 0 || err != 0) {
        return -1;
    }
    return 0;
}

static int write_all_deadline(int fd, const uint8_t *buf, size_t n, long deadline)
{
    size_t sent = 0;
    while (sent < n) {
        if (wait_ready(fd, POLLOUT, deadline) != 1) {
            return -1;
        }
        ssize_t w = write(fd, buf + sent, n - sent);
        if (w < 0) {
            if (errno == EINTR || errno == EAGAIN) {
                continue;
            }
            return -1;
        }
        sent += (size_t)w;
    }
    return 0;
}

static int read_all_deadline(int fd, uint8_t *buf, size_t n, long deadline)
{
    size_t got = 0;
    while (got < n) {
        if (wait_ready(fd, POLLIN, deadline) != 1) {
            return -1;
        }
        ssize_t r = read(fd, buf + got, n - got);
        if (r == 0) {
            return -1; /* EOF before a full frame (e.g. peer-unverified close) */
        }
        if (r < 0) {
            if (errno == EINTR || errno == EAGAIN) {
                continue;
            }
            return -1;
        }
        got += (size_t)r;
    }
    return 0;
}

/* One full round trip. Returns 1 allow, 0 deny/error. */
static int one_attempt(const char *sock_path, long deadline)
{
    struct sockaddr_un addr;
    if (strlen(sock_path) >= sizeof(addr.sun_path)) {
        return 0;
    }

    int fd = socket(AF_UNIX, SOCK_STREAM, 0);
    if (fd < 0) {
        return 0;
    }
    int on = 1;
    setsockopt(fd, SOL_SOCKET, SO_NOSIGPIPE, &on, sizeof(on));
    int flags = fcntl(fd, F_GETFL, 0);
    if (flags < 0 || fcntl(fd, F_SETFL, flags | O_NONBLOCK) < 0) {
        close(fd);
        return 0;
    }

    memset(&addr, 0, sizeof(addr));
    addr.sun_family = AF_UNIX;
    strncpy(addr.sun_path, sock_path, sizeof(addr.sun_path) - 1);

    int verdict = 0;

    if (connect_deadline(fd, &addr, deadline) == 0) {
        uint8_t nonce[REPOSE_PERMIT_NONCE_LEN];
        arc4random_buf(nonce, sizeof(nonce));

        uint8_t req[REPOSE_PERMIT_FRAME_LEN];
        memset(req, 0, sizeof(req));
        req[REPOSE_PERMIT_OFF_MAGIC + 0] = REPOSE_PERMIT_MAGIC0;
        req[REPOSE_PERMIT_OFF_MAGIC + 1] = REPOSE_PERMIT_MAGIC1;
        req[REPOSE_PERMIT_OFF_MAGIC + 2] = REPOSE_PERMIT_MAGIC2;
        req[REPOSE_PERMIT_OFF_MAGIC + 3] = REPOSE_PERMIT_MAGIC3;
        req[REPOSE_PERMIT_OFF_VERSION] = REPOSE_PERMIT_VERSION;
        req[REPOSE_PERMIT_OFF_OP] = REPOSE_PERMIT_OP_REQUEST;
        memcpy(req + REPOSE_PERMIT_OFF_NONCE, nonce, sizeof(nonce));

        if (write_all_deadline(fd, req, sizeof(req), deadline) == 0) {
            /* Tell the daemon the request is complete; it reads to this point
             * before answering. Ignore shutdown errors -- the read below is the
             * real arbiter. */
            shutdown(fd, SHUT_WR);

            uint8_t resp[REPOSE_PERMIT_FRAME_LEN];
            if (read_all_deadline(fd, resp, sizeof(resp), deadline) == 0 &&
                resp[REPOSE_PERMIT_OFF_MAGIC + 0] == REPOSE_PERMIT_MAGIC0 &&
                resp[REPOSE_PERMIT_OFF_MAGIC + 1] == REPOSE_PERMIT_MAGIC1 &&
                resp[REPOSE_PERMIT_OFF_MAGIC + 2] == REPOSE_PERMIT_MAGIC2 &&
                resp[REPOSE_PERMIT_OFF_MAGIC + 3] == REPOSE_PERMIT_MAGIC3 &&
                resp[REPOSE_PERMIT_OFF_VERSION] == REPOSE_PERMIT_VERSION &&
                resp[REPOSE_PERMIT_OFF_OP] == REPOSE_PERMIT_OP_VERDICT &&
                memcmp(resp + REPOSE_PERMIT_OFF_NONCE, nonce, sizeof(nonce)) == 0 &&
                resp[REPOSE_PERMIT_OFF_VERDICT] == REPOSE_PERMIT_VERDICT_ALLOW) {
                verdict = 1;
            }
        }
    }

    close(fd);
    return verdict;
}

int repose_request_permit(void)
{
    const char *sock_path = getenv("REPOSE_PERMIT_SOCK_PATH");
    if (sock_path == NULL || sock_path[0] == '\0') {
        sock_path = REPOSE_PERMIT_SOCK_PATH;
    }
    long total = env_ms("REPOSE_PERMIT_TIMEOUT_MS", TIMEOUT_MS_DEFAULT);
    long interval = env_ms("REPOSE_PERMIT_POLL_MS", POLL_MS_DEFAULT);
    long overall_deadline = now_ms() + total;

    for (;;) {
        long attempt_deadline = now_ms() + ATTEMPT_MS_CAP;
        if (attempt_deadline > overall_deadline) {
            attempt_deadline = overall_deadline;
        }
        if (one_attempt(sock_path, attempt_deadline) == 1) {
            return 1;
        }
        long remaining = overall_deadline - now_ms();
        if (remaining <= 0) {
            return 0;
        }
        long nap = (remaining < interval) ? remaining : interval;
        if (nap > 0) {
            usleep((useconds_t)nap * 1000);
        }
    }
}
