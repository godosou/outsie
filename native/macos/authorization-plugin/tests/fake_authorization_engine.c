#include "fake_authorization_engine.h"

#include <assert.h>
#include <errno.h>
#include <string.h>
#include <time.h>

static repose_fake_authorization_engine_t *fake_engine(AuthorizationEngineRef reference) {
    assert(reference != NULL);
    return (repose_fake_authorization_engine_t *)reference;
}

static void append_event(repose_fake_authorization_engine_t *engine,
                         repose_fake_callback_event_t event) {
    assert(engine->event_count < REPOSE_FAKE_MAX_EVENTS);
    engine->events[engine->event_count] = event;
    engine->event_count += 1;
}

static OSStatus fake_set_result(AuthorizationEngineRef reference, AuthorizationResult result) {
    repose_fake_authorization_engine_t *engine = fake_engine(reference);
    pthread_mutex_lock(&engine->lock);
    engine->set_result_count += 1;
    switch (result) {
        case kAuthorizationResultAllow:
            engine->allow_count += 1;
            append_event(engine, REPOSE_FAKE_EVENT_ALLOW);
            break;
        case kAuthorizationResultDeny:
            engine->deny_count += 1;
            append_event(engine, REPOSE_FAKE_EVENT_DENY);
            break;
        case kAuthorizationResultUndefined:
            engine->undefined_count += 1;
            append_event(engine, REPOSE_FAKE_EVENT_UNDEFINED);
            break;
        case kAuthorizationResultUserCanceled:
            engine->user_canceled_count += 1;
            append_event(engine, REPOSE_FAKE_EVENT_USER_CANCELED);
            break;
    }
    engine->result_entered = true;
    pthread_cond_broadcast(&engine->changed);
    while (engine->block_result && !engine->release_result) {
        assert(pthread_cond_wait(&engine->changed, &engine->lock) == 0);
    }
    OSStatus status = engine->set_result_status;
    pthread_cond_broadcast(&engine->changed);
    pthread_mutex_unlock(&engine->lock);
    return status;
}

static OSStatus fake_request_interrupt(AuthorizationEngineRef reference) {
    repose_fake_authorization_engine_t *engine = fake_engine(reference);
    pthread_mutex_lock(&engine->lock);
    engine->interrupt_count += 1;
    append_event(engine, REPOSE_FAKE_EVENT_INTERRUPT);
    engine->interrupt_entered = true;
    pthread_cond_broadcast(&engine->changed);
    while (engine->block_interrupt && !engine->release_interrupt) {
        assert(pthread_cond_wait(&engine->changed, &engine->lock) == 0);
    }
    OSStatus status = engine->request_interrupt_status;
    pthread_mutex_unlock(&engine->lock);
    return status;
}

static OSStatus fake_did_deactivate(AuthorizationEngineRef reference) {
    repose_fake_authorization_engine_t *engine = fake_engine(reference);
    pthread_mutex_lock(&engine->lock);
    engine->did_deactivate_count += 1;
    append_event(engine, REPOSE_FAKE_EVENT_DID_DEACTIVATE);
    engine->deactivate_entered = true;
    pthread_cond_broadcast(&engine->changed);
    while (engine->block_deactivate && !engine->release_deactivate) {
        assert(pthread_cond_wait(&engine->changed, &engine->lock) == 0);
    }
    OSStatus status = engine->did_deactivate_status;
    pthread_cond_broadcast(&engine->changed);
    pthread_mutex_unlock(&engine->lock);
    return status;
}

const AuthorizationCallbacks repose_fake_authorization_callbacks = {
    .version = kAuthorizationCallbacksVersion,
    .SetResult = fake_set_result,
    .RequestInterrupt = fake_request_interrupt,
    .DidDeactivate = fake_did_deactivate,
};

void repose_fake_engine_init(repose_fake_authorization_engine_t *engine) {
    memset(engine, 0, sizeof(*engine));
    assert(pthread_mutex_init(&engine->lock, NULL) == 0);
    assert(pthread_cond_init(&engine->changed, NULL) == 0);
    engine->set_result_status = errAuthorizationSuccess;
    engine->request_interrupt_status = errAuthorizationSuccess;
    engine->did_deactivate_status = errAuthorizationSuccess;
}

