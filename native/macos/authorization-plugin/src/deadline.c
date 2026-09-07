#include "deadline.h"

#include <limits.h>
#include <stddef.h>

static bool monotonic_now(struct timespec *out_now) {
    return out_now != NULL && clock_gettime(CLOCK_MONOTONIC, out_now) == 0;
}

bool repose_deadline_start(repose_deadline_t *out_deadline, uint32_t milliseconds) {
    if (out_deadline == NULL || !monotonic_now(&out_deadline->absolute)) {
        return false;
    }
    out_deadline->absolute.tv_sec += (time_t)(milliseconds / 1000);
    out_deadline->absolute.tv_nsec += (long)(milliseconds % 1000) * 1000000L;
    if (out_deadline->absolute.tv_nsec >= 1000000000L) {
        out_deadline->absolute.tv_sec += 1;
        out_deadline->absolute.tv_nsec -= 1000000000L;
    }
    return true;
}

bool repose_deadline_expired(const repose_deadline_t *deadline) {
    if (deadline == NULL) {
        return true;
    }
    struct timespec now;
    if (!monotonic_now(&now)) {
        return true;
    }
    return now.tv_sec > deadline->absolute.tv_sec ||
           (now.tv_sec == deadline->absolute.tv_sec &&
            now.tv_nsec >= deadline->absolute.tv_nsec);
}

int repose_deadline_poll_timeout_ms(const repose_deadline_t *deadline) {
    if (deadline == NULL) {
        return 0;
    }
    struct timespec now;
    if (!monotonic_now(&now)) {
        return 0;
    }
    int64_t seconds = (int64_t)deadline->absolute.tv_sec - (int64_t)now.tv_sec;
    int64_t nanoseconds = (int64_t)deadline->absolute.tv_nsec - (int64_t)now.tv_nsec;
    int64_t remaining = seconds * 1000000000LL + nanoseconds;
    if (remaining <= 0) {
        return 0;
    }
    int64_t milliseconds = (remaining + 999999LL) / 1000000LL;
    return milliseconds > INT_MAX ? INT_MAX : (int)milliseconds;
}
