package ai.repose.blespike

import android.content.Context
import android.content.res.ColorStateList
import android.graphics.Typeface
import android.graphics.drawable.GradientDrawable
import android.graphics.drawable.RippleDrawable
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
import android.view.ViewGroup.LayoutParams.WRAP_CONTENT
import android.widget.LinearLayout
import android.widget.TextView

/**
 * Small kit of view builders so the four screens read declaratively and share one look.
 * Everything is plain framework Views — no Compose, no AppCompat — matching the project.
 */
object Ui {

    /** So AlertDialogs follow the same light/dark mode as the rest of the shell. */
    fun dialogTheme(context: Context): Int =
        if (ReposeTheme.isNight(context)) android.R.style.Theme_Material_Dialog_Alert
        else android.R.style.Theme_Material_Light_Dialog_Alert

    fun rounded(color: Int, radiusPx: Float, strokeColor: Int? = null, strokePx: Int = 0): GradientDrawable =
        GradientDrawable().apply {
            setColor(color)
            cornerRadius = radiusPx
            if (strokeColor != null && strokePx > 0) setStroke(strokePx, strokeColor)
        }

    private fun ripple(context: Context, content: GradientDrawable, rippleColor: Int): RippleDrawable =
        RippleDrawable(ColorStateList.valueOf(rippleColor), content, content)

    /** A raised content card in the surface colour with generous rounded corners. */
    fun card(context: Context, pal: Palette): LinearLayout = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        background = rounded(pal.surface, context.dpF(22f), pal.divider, context.dp(1))
        val p = context.dp(20)
        setPadding(p, context.dp(18), p, context.dp(18))
        elevation = context.dpF(1.5f)
    }

    fun title(context: Context, pal: Palette, text: CharSequence): TextView = TextView(context).apply {
        this.text = text
        setTextColor(pal.textPrimary)
        setTextSize(TypedValue.COMPLEX_UNIT_SP, 25f)
        typeface = Typeface.create("sans-serif", Typeface.BOLD)
        setLineSpacing(context.dpF(4f), 1f)
    }

    fun heading(context: Context, pal: Palette, text: CharSequence): TextView = TextView(context).apply {
        this.text = text
        setTextColor(pal.textPrimary)
        setTextSize(TypedValue.COMPLEX_UNIT_SP, 17f)
        typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
    }

    fun body(context: Context, pal: Palette, text: CharSequence): TextView = TextView(context).apply {
        this.text = text
        setTextColor(pal.textPrimary)
        setTextSize(TypedValue.COMPLEX_UNIT_SP, 15f)
        setLineSpacing(context.dpF(5f), 1f)
    }

    fun secondary(context: Context, pal: Palette, text: CharSequence): TextView = TextView(context).apply {
        this.text = text
        setTextColor(pal.textSecondary)
        setTextSize(TypedValue.COMPLEX_UNIT_SP, 13.5f)
        setLineSpacing(context.dpF(4f), 1f)
    }

    fun primaryButton(context: Context, pal: Palette, text: CharSequence, onClick: () -> Unit): TextView =
        TextView(context).apply {
            this.text = text
            gravity = Gravity.CENTER
            setTextColor(pal.onAccent)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 16f)
            typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
            val v = context.dp(15)
            setPadding(context.dp(24), v, context.dp(24), v)
            minimumHeight = context.dp(54)
            background = ripple(context, rounded(pal.accent, context.dpF(16f)), pal.ripple)
            isClickable = true
            isFocusable = true
            setOnClickListener { onClick() }
        }

    fun ghostButton(context: Context, pal: Palette, text: CharSequence, onClick: () -> Unit): TextView =
        TextView(context).apply {
            this.text = text
            gravity = Gravity.CENTER
            setTextColor(pal.accent)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 16f)
            typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
            val v = context.dp(15)
            setPadding(context.dp(24), v, context.dp(24), v)
            minimumHeight = context.dp(54)
            background = ripple(
                context,
                rounded(0x00000000, context.dpF(16f), pal.divider, context.dp(1)),
                pal.ripple,
            )
            isClickable = true
            isFocusable = true
            setOnClickListener { onClick() }
        }

    /** A calm, warm-amber advisory block. Never red — Outsie has no destructive-red anywhere. */
    fun amberNote(context: Context, pal: Palette, text: CharSequence): TextView = TextView(context).apply {
        this.text = text
        setTextColor(pal.amberText)
        setTextSize(TypedValue.COMPLEX_UNIT_SP, 13.5f)
        setLineSpacing(context.dpF(5f), 1f)
        val h = context.dp(16)
        setPadding(h, context.dp(14), h, context.dp(14))
        background = rounded(pal.amberBg, context.dpF(14f), pal.amberBorder, context.dp(1))
    }

    /** A soft sage information block. */
    fun infoNote(context: Context, pal: Palette, text: CharSequence): TextView = TextView(context).apply {
        this.text = text
        setTextColor(pal.infoText)
        setTextSize(TypedValue.COMPLEX_UNIT_SP, 13.5f)
        setLineSpacing(context.dpF(5f), 1f)
        val h = context.dp(16)
        setPadding(h, context.dp(14), h, context.dp(14))
        background = rounded(pal.infoBg, context.dpF(14f), null, 0)
    }

    fun divider(context: Context, pal: Palette): View = View(context).apply {
        setBackgroundColor(pal.divider)
        layoutParams = LinearLayout.LayoutParams(MATCH_PARENT, context.dp(1))
    }

    fun spacer(context: Context, heightDp: Int): View = View(context).apply {
        layoutParams = LinearLayout.LayoutParams(MATCH_PARENT, context.dp(heightDp))
    }

    fun lp(
        width: Int = MATCH_PARENT,
        height: Int = WRAP_CONTENT,
        top: Int = 0,
        bottom: Int = 0,
        left: Int = 0,
        right: Int = 0,
    ): LinearLayout.LayoutParams = LinearLayout.LayoutParams(width, height).apply {
        setMargins(left, top, right, bottom)
    }
}
