package ai.repose.blespike

import android.animation.ObjectAnimator
import android.animation.PropertyValuesHolder
import android.animation.ValueAnimator
import android.content.Context
import android.content.res.ColorStateList
import android.graphics.Typeface
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
    var macStatus: TextView? = null
    var macHint: TextView? = null

    // A green key breathing inside a soft ring reads as "everything is fine". With no
    // presence key nothing is fine, so the hero goes muted and still -- an animation
    // that says calm during a broken state is the same lie as a wrong title, just
    // harder to notice. Read once here so onState can tell when it has gone stale.
    val healthy = PresenceKey.hasAny(context)
    // How many computer cards this build drew. A different number means the
    // screen is describing a set that no longer exists, and refreshing the text
    // would leave the wrong cards under it.
    val builtCount = store.pairedMacs(context).size
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
        // ---- Is this phone acting as a key ----
        //
        // One line, one switch. The hi-fi design puts nothing else at this
        // level: everything the old screen stacked here -- the fingerprint, a
        // paragraph about what the beacon is, a second card about controlling
        // the Mac -- was either a per-computer fact (now on the cards) or
        // something you read once (now on 这把钥匙).
        val toggleCard = sectionCard(context, pal, "📡", "当你的钥匙")
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

        // Shown only while it is true, which is why it is allowed on this screen
        // at all: it is a condition, not furniture. Measured on this phone --
        // the service is killed a few minutes after the app goes to the
        // background, and nothing says so.
        if (!isBatteryExempt(context)) {
            toggleCard.addView(
                Ui.amberNote(
                    context,
                    pal,
                    "这台手机会在后台把 Outsie 关掉，到时候 Mac 就认不出你了——而且不会有任何提示。",
                ),
                Ui.lp(top = context.dp(12)),
            )
            toggleCard.addView(
                Ui.ghostButton(context, pal, "去设置，别关掉它") { requestBatteryExempt(context) },
                Ui.lp(top = context.dp(10)),
            )
        }
        column.addView(toggleCard, Ui.lp(top = context.dp(14)))

        // ---- One card per computer ----
        //
        // This is the shape the design asked for and the shape the protocol
        // could not support until the Mac's state beacon started carrying which
        // Mac it is. It does now, so a card can say something about ITS machine
        // instead of the screen summarising them all into one sentence -- and
        // 「其中一台锁着」 is exactly the sentence that sends you to the wrong desk.
        column.addView(
            TextView(context).apply {
                text = "我的电脑"
                setTextColor(pal.textSecondary)
                setTextSize(TypedValue.COMPLEX_UNIT_SP, 10f)
                letterSpacing = 0.1f
                typeface = Typeface.MONOSPACE
            },
            Ui.lp(top = context.dp(20), left = context.dp(4)),
        )
        val paired = store.pairedMacs(context)
        if (paired.isEmpty()) {
            column.addView(
                Ui.infoNote(
                    context,
                    pal,
                    "还没有配过电脑。配对要两边同时在场——在 Mac 上打开 Outsie，" +
                        "左边选「手机控制」，点「配对手机」。",
                ),
                Ui.lp(top = context.dp(10)),
            )
        } else {
            for (m in paired) {
                column.addView(computerCard(context, pal, nav, m), Ui.lp(top = context.dp(10)))
            }
        }
        column.addView(
            Ui.ghostButton(context, pal, "＋ 添加电脑") { nav.go(Screen.PAIRING) },
            Ui.lp(top = context.dp(12)),
        )

        column.addView(
            Ui.secondary(context, pal, "手机钥匙用不了的时候，Mac 密码照常登录。"),
            Ui.lp(top = context.dp(18), left = context.dp(4), right = context.dp(4)),
        )
    }

    fun refresh() {
        val running = SpikeState.serviceRunning
        val advertising = SpikeState.advertising
        val authentic = PresenceKey.hasAny(context)
        val count = store.pairedMacs(context).size

        headline.text = when {
            !authentic -> "还不是钥匙"
            count == 0 -> "守着你的 Mac"
            else -> "守着你的 $count 台 Mac"
        }
        subhead.text = when {
            !authentic -> "还没配过电脑，所以还打不开任何一台。"
            count <= 1 -> "只有配对过的 Mac 认得出这台手机"
            else -> "$count 台 Mac 用的是这把钥匙"
        }

        suppress = true
        toggle.isChecked = running && advertising
        suppress = false
        toggleStatus.text = when {
            !authentic -> "还没有配对，Mac 认不出这台手机。"
            running && advertising -> "开着 · 附近的 Mac 认得出你"
            running -> "正在启动…"
            else -> "关着 · 现在谁都认不出这台手机"
        }
        toggleStatus.setTextColor(if (authentic) pal.textSecondary else pal.amberText)
    }
    refresh()

    return ScreenView(root, onState = {
        // The hero's colour and stillness are decided at build time from whether
        // a key exists, and the cards from the list of Macs. Either changing
        // means the screen is describing something that is no longer there.
        if (PresenceKey.hasAny(context) != healthy || store.pairedMacs(context).size != builtCount) {
            nav.go(Screen.HOME)
        } else {
            refresh()
        }
    })
}

