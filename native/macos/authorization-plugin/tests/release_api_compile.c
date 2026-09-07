#include "ipc_client.h"

bool repose_release_exchange_contract(repose_session_selector_t selector,
                                      const repose_deadline_t *deadline,
                                      repose_ipc_initial_result_t *result) {
    return repose_ipc_exchange(selector, deadline, result);
}