void repose_fake_engine_destroy(repose_fake_authorization_engine_t *engine) {
    assert(pthread_cond_destroy(&engine->changed) == 0);
    assert(pthread_mutex_destroy(&engine->lock) == 0);
}

AuthorizationEngineRef repose_fake_engine_ref(repose_fake_authorization_engine_t *engine) {
    return (AuthorizationEngineRef)engine;
}

size_t repose_fake_engine_result_count(repose_fake_authorization_engine_t *engine,
                                       AuthorizationResult result) {
    size_t count = 0;
    pthread_mutex_lock(&engine->lock);
    switch (result) {
        case kAuthorizationResultAllow:
            count = engine->allow_count;
            break;
        case kAuthorizationResultDeny:
            count = engine->deny_count;
            break;
        case kAuthorizationResultUndefined:
            count = engine->undefined_count;
            break;
        case kAuthorizationResultUserCanceled:
            count = engine->user_canceled_count;
            break;
    }
    pthread_mutex_unlock(&engine->lock);
    return count;
}

size_t repose_fake_engine_total_result_count(repose_fake_authorization_engine_t *engine) {
    pthread_mutex_lock(&engine->lock);
    size_t count = engine->set_result_count;
    pthread_mutex_unlock(&engine->lock);
    return count;
}

size_t repose_fake_engine_interrupt_count(repose_fake_authorization_engine_t *engine) {
    pthread_mutex_lock(&engine->lock);
    size_t count = engine->interrupt_count;
    pthread_mutex_unlock(&engine->lock);
    return count;
}

size_t repose_fake_engine_deactivate_count(repose_fake_authorization_engine_t *engine) {
    pthread_mutex_lock(&engine->lock);
    size_t count = engine->did_deactivate_count;
    pthread_mutex_unlock(&engine->lock);
    return count;
}

static struct timespec realtime_deadline(uint32_t timeout_ms) {
    struct timespec deadline;
    assert(clock_gettime(CLOCK_REALTIME, &deadline) == 0);
    deadline.tv_sec += (time_t)(timeout_ms / 1000);
    deadline.tv_nsec += (long)(timeout_ms % 1000) * 1000000L;
    if (deadline.tv_nsec >= 1000000000L) {
        deadline.tv_sec += 1;
        deadline.tv_nsec -= 1000000000L;
    }
    return deadline;
}

static bool wait_for_count(repose_fake_authorization_engine_t *engine,
                           const size_t *count,
                           size_t expected,
                           uint32_t timeout_ms) {
    struct timespec deadline = realtime_deadline(timeout_ms);
    while (*count < expected) {
        int status = pthread_cond_timedwait(&engine->changed, &engine->lock, &deadline);
        if (status == ETIMEDOUT) {
            return false;
        }
        assert(status == 0);
    }
    return true;
}

bool repose_fake_engine_wait_for_results(repose_fake_authorization_engine_t *engine,
                                         size_t count,
                                         uint32_t timeout_ms) {
    pthread_mutex_lock(&engine->lock);
    bool reached = wait_for_count(engine, &engine->set_result_count, count, timeout_ms);
    pthread_mutex_unlock(&engine->lock);
    return reached;
}

bool repose_fake_engine_wait_for_interrupts(repose_fake_authorization_engine_t *engine,
                                            size_t count,
                                            uint32_t timeout_ms) {
    pthread_mutex_lock(&engine->lock);
    bool reached = wait_for_count(engine, &engine->interrupt_count, count, timeout_ms);
    pthread_mutex_unlock(&engine->lock);
    return reached;
}

repose_fake_callback_event_t repose_fake_engine_event_at(
    repose_fake_authorization_engine_t *engine,
    size_t index) {
    pthread_mutex_lock(&engine->lock);
    assert(index < engine->event_count);
    repose_fake_callback_event_t event = engine->events[index];
    pthread_mutex_unlock(&engine->lock);
    return event;
}

void repose_fake_engine_set_result_status(repose_fake_authorization_engine_t *engine,
                                          OSStatus status) {
    pthread_mutex_lock(&engine->lock);
    engine->set_result_status = status;
    pthread_mutex_unlock(&engine->lock);
}

