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

    private val LIGHT = Palette(
        background = 0xFFF1EDE3.toInt(),
        surface = 0xFFFBFAF5.toInt(),
        surfaceMuted = 0xFFF3EFE6.toInt(),
        accent = 0xFF526A43.toInt(),
        accentSoft = 0xFFE4EADD.toInt(),
        onAccent = 0xFFF7F5EE.toInt(),
        textPrimary = 0xFF2B2E27.toInt(),
        textSecondary = 0xFF6B6F62.toInt(),
        divider = 0xFFE2DED2.toInt(),
        amberBg = 0xFFF7EFE2.toInt(),
        amberBorder = 0xFFECDFC8.toInt(),
        amberText = 0xFF9C7F4E.toInt(),
        infoBg = 0xFFE7ECDF.toInt(),
        infoText = 0xFF52604A.toInt(),
        ringSoft = 0xFFDCE6D0.toInt(),
        ripple = 0x33526A43,
    )

    private val DARK = Palette(
        background = 0xFF14160E.toInt(),
        surface = 0xFF20231A.toInt(),
        surfaceMuted = 0xFF1A1D14.toInt(),
        accent = 0xFF9CBB7F.toInt(),
        accentSoft = 0xFF2A3122.toInt(),
        onAccent = 0xFF16180F.toInt(),
        textPrimary = 0xFFECEBE0.toInt(),
        textSecondary = 0xFFA7AC98.toInt(),
        divider = 0xFF2E3227.toInt(),
        amberBg = 0xFF2C2A1E.toInt(),
        amberBorder = 0xFF4A4230.toInt(),
        amberText = 0xFFD9C08A.toInt(),
        infoBg = 0xFF232A1F.toInt(),
        infoText = 0xFFB4BCA6.toInt(),
        ringSoft = 0xFF2E3826.toInt(),
        ripple = 0x449CBB7F,
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
