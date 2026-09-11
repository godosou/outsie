package ai.repose.blespike

import android.content.Context
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.ColorFilter
import android.graphics.Paint
import android.graphics.PixelFormat
import android.graphics.RectF
import android.graphics.Typeface
import android.graphics.drawable.Drawable
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
import android.view.ViewGroup.LayoutParams.WRAP_CONTENT
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.TextView

/**
 * The Outsie mark: four petals around a hole.
 *
 * Ported from `ReposeBrandMark` in mobile/lib/app/repose_theme.dart, ovals and
 * angles unchanged, so the phone draws the same shape as the desktop app rather
 * than an emoji standing in for a logo.
 *
 * The centre is punched with the background colour rather than left transparent:
 * these sit on solid surfaces, and a real hole would show whatever is behind
 * the whole window.
 */
class PetalMark(private val color: Int, private val background: Int) : Drawable() {
    private val paint = Paint(Paint.ANTI_ALIAS_FLAG)
    private val oval = RectF(-7f, -25f, 7f, 5f)

    override fun draw(canvas: Canvas) {
        val size = bounds.width().toFloat()
        if (size <= 0f) return
        canvas.save()
        canvas.translate(bounds.exactCenterX(), bounds.exactCenterY())
        // The Flutter original is authored on a 64pt grid.
        canvas.scale(size / 64f, size / 64f)
        paint.color = color
        for (angle in intArrayOf(-40, 40, 130, 220)) {
            canvas.save()
            canvas.rotate(angle.toFloat())
            canvas.drawOval(oval, paint)
            canvas.restore()
        }
        paint.color = background
        canvas.drawCircle(0f, 0f, 5f, paint)
        canvas.restore()
    }

    override fun setAlpha(alpha: Int) { paint.alpha = alpha }
    override fun setColorFilter(colorFilter: ColorFilter?) { paint.colorFilter = colorFilter }
    @Deprecated("Deprecated in Java")
    override fun getOpacity(): Int = PixelFormat.TRANSLUCENT
}

/**
 * The header every screen wears: mark, wordmark, and what this app is.
 *
 * This is the single biggest reason the previous build read as a debug tool.
 * Each screen opened straight into a bare bold sentence on a flat background —
 * no name, no mark, nothing identifying which of two similar-looking apps you
 * had opened. A person deciding whether to trust this thing with their Mac's
 * lock screen should be able to tell at a glance what they are looking at.
 */
fun brandHeader(context: Context, pal: Palette): View {
    val row = LinearLayout(context).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        setPadding(context.dp(20), context.dp(14), context.dp(20), context.dp(10))
    }
    row.addView(
        ImageView(context).apply {
            setImageDrawable(PetalMark(pal.accent, pal.background))
        },
        LinearLayout.LayoutParams(context.dp(30), context.dp(30)),
    )
    val words = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        setPadding(context.dp(12), 0, 0, 0)
    }
    words.addView(
        TextView(context).apply {
            text = "outsie."
            setTextColor(pal.textPrimary)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 21f)
            typeface = Typeface.create("sans-serif", Typeface.BOLD)
            letterSpacing = -0.01f
        },
    )
    words.addView(
        TextView(context).apply {
            text = "手机钥匙"
            setTextColor(pal.textSecondary)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 11.5f)
            letterSpacing = 0.06f
        },
    )
    row.addView(words, LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f))
    return row
}

/**
 * The hero: a tinted card carrying a chip, a badge, a headline and one line
 * under it.
 *
 * A card rather than loose views on the background, because a headline floating
 * on a flat colour has nothing to say where the state ends and the controls
 * begin — which is exactly how the old pairing screen ended up as four stacked
 * paragraphs.
 */
fun heroCard(
    context: Context,
    pal: Palette,
    chip: String,
    glyph: String,
    headline: String,
    body: String,
    muted: Boolean = false,
): LinearLayout {
    val card = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        background = Ui.rounded(if (muted) pal.surfaceMuted else pal.accentSoft, context.dpF(24f))
        setPadding(context.dp(22), context.dp(20), context.dp(22), context.dp(24))
    }
    card.addView(
        TextView(context).apply {
            text = chip
            setTextColor(pal.infoText)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 11.5f)
            letterSpacing = 0.08f
            val h = context.dp(12)
            setPadding(h, context.dp(6), h, context.dp(6))
            background = Ui.rounded(pal.surface, context.dpF(20f), pal.divider, context.dp(1))
        },
        Ui.lp(width = WRAP_CONTENT),
    )
    card.addView(
        glyphCircle(context, pal.surface, glyph, 78, 32f, pal.accent).apply {
            (layoutParams as LinearLayout.LayoutParams).gravity = Gravity.CENTER_HORIZONTAL
            (layoutParams as LinearLayout.LayoutParams).topMargin = context.dp(18)
        },
    )
    card.addView(
        TextView(context).apply {
            text = headline
            gravity = Gravity.CENTER
            setTextColor(pal.textPrimary)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 22f)
            typeface = Typeface.create("sans-serif", Typeface.BOLD)
            setLineSpacing(context.dpF(3f), 1f)
        },
        Ui.lp(top = context.dp(16)),
    )
    card.addView(
        TextView(context).apply {
            text = body
            gravity = Gravity.CENTER
            setTextColor(pal.textSecondary)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 13.5f)
            setLineSpacing(context.dpF(5f), 1f)
        },
        Ui.lp(top = context.dp(8)),
    )
    return card
}

