#ifndef REPOSE_AUTHORIZATION_DEADLINE_H
#define REPOSE_AUTHORIZATION_DEADLINE_H

#include <stdbool.h>
#include <stdint.h>
#include <time.h>

typedef struct repose_deadline {
    struct timespec absolute;
} repose_deadline_t;

bool repose_deadline_start(repose_deadline_t *out_deadline, uint32_t milliseconds);
bool repose_deadline_expired(const repose_deadline_t *deadline);
int repose_deadline_poll_timeout_ms(const repose_deadline_t *deadline);

#endif
