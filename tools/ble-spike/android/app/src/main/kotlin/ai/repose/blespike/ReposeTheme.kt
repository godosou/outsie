package ai.repose.blespike

import android.content.Context
import android.content.res.Configuration
import android.util.TypedValue

/**
 * Repose design tokens. Forest green + warm off-white in light mode, with a matching
 * dark variant. Every colour here is resolved once per screen build from the current
 * night-mode configuration; the views paint themselves explicitly, so we never depend
 * on the framework theme's default text/background colours.
 *
 * No destructive red anywhere — warnings use a warm amber.
 */
data class Palette(
    val background: Int,
    val surface: Int,
    val surfaceMuted: Int,
    val accent: Int,
    val accentSoft: Int,
    val onAccent: Int,
    val textPrimary: Int,
    val textSecondary: Int,
    val divider: Int,
    val amberBg: Int,
    val amberBorder: Int,
    val amberText: Int,
    val infoBg: Int,
    val infoText: Int,
    val ringSoft: Int,
    val ripple: Int,
)

object ReposeTheme {

    // Lifted from the Flutter shell's reposeTheme(), which shares its palette
    // with src/styles.css -- so the phone, the Mac app and the site are the same
    // product rather than three things with a similar mood.
    private val LIGHT = Palette(
        background = 0xFFFBFCF9.toInt(),
        surface = 0xFFFFFFFF.toInt(),
        surfaceMuted = 0xFFEFF3E9.toInt(),
        accent = 0xFF526A43.toInt(),
        accentSoft = 0xFFEFF3E9.toInt(),
        onAccent = 0xFFFFFFFF.toInt(),
        textPrimary = 0xFF303B32.toInt(),
        textSecondary = 0xFF677160.toInt(),
        divider = 0xFFE9ECE4.toInt(),
        amberBg = 0xFFFAF4E9.toInt(),
        amberBorder = 0xFFEDE1C9.toInt(),
        amberText = 0xFF8A6F42.toInt(),
        infoBg = 0xFFEFF3E9.toInt(),
        infoText = 0xFF52604A.toInt(),
        ringSoft = 0xFFDFE7D4.toInt(),
        ripple = 0x33526A43,
    )

    private val DARK = Palette(
        background = 0xFF202B24.toInt(),
        surface = 0xFF26322A.toInt(),
        surfaceMuted = 0xFF303E2B.toInt(),
        accent = 0xFFA9C391.toInt(),
        accentSoft = 0xFF303E2B.toInt(),
        onAccent = 0xFF202B24.toInt(),
        textPrimary = 0xFFD5DECE.toInt(),
        textSecondary = 0xFFABB9A0.toInt(),
        divider = 0xFF43513E.toInt(),
        amberBg = 0xFF2E2C20.toInt(),
        amberBorder = 0xFF4C4432.toInt(),
        amberText = 0xFFD9C08A.toInt(),
        infoBg = 0xFF303E2B.toInt(),
        infoText = 0xFFB4BCA6.toInt(),
        ringSoft = 0xFF35442F.toInt(),
        ripple = 0x44A9C391,
    )

    fun isNight(context: Context): Boolean =
        (context.resources.configuration.uiMode and Configuration.UI_MODE_NIGHT_MASK) ==
            Configuration.UI_MODE_NIGHT_YES

    fun of(context: Context): Palette = if (isNight(context)) DARK else LIGHT
}

fun Context.dp(value: Int): Int =
    TypedValue.applyDimension(
        TypedValue.COMPLEX_UNIT_DIP,
        value.toFloat(),
        resources.displayMetrics,
    ).toInt()

fun Context.dpF(value: Float): Float =
    TypedValue.applyDimension(TypedValue.COMPLEX_UNIT_DIP, value, resources.displayMetrics)
