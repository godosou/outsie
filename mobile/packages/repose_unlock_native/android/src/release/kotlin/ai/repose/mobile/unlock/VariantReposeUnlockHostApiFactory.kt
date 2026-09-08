package ai.repose.mobile.unlock

import android.content.Context
import ai.repose.mobile.unlock.companion.AssociationConfiguration
import ai.repose.mobile.unlock.companion.AssociationReconciliation

internal object VariantReposeUnlockHostApiFactory {
    fun create(
        context: Context,
        requestCompanionAssociation: ((Result<Unit>) -> Unit) -> Unit,
    ): LifecycleReposeUnlockHostApi = FailClosedReposeUnlockHostApi(
        requestCompanionAssociation = requestCompanionAssociation,
        availability = { ReposeUnlockRuntime.availability(context) },
    )

    fun configureAssociation(
        context: Context,
        associationId: Int,
        completion: (AssociationConfiguration) -> Unit,
    ) {
        ReposeUnlockRuntime.configureAssociation(context, associationId) { result ->
            completion(
                if (result is AssociationReconciliation.Observing) {
                    AssociationConfiguration.CONFIGURED
                } else {
                    AssociationConfiguration.FAILED
                },
            )
        }
    }
}
