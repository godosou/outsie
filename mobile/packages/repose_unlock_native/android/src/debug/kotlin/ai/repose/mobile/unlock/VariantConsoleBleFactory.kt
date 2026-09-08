package ai.repose.mobile.unlock
import android.content.Context
import ai.repose.mobile.unlock.console.AndroidConsoleBleChannel
internal object VariantConsoleBleFactory {
    fun create(context: Context): ConsoleBleChannel = AndroidConsoleBleChannel(context.applicationContext)
}
