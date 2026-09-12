package ai.repose.blespike

import android.content.Context
import android.view.Gravity
import android.view.View
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
import android.view.ViewGroup.LayoutParams.WRAP_CONTENT
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView

/** The product screens. CONTROL belongs to one Mac and opens from its card (design doc §04). */
enum class Screen { PAIRING, HOME, KEEPALIVE, CONTROL }

/** How screens ask the host to move between screens. */
interface Nav {
    fun go(screen: Screen)
    fun back()
}

/**
 * A built screen: its root view plus an optional hook the host calls whenever the BLE
 * service state changes, so a screen can refresh live bits (e.g. the advertise toggle)
 * without being rebuilt.
 */
class ScreenView(val root: View, val onState: (() -> Unit)? = null)

/**
 * Standard screen chrome: an optional back affordance, a large title, an optional lead
 * paragraph, then a scrolling column the caller fills. Keeps the four screens visually
 * consistent and edge-to-edge safe.
 */
fun screenScaffold(
    context: Context,
    pal: Palette,
    title: CharSequence,
    lead: CharSequence? = null,
    onBack: (() -> Unit)? = null,
    /** Set false where the screen supplies its own hero instead of a title. */
    showTitle: Boolean = true,
    build: (LinearLayout) -> Unit,
): View {
    val outer = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        setBackgroundColor(pal.background)
        layoutParams = LinearLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT)
    }

    // Outside the ScrollView: the name of the app does not scroll away.
    outer.addView(brandHeader(context, pal))

    if (onBack != null) {
        outer.addView(
            TextView(context).apply {
                text = "‹  返回"
                setTextColor(pal.accent)
                textSize = 16f
                setPadding(context.dp(20), context.dp(14), context.dp(20), context.dp(6))
                isClickable = true
                isFocusable = true
                setOnClickListener { onBack() }
            },
            Ui.lp(width = WRAP_CONTENT),
        )
    }

    val scroll = ScrollView(context).apply {
        isFillViewport = true
        layoutParams = LinearLayout.LayoutParams(MATCH_PARENT, 0, 1f)
    }
    val column = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        val h = context.dp(20)
        setPadding(h, context.dp(4), h, context.dp(28))
    }

    if (showTitle) {
        column.addView(Ui.title(context, pal, title))
        if (lead != null) {
            column.addView(Ui.body(context, pal, lead), Ui.lp(top = context.dp(10)))
        }
    }
    build(column)
    column.addView(passwordFallbackNote(context, pal), Ui.lp(top = context.dp(26)))

    scroll.addView(column)
    outer.addView(scroll)
    return outer
}

/** A single row inside a card: a leading dot/label column and trailing content. */
fun bulletRow(context: Context, pal: Palette, primary: CharSequence, secondary: CharSequence?): LinearLayout =
    LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        addView(Ui.heading(context, pal, primary))
        if (secondary != null) {
            addView(Ui.secondary(context, pal, secondary), Ui.lp(top = context.dp(3)))
        }
    }

/** Centred glyph in a soft, coloured circle — used for list/section icons. */
fun glyphCircle(context: Context, bg: Int, glyph: CharSequence, sizeDp: Int, textSp: Float, fg: Int): TextView =
    TextView(context).apply {
        text = glyph
        gravity = Gravity.CENTER
        setTextColor(fg)
        textSize = textSp
        background = Ui.rounded(bg, context.dpF(sizeDp / 2f))
        layoutParams = LinearLayout.LayoutParams(context.dp(sizeDp), context.dp(sizeDp))
    }