/**
 * One computer: what it is, what it is doing, and the two things you can ask of
 * it.
 *
 * FOUR STATES, AND 「不在附近」 IS THE HEADLINE ONE
 *
 * Out of range is overwhelmingly the common reason for silence, so it is the
 * main line; asleep, off and 「没开 Outsie」 go underneath. Not 「未知」 -- that
 * reads as a fault, and this is the ordinary case.
 *
 * 「走过去按回车就能进」 appears only when the Mac has said, signed with the
 * paired key, that it is locked. Without the signature anyone with a radio
 * could broadcast 「开着」 and keep you in your chair.
 */
private fun computerCard(context: Context, pal: Palette, nav: Nav, m: PairedMac): LinearLayout {
    val seen = m.macId
        ?.let { hex -> hex.toIntOrNull(16) }
        ?.let { id -> MacState.sightings().firstOrNull { it.macId == id } }

    val card = Ui.card(context, pal)
    val top = LinearLayout(context).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
    }
    top.addView(
        TextView(context).apply {
            text = when (seen?.state) {
                MacLockState.LOCKED -> "🔒"
                MacLockState.UNLOCKED -> "💻"
                else -> "🌫"
            }
            gravity = Gravity.CENTER
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 17f)
            background = Ui.rounded(pal.surfaceMuted, context.dpF(11f))
            layoutParams = LinearLayout.LayoutParams(context.dp(34), context.dp(34))
        },
    )
    top.addView(
        LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            addView(
                TextView(context).apply {
                    text = m.name
                    setTextColor(pal.textPrimary)
                    setTextSize(TypedValue.COMPLEX_UNIT_SP, 14f)
                    typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
                },
            )
            addView(
                Ui.secondary(
                    context,
                    pal,
                    when (seen?.state) {
                        MacLockState.LOCKED -> "锁着 · 走过去，密码框留空按回车就能进"
                        MacLockState.UNLOCKED -> "开着 · 没锁，不用解锁"
                        else -> "不在附近 · 也可能是它睡着了、关机了，或者没开 Outsie"
                    },
                ),
                Ui.lp(top = context.dp(2)),
            )
        },
        LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f).also { it.leftMargin = context.dp(11) },
    )
    card.addView(top)

    val actions = LinearLayout(context).apply { orientation = LinearLayout.HORIZONTAL }
    actions.addView(
        Ui.ghostButton(context, pal, "锁定") { sendCommand(context, SpikeContract.CMD_LOCK) },
        LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f),
    )
    actions.addView(
        Ui.ghostButton(context, pal, "控制") { nav.go(Screen.CONTROL) },
        LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f).also { it.leftMargin = context.dp(10) },
    )
    card.addView(actions, Ui.lp(top = context.dp(12)))
    return card
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
        cmd == SpikeContract.CMD_LOCK -> "已发出。Mac 在附近的话，几秒内会锁屏。"
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

/**
 * A short, stable label for a Mac the phone has only ever heard from.
 *
 * Four hex digits, because that is genuinely all the beacon carries. Pairing
 * exchanges a human name, but it does not yet bind that name to this id -- so
 * showing one here would mean guessing which Mac the name belonged to, and
 * guessing wrong is worse than four hex digits the reader can match against the
 * same four on the Mac's own screen.
 */
fun macLabel(macId: Int): String =
    if (macId == SpikeContract.MAC_ID_UNKNOWN) "一台没报编号的 Mac"
    else "Mac %04X".format(macId)

/**
 * Whether Android will leave this app running in the background.
 *
 * Read, never assumed: the answer is the user's to give, and an app that
 * pretended otherwise would show a row that cannot be dismissed.
 */
fun isBatteryExempt(context: Context): Boolean =
    context.getSystemService(android.os.PowerManager::class.java)
        ?.isIgnoringBatteryOptimizations(context.packageName) == true

/**
 * Open the system's own dialog.
 *
 * From a button the person pressed, never on launch. An app that asks to be
 * exempt from battery optimisation the moment it starts reads as malware, and
 * the person has no context yet for deciding. Let them meet the problem first.
 */
@android.annotation.SuppressLint("BatteryLife")
fun requestBatteryExempt(context: Context) {
    runCatching {
        context.startActivity(
            android.content.Intent(
                android.provider.Settings.ACTION_REQUEST_IGNORE_BATTERY_OPTIMIZATIONS,
                android.net.Uri.parse("package:${context.packageName}"),
            ),
        )
    }.onFailure {
        // Some builds hide this action entirely. The general battery page is
        // still better than a button that does nothing.
        runCatching {
            context.startActivity(
                android.content.Intent(android.provider.Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS),
            )
        }
    }
}
