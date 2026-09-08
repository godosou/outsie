package ai.repose.mobile.unlock.companion

internal object ReposeBluetoothContract {
    const val SERVICE_UUID = "A53E0001-7A6B-4D59-9F2E-5245504F5345"
}

internal interface AssociationPrompt

internal interface AssociationDiscoveryEvents {
    fun onAssociationPending(prompt: AssociationPrompt)

    fun onAssociationCreated(associationId: Int)

    fun onFailure(message: String?)
}

internal enum class AssociationConfiguration {
    CONFIGURED,
    FAILED,
}

internal enum class AssociationSetupFailure {
    BUSY,
    PERMISSION_DENIED,
    USER_CANCELLED,
    ACTIVITY_DETACHED,
    DISCOVERY_FAILED,
    CONFIGURATION_FAILED,
}

internal sealed interface AssociationSetupOutcome {
    data object Associated : AssociationSetupOutcome

    data class Failed(val reason: AssociationSetupFailure) : AssociationSetupOutcome
}

internal interface CompanionAssociationDriver {
    fun missingBluetoothPermissions(): List<String>

    fun requestBluetoothPermissions(permissions: List<String>, requestCode: Int)

    fun discoverReposeMac(serviceUuid: String, events: AssociationDiscoveryEvents)

    fun launchAssociationPrompt(prompt: AssociationPrompt, requestCode: Int)

    fun configureAssociation(
        associationId: Int,
        completion: (AssociationConfiguration) -> Unit,
    )
}

internal class CompanionAssociationCoordinator(
    private val driver: CompanionAssociationDriver,
) {
    private var nextRequestId = 0
    private var pending: PendingRequest? = null

    fun requestAssociation(completion: (AssociationSetupOutcome) -> Unit) {
        if (pending != null) {
            completion(AssociationSetupOutcome.Failed(AssociationSetupFailure.BUSY))
            return
        }

        val requestId = ++nextRequestId
        val request = PendingRequest(
            id = requestId,
            permissionRequestCode = requestCode(requestId, PERMISSION_REQUEST_OFFSET),
            associationRequestCode = requestCode(requestId, ASSOCIATION_REQUEST_OFFSET),
            completion = completion,
        )
        pending = request
        val missingPermissions = driver.missingBluetoothPermissions()
        if (missingPermissions.isEmpty()) {
            discover(request)
            return
        }

        request.phase = RequestPhase.AWAITING_PERMISSION
        try {
            driver.requestBluetoothPermissions(
                missingPermissions,
                request.permissionRequestCode,
            )
        } catch (_: RuntimeException) {
            finish(request, AssociationSetupFailure.PERMISSION_DENIED)
        }
    }

    fun onBluetoothPermissionsResult(requestCode: Int): Boolean {
        val request = pending?.takeIf {
            it.permissionRequestCode == requestCode && it.phase == RequestPhase.AWAITING_PERMISSION
        } ?: return false
        if (driver.missingBluetoothPermissions().isNotEmpty()) {
            finish(request, AssociationSetupFailure.PERMISSION_DENIED)
        } else {
            discover(request)
        }
        return true
    }

    fun onAssociationActivityResult(
        requestCode: Int,
        accepted: Boolean,
        associationId: Int?,
    ): Boolean {
        val request = pending?.takeIf {
            it.associationRequestCode == requestCode &&
                it.phase == RequestPhase.AWAITING_ASSOCIATION
        } ?: return false
        if (!accepted) {
            finish(request, AssociationSetupFailure.USER_CANCELLED)
        } else if (associationId != null) {
            configure(request, associationId)
        } else {
            finish(request, AssociationSetupFailure.CONFIGURATION_FAILED)
        }
        return true
    }

    fun detach() {
        pending?.let { finish(it, AssociationSetupFailure.ACTIVITY_DETACHED) }
    }

    private fun discover(request: PendingRequest) {
        if (!isCurrent(request)) return
        request.phase = RequestPhase.DISCOVERING
        try {
            driver.discoverReposeMac(
                ReposeBluetoothContract.SERVICE_UUID,
                object : AssociationDiscoveryEvents {
                    override fun onAssociationPending(prompt: AssociationPrompt) {
                        if (!isCurrent(request) || request.phase != RequestPhase.DISCOVERING) return
                        request.phase = RequestPhase.AWAITING_ASSOCIATION
                        try {
                            driver.launchAssociationPrompt(
                                prompt,
                                request.associationRequestCode,
                            )
                        } catch (_: RuntimeException) {
                            finish(request, AssociationSetupFailure.DISCOVERY_FAILED)
                        }
                    }

                    override fun onAssociationCreated(associationId: Int) {
                        configure(request, associationId)
                    }

                    override fun onFailure(message: String?) {
                        if (isCurrent(request)) {
                            finish(request, AssociationSetupFailure.DISCOVERY_FAILED)
                        }
                    }
                },
            )
        } catch (_: RuntimeException) {
            finish(request, AssociationSetupFailure.DISCOVERY_FAILED)
        }
    }

    private fun configure(request: PendingRequest, associationId: Int) {
        if (!isCurrent(request) || request.phase == RequestPhase.CONFIGURING) return
        request.phase = RequestPhase.CONFIGURING
        try {
            driver.configureAssociation(associationId) { configuration ->
                if (!isCurrent(request)) return@configureAssociation
                when (configuration) {
                    AssociationConfiguration.CONFIGURED -> finish(request, outcome = AssociationSetupOutcome.Associated)
                    AssociationConfiguration.FAILED -> finish(
                        request,
                        AssociationSetupFailure.CONFIGURATION_FAILED,
                    )
                }
            }
        } catch (_: RuntimeException) {
            finish(request, AssociationSetupFailure.CONFIGURATION_FAILED)
        }
    }

    private fun finish(request: PendingRequest, failure: AssociationSetupFailure) {
        finish(request, AssociationSetupOutcome.Failed(failure))
    }

    private fun finish(request: PendingRequest, outcome: AssociationSetupOutcome) {
        if (!isCurrent(request)) return
        pending = null
        request.phase = RequestPhase.FINISHED
        request.completion(outcome)
    }

    private fun isCurrent(request: PendingRequest): Boolean = pending === request

    private fun requestCode(requestId: Int, offset: Int): Int =
        REQUEST_CODE_BASE + ((requestId - 1) % REQUEST_CODE_SLOTS) * 2 + offset

    private data class PendingRequest(
        val id: Int,
        val permissionRequestCode: Int,
        val associationRequestCode: Int,
        val completion: (AssociationSetupOutcome) -> Unit,
        var phase: RequestPhase = RequestPhase.CREATED,
    )

    private enum class RequestPhase {
        CREATED,
        AWAITING_PERMISSION,
        DISCOVERING,
        AWAITING_ASSOCIATION,
        CONFIGURING,
        FINISHED,
    }

    private companion object {
        const val REQUEST_CODE_BASE = 0x5200
        const val REQUEST_CODE_SLOTS = 512
        const val PERMISSION_REQUEST_OFFSET = 0
        const val ASSOCIATION_REQUEST_OFFSET = 1
    }
}
