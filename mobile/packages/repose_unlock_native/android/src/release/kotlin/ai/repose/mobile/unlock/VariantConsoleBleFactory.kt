package ai.repose.mobile.unlock
import android.content.Context
internal object VariantConsoleBleFactory {
    fun create(context: Context): ConsoleBleChannel = UnsupportedConsoleBleChannel()
}
