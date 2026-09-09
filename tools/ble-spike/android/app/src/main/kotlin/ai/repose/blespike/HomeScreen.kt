package ai.repose.blespike

import android.animation.ObjectAnimator
import android.animation.PropertyValuesHolder
import android.animation.ValueAnimator
import android.content.Context
import android.content.res.ColorStateList
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
import android.view.ViewGroup.LayoutParams.WRAP_CONTENT
import android.view.animation.AccelerateDecelerateInterpolator
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.Switch
import android.widget.TextView

/**
 * Screen 2 — 守护中 · 主屏. The daily screen. The "让附近的 Mac 认出我" switch IS the BLE
 * advertiser: it starts/stops [BleSpikeService] through [onAdvertiseChange], and the
 * status line reflects the real service + advertising state reported by [SpikeState].
 */
fun buildHomeScreen(
    context: Context,
    store: AppStore,
    nav: Nav,
    onAdvertiseChange: (Boolean) -> Unit,
): ScreenView {
    val pal = ReposeTheme.of(context)

    lateinit var toggle: Switch
    lateinit var toggleStatus: TextView
    var suppress = false

    val root = screenScaffold(
        context = context,
        pal = pal,
        title = "守护中",
    ) { column ->

        // ---- Hero: a key in a softly pulsing sage ring ----
        val hero = FrameLayout(context)
        val ring = View(context).apply {
            background = Ui.rounded(pal.ringSoft, context.dpF(70f))
            layoutParams = FrameLayout.LayoutParams(context.dp(140), context.dp(140), Gravity.CENTER)
        }
        val disc = TextView(context).apply {
            text = "🔑" // key
            gravity = Gravity.CENTER
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 44f)
            background = Ui.rounded(pal.accentSoft, context.dpF(52f))
            layoutParams = FrameLayout.LayoutParams(context.dp(104), context.dp(104), Gravity.CENTER)
        }
        hero.addView(ring)
        hero.addView(disc)
        startPulse(ring)
        column.addView(
            hero,
            LinearLayout.LayoutParams(MATCH_PARENT, context.dp(160)).apply {
                topMargin = context.dp(8)
            },
        )

        column.addView(
            TextView(context).apply {
                text = "正在为附近的 Mac 守护"
                gravity = Gravity.CENTER
                setTextColor(pal.textPrimary)
                setTextSize(TypedValue.COMPLEX_UNIT_SP, 19f)
                typeface = android.graphics.Typeface.create("sans-serif-medium", android.graphics.Typeface.NORMAL)
            },
            Ui.lp(top = context.dp(6)),
        )
        column.addView(
            TextView(context).apply {
                text = "它们靠近时可用回车解锁"
                gravity = Gravity.CENTER
                setTextColor(pal.textSecondary)
                setTextSize(TypedValue.COMPLEX_UNIT_SP, 14f)
            },
            Ui.lp(top = context.dp(4)),
        )

        // ---- The advertise toggle card ----
        val toggleCard = Ui.card(context, pal)
        val toggleRow = LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        toggleRow.addView(
            Ui.heading(context, pal, "让附近的 Mac 认出我"),
            LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f),
        )
        toggle = Switch(context).apply {
            thumbTintList = ColorStateList(
                arrayOf(intArrayOf(android.R.attr.state_checked), intArrayOf()),
                intArrayOf(pal.accent, pal.surfaceMuted),
            )
            trackTintList = ColorStateList(
                arrayOf(intArrayOf(android.R.attr.state_checked), intArrayOf()),
                intArrayOf(pal.accentSoft, pal.divider),
            )
            setOnCheckedChangeListener { _, isChecked ->
                if (suppress) return@setOnCheckedChangeListener
                onAdvertiseChange(isChecked)
            }
        }
        toggleRow.addView(toggle, Ui.lp(width = WRAP_CONTENT))
        toggleCard.addView(toggleRow)

        toggleStatus = Ui.secondary(context, pal, "")
        toggleCard.addView(toggleStatus, Ui.lp(top = context.dp(10)))
        column.addView(toggleCard, Ui.lp(top = context.dp(22)))

        // ---- Which Mac this phone can unlock ----
        val enabled = store.macs().filter { it.enabled }
        val macCard = Ui.card(context, pal)
        macCard.addView(Ui.secondary(context, pal, "这台手机能解锁"))
        if (enabled.isEmpty()) {
            macCard.addView(
                Ui.body(context, pal, "还没有已授权的 Mac。"),
                Ui.lp(top = context.dp(10)),
            )
        } else {
            val first = enabled.first()
            val row = LinearLayout(context).apply {
                orientation = LinearLayout.HORIZONTAL
                gravity = Gravity.CENTER_VERTICAL
            }
            row.addView(
                glyphCircle(context, pal.accentSoft, "💻", 44, 20f, pal.accent),
                Ui.lp(width = WRAP_CONTENT, right = context.dp(14)),
            )
            row.addView(
                bulletRow(context, pal, first.name, "刚刚在一起 · 今天解锁 ${store.unlocksToday} 次"),
                LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f),
            )
            macCard.addView(row, Ui.lp(top = context.dp(12)))
            if (enabled.size > 1) {
                macCard.addView(
                    Ui.secondary(context, pal, "另有 ${enabled.size - 1} 台 · 在「我的 Mac」里管理"),
                    Ui.lp(top = context.dp(10)),
                )
            }
        }
        column.addView(macCard, Ui.lp(top = context.dp(16)))

        // ---- Keep-alive entry ----
        val keepAlive = Ui.card(context, pal).apply {
            isClickable = true
            isFocusable = true
            setOnClickListener { nav.go(Screen.KEEPALIVE) }
        }
        val kaRow = LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        kaRow.addView(
            bulletRow(context, pal, "让 Repose 一直醒着", "锁屏时也要能被 Mac 认出 · 后台设置"),
            LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f),
        )
        kaRow.addView(
            TextView(context).apply {
                text = "›"
                setTextColor(pal.textSecondary)
                setTextSize(TypedValue.COMPLEX_UNIT_SP, 22f)
            },
            Ui.lp(width = WRAP_CONTENT),
        )
        keepAlive.addView(kaRow)
        column.addView(keepAlive, Ui.lp(top = context.dp(16)))

        // ---- Footnote ----
        column.addView(
            Ui.secondary(context, pal, "锁屏时的提示都发到这台手机——Mac 那时锁着，你也看不到。"),
            Ui.lp(top = context.dp(18), left = context.dp(4), right = context.dp(4)),
        )
    }

    fun refresh() {
        val running = SpikeState.serviceRunning
        val advertising = SpikeState.advertising
        suppress = true
        toggle.isChecked = running
        suppress = false
        when {
            !running -> {
                toggleStatus.text = "已关闭 · 附近的 Mac 暂时认不出这台手机"
                toggleStatus.setTextColor(pal.textSecondary)
            }
            advertising -> {
                toggleStatus.text = "● 已开启 · 正在被附近的 Mac 认出"
                toggleStatus.setTextColor(pal.accent)
            }
            else -> {
                // Running but not advertising — expected on the emulator (no BLE radio).
                toggleStatus.text = "已开启，但这台设备暂时无法广播（可能没有蓝牙硬件）——换到真机就能被认出。"
                toggleStatus.setTextColor(pal.amberText)
            }
        }
    }
    refresh()

    return ScreenView(root, onState = { refresh() })
}

/** A subtle, endless breathing pulse on the sage ring behind the key. */
private fun startPulse(ring: View) {
    val animator = ObjectAnimator.ofPropertyValuesHolder(
        ring,
        PropertyValuesHolder.ofFloat(View.SCALE_X, 1f, 1.14f),
        PropertyValuesHolder.ofFloat(View.SCALE_Y, 1f, 1.14f),
        PropertyValuesHolder.ofFloat(View.ALPHA, 0.55f, 0.12f),
    ).apply {
        duration = 1900
        repeatCount = ValueAnimator.INFINITE
        repeatMode = ValueAnimator.REVERSE
        interpolator = AccelerateDecelerateInterpolator()
    }
    ring.addOnAttachStateChangeListener(object : View.OnAttachStateChangeListener {
        override fun onViewAttachedToWindow(v: View) { animator.start() }
        override fun onViewDetachedFromWindow(v: View) { animator.cancel() }
    })
}
