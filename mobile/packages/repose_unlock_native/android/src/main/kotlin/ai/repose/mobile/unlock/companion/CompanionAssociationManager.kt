package ai.repose.mobile.unlock.companion

import android.annotation.TargetApi
import android.companion.CompanionDeviceManager
import android.companion.ObservingDevicePresenceRequest
import android.content.Context

interface AssociationIdStore {
    fun loadAssociationId(): Int?

    fun saveAssociationId(associationId: Int): Boolean

    fun clearAssociationId(): Boolean
}

fun interface AssociationCatalog {
    fun getMyAssociationIds(): List<Int>
}

fun interface AssociationPresenceObserver {
    fun startObservingAssociation(associationId: Int)
}

sealed interface AssociationReconciliation {
    data class Observing(val associationId: Int) : AssociationReconciliation

    data object NotConfigured : AssociationReconciliation

    data object Missing : AssociationReconciliation

    data object Ambiguous : AssociationReconciliation

    data object PersistenceUnavailable : AssociationReconciliation
}

class CompanionAssociationManager(
    private val store: AssociationIdStore,
    private val catalog: AssociationCatalog,
    private val observer: AssociationPresenceObserver,
) {
    fun configureAssociation(associationId: Int): AssociationReconciliation {
        if (associationId == DevicePresenceNoAssociation) {
            return AssociationReconciliation.Missing
        }
        return when (catalog.getMyAssociationIds().count { it == associationId }) {
            0 -> AssociationReconciliation.Missing
            1 -> {
                if (!store.saveAssociationId(associationId)) {
                    return AssociationReconciliation.PersistenceUnavailable
                }
                try {
                    observer.startObservingAssociation(associationId)
                } catch (error: RuntimeException) {
                    store.clearAssociationId()
                    throw error
                }
                AssociationReconciliation.Observing(associationId)
            }
            else -> AssociationReconciliation.Ambiguous
        }
    }

    fun reconcileAtStartup(): AssociationReconciliation {
        val currentIds = catalog.getMyAssociationIds()
        val persistedId = store.loadAssociationId() ?: return AssociationReconciliation.NotConfigured
        return when (currentIds.count { it == persistedId }) {
            0 -> {
                store.clearAssociationId()
                AssociationReconciliation.Missing
            }
            1 -> {
                observer.startObservingAssociation(persistedId)
                AssociationReconciliation.Observing(persistedId)
            }
            else -> AssociationReconciliation.Ambiguous
        }
    }
}

internal class SharedPreferencesAssociationIdStore(context: Context) : AssociationIdStore {
    private val preferences = context.getSharedPreferences(PREFERENCES_NAME, Context.MODE_PRIVATE)

    override fun loadAssociationId(): Int? = if (preferences.contains(ASSOCIATION_ID_KEY)) {
        preferences.getInt(ASSOCIATION_ID_KEY, DevicePresenceNoAssociation)
            .takeIf { it != DevicePresenceNoAssociation }
    } else {
        null
    }

    override fun saveAssociationId(associationId: Int): Boolean {
        require(associationId != DevicePresenceNoAssociation) {
            "the platform no-association sentinel cannot be persisted"
        }
        return preferences.edit().putInt(ASSOCIATION_ID_KEY, associationId).commit()
    }

    override fun clearAssociationId(): Boolean = preferences.edit()
        .remove(ASSOCIATION_ID_KEY)
        .commit()

    private companion object {
        const val PREFERENCES_NAME = "repose_unlock_native"
        const val ASSOCIATION_ID_KEY = "companion_association_id"
    }
}

@TargetApi(36)
internal class AndroidAssociationCatalog(
    private val manager: CompanionDeviceManager,
) : AssociationCatalog {
    override fun getMyAssociationIds(): List<Int> = manager.myAssociations.map { it.id }
}

@TargetApi(36)
internal class AndroidAssociationPresenceObserver(
    private val manager: CompanionDeviceManager,
) : AssociationPresenceObserver {
    override fun startObservingAssociation(associationId: Int) {
        val request = ObservingDevicePresenceRequest.Builder()
            .setAssociationId(associationId)
            .build()
        manager.startObservingDevicePresence(request)
    }
}
