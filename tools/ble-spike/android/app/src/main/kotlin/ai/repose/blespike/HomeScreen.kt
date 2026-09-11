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
import android.widget.Toast

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
        title = "",
        showTitle = false,
    ) { column ->

        // ---- Hero: a key in a softly pulsing sage ring ----
        //
        // The pulse is state, not decoration. With no presence key nothing is
        // fine, so the hero goes muted and still -- an animation that says calm
        // during a broken state is the same lie as a wrong title, just harder
        // to notice.
        val hero = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            background = Ui.rounded(
                if (healthy) pal.accentSoft else pal.surfaceMuted,
                context.dpF(24f),
            )
            setPadding(context.dp(22), context.dp(22), context.dp(22), context.dp(24))
        }
        val stack = FrameLayout(context)
        val ring = View(context).apply {
            background = Ui.rounded(if (healthy) pal.ringSoft else pal.divider, context.dpF(60f))
            layoutParams = FrameLayout.LayoutParams(context.dp(120), context.dp(120), Gravity.CENTER)
        }
        val disc = TextView(context).apply {
            text = "🔑"
            gravity = Gravity.CENTER
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 36f)
            background = Ui.rounded(pal.surface, context.dpF(44f))
            alpha = if (healthy) 1f else 0.6f
            layoutParams = FrameLayout.LayoutParams(context.dp(88), context.dp(88), Gravity.CENTER)
        }
        stack.addView(ring)
        stack.addView(disc)
        if (healthy) startPulse(ring)
        hero.addView(
            stack,
            LinearLayout.LayoutParams(MATCH_PARENT, context.dp(126)),
        )

        headline = TextView(context).apply {
            gravity = Gravity.CENTER
            setTextColor(pal.textPrimary)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 22f)
            typeface = android.graphics.Typeface.create("sans-serif", android.graphics.Typeface.BOLD)
        }
        hero.addView(headline, Ui.lp(top = context.dp(12)))
        subhead = TextView(context).apply {
            gravity = Gravity.CENTER
            setTextColor(pal.textSecondary)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 13.5f)
            setLineSpacing(context.dpF(4f), 1f)
        }
        hero.addView(subhead, Ui.lp(top = context.dp(8)))
        column.addView(hero, Ui.lp(top = context.dp(6)))

        // ---- The advertise toggle card ----
        val toggleCard = sectionCard(context, pal, "📡", "让附近的 Mac 认出我")
        val toggleRow = LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        toggleStatus = Ui.secondary(context, pal, "")
        toggleRow.addView(toggleStatus, LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f))
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
        toggleRow.addView(toggle, Ui.lp(width = WRAP_CONTENT, left = context.dp(12)))
        toggleCard.addView(toggleRow, Ui.lp(top = context.dp(12)))
        column.addView(toggleCard, Ui.lp(top = context.dp(14)))

        // ---- What this phone can actually see ----
        //
        // Not a device list: there is no pairing store, and the previous
        // version's list was seeded placeholders shown as though they were real.
        val keyCard = sectionCard(context, pal, "🔑", "配对状态")
        keyStatus = Ui.body(context, pal, "")
        keyCard.addView(keyStatus, Ui.lp(top = context.dp(12)))
        keyCard.addView(
            Ui.secondary(
                context,
                pal,
                "这台手机只往外发信号、收不到回音，所以看不出哪台 Mac 认出了你。" +
                    "要看那一边，去 Mac 上的 ${Brand.NAME}。",
            ),
            Ui.lp(top = context.dp(8)),
        )
        column.addView(keyCard, Ui.lp(top = context.dp(14)))

        // ---- Controlling the Mac from here ----
        //
        // Only shown once there is a key. Without one the Mac refuses every
        // command, and a button that is guaranteed to do nothing is worse than
        // no button: it teaches people the feature is flaky rather than that
        // they have not finished setting it up.
        if (healthy) {
            val control = sectionCard(context, pal, "🖥", "从这里控制 Mac")
            control.addView(
                Ui.secondary(
                    context,
                    pal,
                    "指令跟着信标一起发出，所以只有 Mac 听得到这台手机时才有用 —— " +
                        "人不在电脑旁边，按了也不会生效。",
                ),
                Ui.lp(top = context.dp(12)),
            )
            control.addView(
                Ui.primaryButton(context, pal, "锁定 Mac") {
                    sendCommand(context, SpikeContract.CMD_LOCK)
                },
                Ui.lp(top = context.dp(16)),
            )
            // 「允许下一次解锁」 used to sit here and has been removed.
            //
            // It sent a command that made the Mac write an unlock permit. The
            // trouble is that a phone which is near and switched on makes the
            // Mac write that permit CONTINUOUSLY -- so the button described
            // something already happening, and pressing it changed nothing you
            // could see. A control with no observable effect teaches people
            // that the app's buttons are decorative.
            //
            // It earns a place only alongside a stricter mode, where being near
            // stops being enough and the tap becomes the only way in. That is a
            // security change, not a convenience, so it ships with that mode or
            // not at all. See docs/plans/2026-09-11-phone-commands.md.
            control.addView(
                Ui.secondary(
                    context,
                    pal,
                    "手机收不到 Mac 的回音，所以这里不会显示「已锁定」。要确认，看 Mac。",
                ),
                Ui.lp(top = context.dp(10)),
            )
            column.addView(control, Ui.lp(top = context.dp(14)))
        }

        // ---- Keep-alive entry ----
        val keepAlive = sectionCard(context, pal, "🔋", "让 ${Brand.NAME} 一直醒着").apply {
            isClickable = true
            isFocusable = true
            setOnClickListener { nav.go(Screen.KEEPALIVE) }
        }
        val kaRow = LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        kaRow.addView(
            Ui.secondary(context, pal, "锁屏时也要能被 Mac 认出 · 后台设置"),
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
        keepAlive.addView(kaRow, Ui.lp(top = context.dp(12)))
        column.addView(keepAlive, Ui.lp(top = context.dp(14)))

        column.addView(
            Ui.secondary(context, pal, "锁屏时的提示都发到这台手机——Mac 那时锁着，你也看不到。"),
            Ui.lp(top = context.dp(16), left = context.dp(4), right = context.dp(4)),
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
                headline.text = "守着你的 Mac"
                subhead.text = "只有和你配对过的 Mac 认得出这台手机"
            }
            advertising -> {
                headline.text = "还没有配对"
                subhead.text = "Mac 还认不出这台手机，先配对一次"
            }
            else -> {
                headline.text = "开着，但蓝牙没能用起来"
                subhead.text = "这台设备可能没有蓝牙"
            }
        }

        when {
            !running -> {
                toggleStatus.text = "已关闭 · Mac 认不出这台手机"
                toggleStatus.setTextColor(pal.textSecondary)
            }
            advertising -> {
                // Deliberately not "正在被认出": nothing tells this phone that.
                toggleStatus.text = "● 已开启 · 正在让附近的 Mac 认出你"
                toggleStatus.setTextColor(if (authentic) pal.accent else pal.amberText)
            }
            else -> {
                toggleStatus.text = "已开启，但这台设备用不了蓝牙。"
                toggleStatus.setTextColor(pal.amberText)
            }
        }

        keyStatus.text = if (authentic) {
            "已配对 · 编号 ${PresenceKey.fingerprint(context) ?: "?"}"
        } else {
            "还没有配对。Mac 会看见这台手机，但认不出它是你的，所以不会解锁。"
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

/**
 * Queue a command for the Mac and say what happened — as far as this phone can
 * know, which is not far.
 *
 * The beacon is one-way. Nothing comes back, so the toast can only report that
 * the command went out, never that the Mac did it. Drawing a tick here would be
 * inventing the half of the story this phone cannot see.
 */
private fun sendCommand(context: Context, cmd: Int) {
    val queued = BleSpikeService.postCommand(context, cmd)
    val message = when {
        !queued -> "还没有配对，Mac 不会接受这条指令。"
        !SpikeState.serviceRunning -> "手机钥匙是关着的，先打开上面的开关。"
        cmd == SpikeContract.CMD_LOCK -> "已发出。Mac 听得到这台手机的话，几秒内会锁屏。"
        else -> "已发出。回到 Mac 前按回车即可。"
    }
    Toast.makeText(context, message, Toast.LENGTH_LONG).show()
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
