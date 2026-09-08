package ai.repose.mobile.unlock

import android.app.Activity
import android.content.Intent
import android.os.Handler
import android.os.Looper
import ai.repose.mobile.unlock.companion.AndroidCompanionAssociationDriver
import ai.repose.mobile.unlock.companion.AssociationSetupFailure
import ai.repose.mobile.unlock.companion.AssociationSetupOutcome
import ai.repose.mobile.unlock.companion.CompanionAssociationCoordinator
import ai.repose.mobile.unlock.generated.FlutterError
import ai.repose.mobile.unlock.generated.ReposeUnlockHostApi
import io.flutter.embedding.engine.plugins.FlutterPlugin
import io.flutter.embedding.engine.plugins.activity.ActivityAware
import io.flutter.embedding.engine.plugins.activity.ActivityPluginBinding
import io.flutter.plugin.common.BinaryMessenger
import io.flutter.plugin.common.PluginRegistry

class ReposeUnlockNativePlugin :
    FlutterPlugin,
    ActivityAware,
    PluginRegistry.RequestPermissionsResultListener,
    PluginRegistry.ActivityResultListener {
    private val mainHandler = Handler(Looper.getMainLooper())
    private var binaryMessenger: BinaryMessenger? = null
    private var activityBinding: ActivityPluginBinding? = null
    private var associationDriver: AndroidCompanionAssociationDriver? = null
    private var associationCoordinator: CompanionAssociationCoordinator? = null
    private var hostApi: LifecycleReposeUnlockHostApi? = null

    override fun onAttachedToEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        val context = binding.applicationContext
        ReposeUnlockRuntime.initialize(context)
        val host = VariantReposeUnlockHostApiFactory.create(
            context = context,
            requestCompanionAssociation = ::requestCompanionAssociation,
        )
        ReposeUnlockHostApi.setUp(binding.binaryMessenger, host)
        hostApi = host
        binaryMessenger = binding.binaryMessenger
    }

    override fun onDetachedFromEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        detachActivity()
        binaryMessenger?.let { messenger -> ReposeUnlockHostApi.setUp(messenger, null) }
        hostApi?.close()
        hostApi = null
        binaryMessenger = null
    }

    override fun onAttachedToActivity(binding: ActivityPluginBinding) {
        attachActivity(binding)
    }

    override fun onDetachedFromActivityForConfigChanges() {
        detachActivity()
    }

    override fun onReattachedToActivityForConfigChanges(binding: ActivityPluginBinding) {
        attachActivity(binding)
    }

    override fun onDetachedFromActivity() {
        detachActivity()
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ): Boolean = associationCoordinator?.onBluetoothPermissionsResult(requestCode) ?: false

    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?): Boolean {
        val driver = associationDriver ?: return false
        return associationCoordinator?.onAssociationActivityResult(
            requestCode = requestCode,
            accepted = resultCode == Activity.RESULT_OK,
            associationId = driver.associationIdFromResult(data),
        ) ?: false
    }

    private fun attachActivity(binding: ActivityPluginBinding) {
        detachActivity()
        val applicationContext = binding.activity.applicationContext
        val driver = AndroidCompanionAssociationDriver(
            activity = binding.activity,
            configureRuntimeAssociation = { associationId, completion ->
                VariantReposeUnlockHostApiFactory.configureAssociation(
                    applicationContext,
                    associationId,
                    completion,
                )
            },
        )
        val coordinator = CompanionAssociationCoordinator(driver)
        binding.addRequestPermissionsResultListener(this)
        binding.addActivityResultListener(this)
        associationDriver = driver
        associationCoordinator = coordinator
        activityBinding = binding
    }

    private fun detachActivity() {
        activityBinding?.removeRequestPermissionsResultListener(this)
        activityBinding?.removeActivityResultListener(this)
        associationCoordinator?.detach()
        associationCoordinator = null
        associationDriver = null
        activityBinding = null
    }

    private fun requestCompanionAssociation(callback: (Result<Unit>) -> Unit) {
        mainHandler.post {
            val coordinator = associationCoordinator
            if (coordinator == null) {
                callback(Result.failure(FlutterError(
                    code = "activityUnavailable",
                    message = "Open Repose on your phone before starting system association.",
                    details = null,
                )))
                return@post
            }
            coordinator.requestAssociation { outcome -> callback(associationSetupResult(outcome)) }
        }
    }
}

internal fun associationSetupResult(outcome: AssociationSetupOutcome): Result<Unit> = when (outcome) {
    AssociationSetupOutcome.Associated -> Result.success(Unit)
    is AssociationSetupOutcome.Failed -> {
        val (code, message) = when (outcome.reason) {
            AssociationSetupFailure.BUSY ->
                "associationBusy" to "A companion association request is already open."
            AssociationSetupFailure.PERMISSION_DENIED ->
                "bluetoothPermissionDenied" to "Nearby devices access is required to find your Mac."
            AssociationSetupFailure.USER_CANCELLED ->
                "associationCancelled" to "No Mac was associated. You can try again."
            AssociationSetupFailure.ACTIVITY_DETACHED ->
                "activityUnavailable" to "The association screen closed before setup completed."
            AssociationSetupFailure.DISCOVERY_FAILED ->
                "associationDiscoveryFailed" to
                    "Repose could not find a Mac advertising the phone-key service."
            AssociationSetupFailure.CONFIGURATION_FAILED ->
                "associationConfigurationFailed" to
                    "Android did not confirm the companion association."
        }
        Result.failure(FlutterError(code = code, message = message, details = null))
    }
}
