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
 * Screen 2 — 主屏. The daily screen. The switch IS the BLE advertiser: it starts and
 * stops [BleSpikeService] through [onAdvertiseChange].
 *
 * WHAT THIS SCREEN MAY CLAIM
 * --------------------------
 * The beacon is non-connectable and nothing ever answers it, so this phone has no
 * back-channel and cannot know whether any Mac heard it. It knows three things: the
 * service is running, the stack accepted the advertisement, and whether a presence
 * key exists. Everything else on this screen used to be invented --
 *
 *   "正在被附近的 Mac 认出"          the phone cannot see the other end at all
 *   "MacBook Pro（工作）"            a seeded placeholder, no pairing store exists
 *   "刚刚在一起 · 今天解锁 4 次"      a hardcoded 4
 *
 * -- so a phone with no key, broadcasting a tag no Mac would ever accept, reported
 * that it was being recognised and had unlocked something four times today. The
 * screen now says only what the phone can observe, and says plainly where the other
 * half of the answer lives.
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
    lateinit var keyStatus: TextView

    // A green key breathing inside a soft ring reads as "everything is fine". With no
    // presence key nothing is fine, so the hero goes muted and still -- an animation
    // that says calm during a broken state is the same lie as a wrong title, just
    // harder to notice. Read once here so onState can tell when it has gone stale.
    val healthy = PresenceKey.has(SpikeContract.PRESENCE_KEY_ID)
    lateinit var headline: TextView
    lateinit var subhead: TextView
    var suppress = false

    val root = screenScaffold(
        context = context,
        pal = pal,
        // Not a state word. It used to say 守护中 -- in the largest type on the
        // screen, directly above "任何 Mac 都会拒绝" on a phone with no key. The
        // headline below carries the state; the title just names the thing.
        title = "手机钥匙",
    ) { column ->

        // ---- Hero: a key in a softly pulsing sage ring ----
        val hero = FrameLayout(context)
        val ring = View(context).apply {
            background = Ui.rounded(if (healthy) pal.ringSoft else pal.surfaceMuted, context.dpF(70f))
            layoutParams = FrameLayout.LayoutParams(context.dp(140), context.dp(140), Gravity.CENTER)
        }
        val disc = TextView(context).apply {
            text = "🔑" // key
            gravity = Gravity.CENTER
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 44f)
            background = Ui.rounded(if (healthy) pal.accentSoft else pal.surfaceMuted, context.dpF(52f))
            alpha = if (healthy) 1f else 0.55f
            layoutParams = FrameLayout.LayoutParams(context.dp(104), context.dp(104), Gravity.CENTER)
        }
        hero.addView(ring)
        hero.addView(disc)
        if (healthy) startPulse(ring)
        column.addView(
            hero,
            LinearLayout.LayoutParams(MATCH_PARENT, context.dp(160)).apply {
                topMargin = context.dp(8)
            },
        )

        headline = TextView(context).apply {
            gravity = Gravity.CENTER
            setTextColor(pal.textPrimary)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 19f)
            typeface = android.graphics.Typeface.create("sans-serif-medium", android.graphics.Typeface.NORMAL)
        }
        column.addView(headline, Ui.lp(top = context.dp(6)))
        subhead = TextView(context).apply {
            gravity = Gravity.CENTER
            setTextColor(pal.textSecondary)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 14f)
        }
        column.addView(subhead, Ui.lp(top = context.dp(4)))

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

        // ---- What this phone can actually see ----
        //
        // Not a device list: there is no pairing store, and the previous version's
        // list was seeded placeholders shown as though they were real.
        val keyCard = Ui.card(context, pal)
        keyCard.addView(Ui.secondary(context, pal, "这台手机的信标"))
        keyStatus = Ui.body(context, pal, "")
        keyCard.addView(keyStatus, Ui.lp(top = context.dp(10)))
        keyCard.addView(
            Ui.secondary(context, pal, "信标是单向的，没有 Mac 会回话，所以这里看不到哪台 Mac 收到了。要看那一侧，去 Mac 上的 Repose。"),
            Ui.lp(top = context.dp(10)),
        )
        column.addView(keyCard, Ui.lp(top = context.dp(16)))

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
        // Ask the Keystore, not the service.
        //
        // SpikeState.authentic is only set when BleSpikeService starts, so with
        // the beacon switched off this screen announced 没有配对密钥 on a phone
        // that had one -- telling someone their pairing is gone because a toggle
        // is off. "Is there a key" and "is the beacon running" are different
        // questions and the screen now asks each of them separately.
        val authentic = PresenceKey.has(SpikeContract.PRESENCE_KEY_ID)
        suppress = true
        toggle.isChecked = running
        suppress = false

        // Two independent facts, and the headline must not collapse them. A phone
        // broadcasting a worthless tag is on the air and is still useless, so
        // "advertising" alone is not "working".
        when {
            !running -> {
                headline.text = "已关闭"
                subhead.text = "附近的 Mac 认不出这台手机"
            }
            advertising && authentic -> {
                headline.text = "正在广播"
                subhead.text = "配好同一把密钥的 Mac 才认得出这串信号"
            }
            advertising -> {
                headline.text = "正在广播，但没有密钥"
                subhead.text = "标签是随机凑的，任何 Mac 都会拒绝"
            }
            else -> {
                headline.text = "已开启，但没能上天线"
                subhead.text = "这台设备可能没有蓝牙硬件"
            }
        }

        when {
            !running -> {
                toggleStatus.text = "已关闭 · 没有在广播"
                toggleStatus.setTextColor(pal.textSecondary)
            }
            advertising -> {
                // Deliberately not "正在被认出": nothing tells this phone that.
                toggleStatus.text = "● 已开启 · 正在广播，每 ${SpikeContract.WINDOW_SECONDS} 秒换一次标签"
                toggleStatus.setTextColor(if (authentic) pal.accent else pal.amberText)
            }
            else -> {
                toggleStatus.text = "已开启，但这台设备暂时无法广播（可能没有蓝牙硬件）——换到真机就能广播。"
                toggleStatus.setTextColor(pal.amberText)
            }
        }

        keyStatus.text = if (authentic) {
            "密钥指纹 ${PresenceKey.fingerprint(context) ?: "?"} · 已发出 ${SpikeState.beaconsSent} 次"
        } else {
            "没有配对密钥。信标照常发，但标签是随机凑出来的，Mac 会看见这台手机然后拒绝它。"
        }
        keyStatus.setTextColor(if (authentic) pal.textPrimary else pal.amberText)
    }
    refresh()

    return ScreenView(root, onState = {
        // The hero's colour and stillness are decided at build time from whether a
        // key exists. If that changes underneath us the picture would keep saying
        // the old thing, so rebuild; otherwise just refresh the live text.
        if (PresenceKey.has(SpikeContract.PRESENCE_KEY_ID) != healthy) nav.go(Screen.HOME) else refresh()
    })
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
