package ai.repose.mobile.unlock.pairing

import ai.repose.mobile.unlock.companion.canonicalBluetoothAddress

internal data class DebugCompanionAssociation(
    val associationId: Int?,
    val deviceAddress: String?,
)

internal object DebugAssociationSelector {
    fun select(
        candidates: List<DebugCompanionAssociation>,
        preferred: DebugCompanionAssociation?,
    ): DebugCompanionAssociation? {
        val validCandidates = candidates.mapNotNull(::validated)
        if (preferred == null) return validCandidates.singleOrNull()
        val validPreferred = validated(preferred) ?: return null
        return validCandidates.singleOrNull { candidate ->
            (validPreferred.associationId == null ||
                candidate.associationId == validPreferred.associationId) &&
                (validPreferred.deviceAddress == null ||
                    candidate.deviceAddress == validPreferred.deviceAddress)
        }
    }

    private fun validated(
        candidate: DebugCompanionAssociation,
    ): DebugCompanionAssociation? {
        val associationId = candidate.associationId?.takeIf { it >= 0 }
        val address = canonicalBluetoothAddress(candidate.deviceAddress)
        if (associationId == null && address == null) return null
        return DebugCompanionAssociation(associationId, address)
    }
}
