package ai.repose.mobile.unlock

import android.companion.CompanionDeviceManager
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import ai.repose.mobile.unlock.companion.AndroidAssociationCatalog
import ai.repose.mobile.unlock.companion.AndroidAssociationPresenceObserver
import ai.repose.mobile.unlock.companion.AssociationReconciliation
import ai.repose.mobile.unlock.companion.CompanionAssociationManager
import ai.repose.mobile.unlock.companion.PresenceEventRouter
import ai.repose.mobile.unlock.companion.PresenceTransition
import ai.repose.mobile.unlock.companion.SharedPreferencesAssociationIdStore
import ai.repose.mobile.unlock.companion.SerialPresenceDispatcher
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicReference

internal object ReposeUnlockRuntime {
    private val lock = Any()
    private val state = AtomicReference<RuntimeState?>()

    fun initialize(context: Context) {
        if (Build.VERSION.SDK_INT < 36) return
        val applicationContext = context.applicationContext
        if (!applicationContext.packageManager.hasSystemFeature(
                PackageManager.FEATURE_COMPANION_DEVICE_SETUP,
            )
        ) {
            return
        }
        if (state.get() != null) return

        synchronized(lock) {
            if (state.get() != null) return
            val executor = Executors.newSingleThreadExecutor { work ->
                Thread(work, "repose-native-runtime").apply { isDaemon = true }
            }
            val activeAssociationId = AtomicReference<Int?>()
            val lastPresence = AtomicReference<PresenceTransition?>()
            val associationStore = SharedPreferencesAssociationIdStore(applicationContext)
            val router = PresenceEventRouter(
                activeAssociationId = activeAssociationId::get,
                enqueue = lastPresence::set,
                removeAssociation = {
                    associationStore.clearAssociationId()
                    activeAssociationId.set(null)
                    lastPresence.set(null)
                },
            )
            val dispatcher = SerialPresenceDispatcher(
                execute = { task -> executor.execute(task) },
                reconcileAtStartup = {
                    reconcile(applicationContext, associationStore, activeAssociationId)
                },
                route = router::route,
            )
            val runtime = RuntimeState(
                dispatcher,
                activeAssociationId,
                lastPresence,
                associationStore,
            )
            state.set(runtime)
        }
    }

    fun enqueuePresenceEvent(context: Context, associationId: Int, event: Int) {
        initialize(context)
        state.get()?.dispatcher?.enqueue(associationId, event)
    }

    internal fun availability(context: Context): NativeRuntimeAvailability {
        if (Build.VERSION.SDK_INT < 36) return NativeRuntimeAvailability.UNSUPPORTED_API
        val applicationContext = context.applicationContext
        if (!applicationContext.packageManager.hasSystemFeature(
                PackageManager.FEATURE_COMPANION_DEVICE_SETUP,
            )
        ) {
            return NativeRuntimeAvailability.COMPANION_FEATURE_UNAVAILABLE
        }
        initialize(applicationContext)
        return if (state.get()?.activeAssociationId?.get() == null) {
            NativeRuntimeAvailability.ASSOCIATION_NOT_CONFIGURED
        } else {
            NativeRuntimeAvailability.TRANSPORT_NOT_IMPLEMENTED
        }
    }

    internal fun configureAssociation(
        context: Context,
        associationId: Int,
        completion: (AssociationReconciliation) -> Unit,
    ) {
        initialize(context)
        val applicationContext = context.applicationContext
        val runtime = state.get()
        if (runtime == null) {
            completion(AssociationReconciliation.PersistenceUnavailable)
            return
        }
        runtime.dispatcher.executeControl {
            val result = runCatching {
                associationManager(applicationContext, runtime.associationStore)
                    ?.configureAssociation(associationId)
                    ?: AssociationReconciliation.PersistenceUnavailable
            }.getOrDefault(AssociationReconciliation.PersistenceUnavailable)
            runtime.activeAssociationId.set(
                (result as? AssociationReconciliation.Observing)?.associationId,
            )
            completion(result)
        }
    }

    private fun reconcile(
        context: Context,
        associationStore: SharedPreferencesAssociationIdStore,
        activeAssociationId: AtomicReference<Int?>,
    ) {
        val associationManager = associationManager(context, associationStore) ?: return
        val reconciliation = runCatching { associationManager.reconcileAtStartup() }.getOrNull()
        activeAssociationId.set(
            (reconciliation as? AssociationReconciliation.Observing)?.associationId,
        )
    }

    private fun associationManager(
        context: Context,
        associationStore: SharedPreferencesAssociationIdStore,
    ): CompanionAssociationManager? {
        val manager = context.getSystemService(CompanionDeviceManager::class.java) ?: return null
        return CompanionAssociationManager(
            associationStore,
            AndroidAssociationCatalog(manager),
            AndroidAssociationPresenceObserver(manager),
        )
    }

    private data class RuntimeState(
        val dispatcher: SerialPresenceDispatcher,
        val activeAssociationId: AtomicReference<Int?>,
        val lastPresence: AtomicReference<PresenceTransition?>,
        val associationStore: SharedPreferencesAssociationIdStore,
    )
}
