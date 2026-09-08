package ai.repose.mobile.unlock

import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel

internal interface ConsoleBleChannel : MethodChannel.MethodCallHandler {
    fun close()
}

internal class UnsupportedConsoleBleChannel : ConsoleBleChannel {
    override fun onMethodCall(call: MethodCall, result: MethodChannel.Result) {
        if (call.method == "disconnect") result.success(null)
        else result.error("unsupported", "Bluetooth App controls require the supported Android debug companion.", null)
    }
    override fun close() = Unit
}