/**
 * Three labelled steps with a rule between them, showing where you are.
 *
 * Borrowed from the Flutter shell's 安全检查 / 设备配对 / 距离校准 row. Ours
 * names the three things that actually have to be true — paired, switched on,
 * within range — because a step people cannot complete is worse than no step at
 * all: the old app listed 距离校准, which is not implemented.
 */
fun stepRow(context: Context, pal: Palette, labels: List<String>, activeIndex: Int): View {
    val card = Ui.card(context, pal).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        setPadding(context.dp(8), context.dp(14), context.dp(8), context.dp(14))
    }
    labels.forEachIndexed { i, label ->
        if (i > 0) {
            card.addView(
                View(context).apply { setBackgroundColor(pal.divider) },
                LinearLayout.LayoutParams(0, context.dp(1), 0.4f).apply {
                    gravity = Gravity.CENTER_VERTICAL
                },
            )
        }
        val done = i < activeIndex
        val active = i == activeIndex
        val cell = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER_HORIZONTAL
        }
        cell.addView(
            glyphCircle(
                context,
                if (active) pal.accent else pal.accentSoft,
                if (done) "✓" else "${i + 1}",
                26,
                12f,
                if (active) pal.onAccent else pal.accent,
            ),
        )
        cell.addView(
            TextView(context).apply {
                text = label
                gravity = Gravity.CENTER
                setTextColor(if (active) pal.textPrimary else pal.textSecondary)
                setTextSize(TypedValue.COMPLEX_UNIT_SP, 11.5f)
                maxLines = 1
            },
            Ui.lp(width = WRAP_CONTENT, top = context.dp(7)),
        )
        card.addView(cell, LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f))
    }
    return card
}

/**
 * A card that opens with a small tinted badge and a heading, the way the
 * Flutter shell's section cards do.
 */
fun sectionCard(context: Context, pal: Palette, glyph: String, heading: String): LinearLayout {
    val card = Ui.card(context, pal)
    val head = LinearLayout(context).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
    }
    head.addView(
        TextView(context).apply {
            text = glyph
            gravity = Gravity.CENTER
            setTextColor(pal.accent)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 15f)
            background = Ui.rounded(pal.accentSoft, context.dpF(11f))
            layoutParams = LinearLayout.LayoutParams(context.dp(36), context.dp(36))
        },
    )
    head.addView(
        Ui.heading(context, pal, heading),
        LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f).apply { leftMargin = context.dp(12) },
    )
    card.addView(head)
    return card
}

/**
 * The line at the bottom of every screen.
 *
 * The single most important sentence in this app, and it belongs everywhere:
 * whatever is broken, misconfigured or half-paired, the password still works.
 * Someone whose phone key is not behaving needs to know they are not locked out
 * before they need to know why.
 */
fun passwordFallbackNote(context: Context, pal: Palette): TextView =
    Ui.secondary(context, pal, "手机钥匙用不了的时候，Mac 密码照常能登录。").apply {
        gravity = Gravity.CENTER
        setTextSize(TypedValue.COMPLEX_UNIT_SP, 12f)
        layoutParams = LinearLayout.LayoutParams(MATCH_PARENT, WRAP_CONTENT)
    }

/**
 * A collapsed 技术细节 block carrying the key fingerprint.
 *
 * The fingerprint used to be the headline of two screens, under the label
 * 配对编号, immediately after a flow whose whole point was comparing six
 * different digits. Two opaque codes, no way to tell from the screen which one
 * mattered — and being unsure about that is exactly the confusion a man in the
 * middle needs.
 *
 * It is not deleted, because it is the only way to check that two devices hold
 * the same key after the fact. It is just no longer competing for attention
 * with the one number a person is actually required to read.
 */
fun techDetails(context: Context, pal: Palette, fingerprint: String?): View {
    val wrap = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
    val body = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        visibility = View.GONE
    }
    val toggle = TextView(context).apply {
        text = "＋ 技术细节"
        setTextColor(pal.textSecondary)
        setTextSize(TypedValue.COMPLEX_UNIT_SP, 12f)
        setPadding(context.dp(4), context.dp(10), context.dp(4), context.dp(10))
        isClickable = true
        isFocusable = true
        setOnClickListener {
            val open = body.visibility == View.VISIBLE
            body.visibility = if (open) View.GONE else View.VISIBLE
            text = if (open) "＋ 技术细节" else "－ 技术细节"
        }
    }
    body.addView(Ui.secondary(context, pal, "密钥指纹"), Ui.lp(top = context.dp(4)))
    body.addView(
        TextView(context).apply {
            text = fingerprint ?: "????????"
            setTextColor(pal.textPrimary)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 18f)
            typeface = Typeface.create("monospace", Typeface.BOLD)
            letterSpacing = 0.14f
            maxLines = 1
        },
        Ui.lp(top = context.dp(4)),
    )
    body.addView(
        Ui.secondary(
            context,
            pal,
            "Mac 上「配对完成 → 技术细节」里是同一串。核对它不是必须的——" +
                "配对时那六位数字已经做完了这件事。",
        ),
        Ui.lp(top = context.dp(8)),
    )
    wrap.addView(toggle, Ui.lp(width = WRAP_CONTENT))
    wrap.addView(body)
    return wrap
}