void repose_fake_engine_set_interrupt_status(repose_fake_authorization_engine_t *engine,
                                             OSStatus status) {
    pthread_mutex_lock(&engine->lock);
    engine->request_interrupt_status = status;
    pthread_mutex_unlock(&engine->lock);
}

void repose_fake_engine_set_deactivate_status(repose_fake_authorization_engine_t *engine,
                                              OSStatus status) {
    pthread_mutex_lock(&engine->lock);
    engine->did_deactivate_status = status;
    pthread_mutex_unlock(&engine->lock);
}

void repose_fake_engine_block_interrupt(repose_fake_authorization_engine_t *engine) {
    pthread_mutex_lock(&engine->lock);
    engine->block_interrupt = true;
    engine->release_interrupt = false;
    engine->interrupt_entered = false;
    pthread_mutex_unlock(&engine->lock);
}

bool repose_fake_engine_wait_for_interrupt_entry(repose_fake_authorization_engine_t *engine,
                                                 uint32_t timeout_ms) {
    pthread_mutex_lock(&engine->lock);
    struct timespec deadline = realtime_deadline(timeout_ms);
    while (!engine->interrupt_entered) {
        int status = pthread_cond_timedwait(&engine->changed, &engine->lock, &deadline);
        if (status == ETIMEDOUT) {
            pthread_mutex_unlock(&engine->lock);
            return false;
        }
        assert(status == 0);
    }
    pthread_mutex_unlock(&engine->lock);
    return true;
}

void repose_fake_engine_release_interrupt(repose_fake_authorization_engine_t *engine) {
    pthread_mutex_lock(&engine->lock);
    engine->release_interrupt = true;
    pthread_cond_broadcast(&engine->changed);
    pthread_mutex_unlock(&engine->lock);
}

void repose_fake_engine_block_result(repose_fake_authorization_engine_t *engine) {
    pthread_mutex_lock(&engine->lock);
    engine->block_result = true;
    engine->release_result = false;
    engine->result_entered = false;
    pthread_mutex_unlock(&engine->lock);
}

bool repose_fake_engine_wait_for_result_entry(repose_fake_authorization_engine_t *engine,
                                              uint32_t timeout_ms) {
    pthread_mutex_lock(&engine->lock);
    struct timespec deadline = realtime_deadline(timeout_ms);
    while (!engine->result_entered) {
        int status = pthread_cond_timedwait(&engine->changed, &engine->lock, &deadline);
        if (status == ETIMEDOUT) {
            pthread_mutex_unlock(&engine->lock);
            return false;
        }
        assert(status == 0);
    }
    pthread_mutex_unlock(&engine->lock);
    return true;
}

void repose_fake_engine_release_result(repose_fake_authorization_engine_t *engine) {
    pthread_mutex_lock(&engine->lock);
    engine->release_result = true;
    pthread_cond_broadcast(&engine->changed);
    pthread_mutex_unlock(&engine->lock);
}

void repose_fake_engine_block_deactivate(repose_fake_authorization_engine_t *engine) {
    pthread_mutex_lock(&engine->lock);
    engine->block_deactivate = true;
    engine->release_deactivate = false;
    engine->deactivate_entered = false;
    pthread_mutex_unlock(&engine->lock);
}

bool repose_fake_engine_wait_for_deactivate_entry(
    repose_fake_authorization_engine_t *engine,
    uint32_t timeout_ms) {
    pthread_mutex_lock(&engine->lock);
    struct timespec deadline = realtime_deadline(timeout_ms);
    while (!engine->deactivate_entered) {
        int status = pthread_cond_timedwait(&engine->changed, &engine->lock, &deadline);
        if (status == ETIMEDOUT) {
            pthread_mutex_unlock(&engine->lock);
            return false;
        }
        assert(status == 0);
    }
    pthread_mutex_unlock(&engine->lock);
    return true;
}

void repose_fake_engine_release_deactivate(repose_fake_authorization_engine_t *engine) {
    pthread_mutex_lock(&engine->lock);
    engine->release_deactivate = true;
    pthread_cond_broadcast(&engine->changed);
    pthread_mutex_unlock(&engine->lock);
}
