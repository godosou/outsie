package ai.repose.mobile.unlock

import ai.repose.mobile.unlock.generated.ReposeUnlockHostApi
import io.flutter.plugin.common.BinaryMessenger
import io.flutter.embedding.engine.plugins.FlutterPlugin

class ReposeUnlockNativePlugin : FlutterPlugin {
    private var binaryMessenger: BinaryMessenger? = null

    override fun onAttachedToEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        val context = binding.applicationContext
        ReposeUnlockRuntime.initialize(context)
        ReposeUnlockHostApi.setUp(
            binding.binaryMessenger,
            FailClosedReposeUnlockHostApi { ReposeUnlockRuntime.availability(context) },
        )
        binaryMessenger = binding.binaryMessenger
    }

    override fun onDetachedFromEngine(binding: FlutterPlugin.FlutterPluginBinding) {
        binaryMessenger?.let { messenger -> ReposeUnlockHostApi.setUp(messenger, null) }
        binaryMessenger = null
    }
}
