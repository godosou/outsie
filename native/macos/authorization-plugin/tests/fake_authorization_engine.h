#ifndef REPOSE_FAKE_AUTHORIZATION_ENGINE_H
#define REPOSE_FAKE_AUTHORIZATION_ENGINE_H

#include <Security/AuthorizationPlugin.h>
#include <pthread.h>
#include <stdbool.h>
#include <stddef.h>

#define REPOSE_FAKE_MAX_EVENTS 64u

typedef enum repose_fake_callback_event {
    REPOSE_FAKE_EVENT_ALLOW,
    REPOSE_FAKE_EVENT_DENY,
    REPOSE_FAKE_EVENT_UNDEFINED,
    REPOSE_FAKE_EVENT_USER_CANCELED,
    REPOSE_FAKE_EVENT_INTERRUPT,
    REPOSE_FAKE_EVENT_DID_DEACTIVATE,
} repose_fake_callback_event_t;

typedef struct repose_fake_authorization_engine {
    pthread_mutex_t lock;
    pthread_cond_t changed;
    size_t set_result_count;
    size_t allow_count;
    size_t deny_count;
    size_t undefined_count;
    size_t user_canceled_count;
    size_t interrupt_count;
    size_t did_deactivate_count;
    repose_fake_callback_event_t events[REPOSE_FAKE_MAX_EVENTS];
    size_t event_count;
    OSStatus set_result_status;
    OSStatus request_interrupt_status;
    OSStatus did_deactivate_status;
    bool block_interrupt;
    bool interrupt_entered;
    bool release_interrupt;
    bool block_result;
    bool result_entered;
    bool release_result;
    bool block_deactivate;
    bool deactivate_entered;
    bool release_deactivate;
} repose_fake_authorization_engine_t;

extern const AuthorizationCallbacks repose_fake_authorization_callbacks;

void repose_fake_engine_init(repose_fake_authorization_engine_t *engine);
void repose_fake_engine_destroy(repose_fake_authorization_engine_t *engine);
AuthorizationEngineRef repose_fake_engine_ref(repose_fake_authorization_engine_t *engine);
size_t repose_fake_engine_result_count(repose_fake_authorization_engine_t *engine,
                                       AuthorizationResult result);
size_t repose_fake_engine_total_result_count(repose_fake_authorization_engine_t *engine);
size_t repose_fake_engine_interrupt_count(repose_fake_authorization_engine_t *engine);
size_t repose_fake_engine_deactivate_count(repose_fake_authorization_engine_t *engine);
bool repose_fake_engine_wait_for_results(repose_fake_authorization_engine_t *engine,
                                         size_t count,
                                         uint32_t timeout_ms);
bool repose_fake_engine_wait_for_interrupts(repose_fake_authorization_engine_t *engine,
                                            size_t count,
                                            uint32_t timeout_ms);
repose_fake_callback_event_t repose_fake_engine_event_at(
    repose_fake_authorization_engine_t *engine,
    size_t index);
void repose_fake_engine_set_result_status(repose_fake_authorization_engine_t *engine,
                                          OSStatus status);
void repose_fake_engine_set_interrupt_status(repose_fake_authorization_engine_t *engine,
                                             OSStatus status);
void repose_fake_engine_set_deactivate_status(repose_fake_authorization_engine_t *engine,
                                              OSStatus status);
void repose_fake_engine_block_interrupt(repose_fake_authorization_engine_t *engine);
bool repose_fake_engine_wait_for_interrupt_entry(repose_fake_authorization_engine_t *engine,
                                                 uint32_t timeout_ms);
void repose_fake_engine_release_interrupt(repose_fake_authorization_engine_t *engine);
void repose_fake_engine_block_result(repose_fake_authorization_engine_t *engine);
bool repose_fake_engine_wait_for_result_entry(repose_fake_authorization_engine_t *engine,
                                              uint32_t timeout_ms);
void repose_fake_engine_release_result(repose_fake_authorization_engine_t *engine);
void repose_fake_engine_block_deactivate(repose_fake_authorization_engine_t *engine);
bool repose_fake_engine_wait_for_deactivate_entry(
    repose_fake_authorization_engine_t *engine,
    uint32_t timeout_ms);
void repose_fake_engine_release_deactivate(repose_fake_authorization_engine_t *engine);

#endif
