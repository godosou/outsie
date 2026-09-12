package ai.repose.blespike

import android.animation.ObjectAnimator
import android.animation.PropertyValuesHolder
import android.animation.ValueAnimator
import android.app.AlertDialog
import android.content.Context
import android.content.Intent
import android.content.res.ColorStateList
import android.graphics.Typeface
import android.os.SystemClock
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
 * 主屏 — the only destination (design doc §03 §04).
 *
 * There is no tab bar. 控制 belongs to one computer and opens from its card;
 * what used to be the 「这把钥匙」 tab — what this key is, how to get rid of it,
 * what if the phone is lost — is read once, and folds under 「更多」 at the
 * bottom. A one-item tab bar is furniture, not navigation.
 *
 * WHAT THIS SCREEN MAY CLAIM
 *
 * The beacon is non-connectable and nothing answers it, so this phone knows
 * only what the service records in [SpikeState.radio] -- alive, has a key,
 * Bluetooth on, the stack accepted the advertisement, what last went wrong --
 * and the one word [RadioFacts.phase] derives from that. What each Mac is
 * doing comes from the Mac's own signed state beacon, per card. Everything
 * else it would be inventing.
 *
 * The screen prints from the facts, never from a boolean it cached: on the
 * device the old two-boolean version showed 「正在启动…」 for good after
 * Bluetooth was switched off under a running service, with a switch that
 * looked dead.
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
    lateinit var headline: TextView
    lateinit var subhead: TextView
    var suppress = false

    // A green key breathing inside a soft ring reads as "everything is fine".
    // With no key nothing is fine, so the hero goes muted and still. Read once
    // here so onState can tell when it has gone stale.
    val healthy = PresenceKey.hasAny(context)
    val builtCount = store.pairedMacs(context).size
    // Which Macs were heard when the cards were drawn. The buttons on a card
    // are enabled by this, so a Mac going silent -- or the key being switched
    // off, which forgets every Mac -- has to redraw the cards, not just the
    // headline. Otherwise 锁定/控制 stay live for a machine nobody hears.
    val builtHeard = MacState.sightings().map { it.macId }.toSet()
    // What a card says moves without the heard set moving (LOCKED → UNLOCKED,
    // the phone's radio coming up, ten minutes of silence passing). Each card
    // leaves a redraw here; refresh() runs them.
    val cardRefreshers = mutableListOf<() -> Unit>()

    val root = screenScaffold(context, pal, title = "", showTitle = false) { column ->

        // ---- Hero ----
        val hero = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            background = Ui.rounded(if (healthy) pal.accentSoft else pal.surfaceMuted, context.dpF(24f))
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
        hero.addView(stack, LinearLayout.LayoutParams(MATCH_PARENT, context.dp(126)))
        headline = TextView(context).apply {
            gravity = Gravity.CENTER
            setTextColor(pal.textPrimary)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 22f)
            typeface = Typeface.create("sans-serif", Typeface.BOLD)
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

        // ---- Is this phone acting as a key: one line, one switch ----
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
                // The switch reads off in NO_KEY with the service alive under
                // it, and starting a running service is a no-op -- so on the
                // device this tap did nothing and the switch looked dead. Ask
                // the service to look again instead: it re-reads the keys and
                // the adapter, and starts the radio if it now can.
                if (isChecked && SpikeState.radio.running) {
                    runCatching {
                        context.startForegroundService(
                            Intent(context, BleSpikeService::class.java).setAction(BleSpikeService.ACTION_REBUILD_BEACON),
                        )
                    }
                }
            }
        }
        toggleRow.addView(toggle, Ui.lp(width = WRAP_CONTENT, left = context.dp(12)))
        toggleCard.addView(toggleRow, Ui.lp(top = context.dp(12)))

        // Shown only while it is true: a condition, not furniture. Measured on
        // this phone -- the service is killed a few minutes after the app goes
        // to the background, and nothing says so.
        if (!isBatteryExempt(context)) {
            toggleCard.addView(
                Ui.amberNote(context, pal, "这部手机会在后台把 Outsie 关掉。到时候 Mac 认不出你，也不会有提示。"),
                Ui.lp(top = context.dp(12)),
            )
            toggleCard.addView(
                Ui.ghostButton(context, pal, "去设置，让它留在后台") { requestBatteryExempt(context) },
                Ui.lp(top = context.dp(10)),
            )
        }
        column.addView(toggleCard, Ui.lp(top = context.dp(14)))

        // ---- One card per computer ----
        column.addView(sectionLabel(context, pal, "我的电脑"), Ui.lp(top = context.dp(20), left = context.dp(4)))
        val paired = store.pairedMacs(context)
        if (paired.isEmpty()) {
            column.addView(
                Ui.infoNote(context, pal, "还没配过电脑。在 Mac 上打开 Outsie，左边选「手机控制」，点「配一部新手机」。"),
                Ui.lp(top = context.dp(10)),
            )
        } else {
            for (m in paired) {
                column.addView(computerCard(context, pal, nav, store, m, cardRefreshers), Ui.lp(top = context.dp(10)))
            }
        }
        column.addView(
            Ui.ghostButton(context, pal, "＋ 添加电脑") { nav.go(Screen.PAIRING) },
            Ui.lp(top = context.dp(12)),
        )

        // ---- 更多: what you read once ----
        //
        // This used to be a tab of its own, carrying a second list of the same
        // computers. What is left -- what this key is, what to do if the phone
        // is lost, how to undo everything -- folds here, under the computers it
        // talks about. Technical detail (the fingerprint) lives here too: the
        // surface stays in plain words (design doc §01 rule 4).
        val more = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            visibility = View.GONE
        }
        val moreToggle = TextView(context).apply {
            text = "▸ 更多"
            setTextColor(pal.textSecondary)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 13f)
            setPadding(context.dp(4), context.dp(16), context.dp(4), context.dp(4))
            isClickable = true
            setOnClickListener {
                val open = more.visibility == View.VISIBLE
                more.visibility = if (open) View.GONE else View.VISIBLE
                text = if (open) "▸ 更多" else "▾ 更多"
            }
        }
        column.addView(moreToggle, Ui.lp(width = WRAP_CONTENT))
        if (healthy) {
            more.addView(
                Ui.infoNote(context, pal, "每台 Mac 一把钥匙，都存在这部手机里，导不出去。要单独去掉一台，用卡片上的「移走」。"),
                Ui.lp(top = context.dp(6)),
            )
            more.addView(
                Ui.amberNote(context, pal, "手机丢了：在每一台 Mac 上关掉它的开关。人不在手机旁边时，只能这样。"),
                Ui.lp(top = context.dp(10)),
            )
            more.addView(techDetails(context, pal, PresenceKey.fingerprint(context)), Ui.lp(top = context.dp(6)))
            more.addView(
                Ui.ghostButton(context, pal, "和所有 Mac 解除配对") {
                    AlertDialog.Builder(context, Ui.dialogTheme(context))
                        .setTitle("和所有 Mac 解除配对？")
                        .setMessage("所有 Mac 马上认不出这部手机。想再用，重新配对。")
                        .setPositiveButton("解除") { _, _ ->
                            // Every slot, not just the first: with two Macs paired,
                            // deleting one key would leave the phone still opening
                            // the other while the screen says the pairing is gone.
                            for (id in PresenceKey.activeIds(context)) {
                                PresenceKey.delete(context, id)
                                ConsoleCatalogue.forget(context, id)
                                ConsoleArrangement.forget(context, id)
                            }
                            AppStore(context).keyIds = emptyList()
                            // Nothing left to advertise: the beacon stops.
                            BleSpikeService.keysChanged(context)
                            Toast.makeText(context, "解除了。", Toast.LENGTH_LONG).show()
                            nav.go(Screen.HOME)
                        }
                        .setNegativeButton("算了", null)
                        .show()
                },
                Ui.lp(top = context.dp(12)),
            )
        } else {
            more.addView(Ui.secondary(context, pal, "还没有钥匙。配一台电脑，这里才有东西。"), Ui.lp(top = context.dp(6)))
        }
        column.addView(more)
    }

    // Re-rendered on the clock as well as on events: STARTING turns into
    // FAILED by time passing (RadioFacts.STARTING_GRACE_MS), and nothing
    // notifies when it does.
    lateinit var tick: Runnable

    fun refresh() {
        val now = SystemClock.elapsedRealtime()
        val radio = SpikeState.radio
        val phase = radio.phase(now)
        val authentic = PresenceKey.hasAny(context)
        val count = store.pairedMacs(context).size

        headline.text = when {
            !authentic -> "还不是钥匙"
            count == 0 -> "守着你的 Mac"
            else -> "守着你的 $count 台 Mac"
        }
        subhead.text = when {
            !authentic -> "还没配过电脑。配一台，它就是钥匙了。"
            else -> "带着手机走，它们自己锁；走回去，按一下回车。"
        }

        // The switch shows the person's intent (phase.switchOn), never the
        // radio's luck, and it is always live: an off switch that cannot be
        // pressed is a phone that cannot be made a key of. The sentence is
        // the phase's own, so there is no state the service can be in that
        // this row has no words for -- 「正在启动…」 for good was exactly that.
        suppress = true
        toggle.isChecked = phase.switchOn
        suppress = false
        toggle.isEnabled = true
        toggleStatus.text = phase.sentence(radio.failure)
        toggleStatus.setTextColor(
            when (phase) {
                RadioPhase.ON, RadioPhase.OFF, RadioPhase.STARTING -> pal.textSecondary
                RadioPhase.NO_KEY, RadioPhase.BLUETOOTH_OFF, RadioPhase.FAILED -> pal.amberText
            },
        )

        toggleStatus.removeCallbacks(tick)
        val since = radio.startingSince
        if (phase == RadioPhase.STARTING && since != null) {
            // Just past the grace: the same read then yields FAILED.
            val due = since + RadioFacts.STARTING_GRACE_MS + 100 - now
            toggleStatus.postDelayed(tick, due.coerceAtLeast(250L))
        }

        cardRefreshers.forEach { it() }
    }
    tick = Runnable { refresh() }
    refresh()

    return ScreenView(root, onState = {
        // The hero's colour and the cards are decided at build time. Either
        // changing means the screen is describing something no longer there.
        val heardNow = MacState.sightings().map { it.macId }.toSet()
        if (PresenceKey.hasAny(context) != healthy || store.pairedMacs(context).size != builtCount || heardNow != builtHeard) {
            nav.go(Screen.HOME)
        } else {
            refresh()
        }
    })
}

private fun sectionLabel(context: Context, pal: Palette, text: String) = TextView(context).apply {
    this.text = text
    setTextColor(pal.textSecondary)
    setTextSize(TypedValue.COMPLEX_UNIT_SP, 10f)
    letterSpacing = 0.1f
    typeface = Typeface.MONOSPACE
}

/**
 * One computer: what it is doing, and the two things you can ask of it.
 *
 * THREE STATES, AND 「没听到它」 IS THE ORDINARY ONE
 *
 * Out of range is overwhelmingly the common reason for silence, so it is the
 * main clause; asleep, off and 「没开 Outsie」 follow. Not 「未知」 -- that reads
 * as a fault, and this is the ordinary case.
 *
 * 「走过去，密码框留空，按回车」 appears only when the Mac has said, signed with
 * the paired key, that it is locked. Without the signature anyone with a radio
 * could broadcast 「开着」 and keep you in your chair.
 *
 * SILENCE HAS THREE SPELLINGS
 *
 * A Mac this phone cannot hear is not one fact but three, and the person acts
 * differently on each. If this phone's own radio is not on, nothing could be
 * heard and the fault is here, not there: the card says so, or it would send
 * someone across the room to check a Mac that is fine. If the Mac was heard in
 * the last ten minutes, the likeliest story is that the person walked away and
 * the Mac locked, as it should. Only after that is it the ordinary silence.
 */
private fun computerCard(
    context: Context,
    pal: Palette,
    nav: Nav,
    store: AppStore,
    m: PairedMac,
    refreshers: MutableList<() -> Unit>,
): LinearLayout {
    // By the slot first: it is what the beacon verified under, and a slot paired
    // by an older build has no stored id to match on. The id is the fallback for
    // a sighting recorded without a slot.
    val macId = m.macId?.toIntOrNull(16)
    fun sightingNow(): MacSighting? =
        MacState.sightingFor(m.keyId)
            ?: macId?.let { id -> MacState.sightings().firstOrNull { it.macId == id } }
    fun lastHeard(): Long? =
        MacState.lastHeardFor(m.keyId) ?: macId?.let { MacState.lastHeardAt(it) }
    val seen = sightingNow()
    val heard = seen != null

    val card = Ui.card(context, pal)
    val top = LinearLayout(context).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
    }
    val glyph = TextView(context).apply {
        gravity = Gravity.CENTER
        setTextColor(pal.textSecondary)
        setTextSize(TypedValue.COMPLEX_UNIT_SP, 17f)
        background = Ui.rounded(pal.surfaceMuted, context.dpF(11f))
        layoutParams = LinearLayout.LayoutParams(context.dp(34), context.dp(34))
    }
    top.addView(glyph)
    val subtitle = Ui.secondary(context, pal, "")
    top.addView(
        LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            addView(TextView(context).apply {
                text = m.name
                setTextColor(pal.textPrimary)
                setTextSize(TypedValue.COMPLEX_UNIT_SP, 14f)
                typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
            })
            addView(subtitle, Ui.lp(top = context.dp(2)))
        },
        LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f).also { it.leftMargin = context.dp(11) },
    )
    card.addView(top)

    // What the Mac said, and whether this phone could hear it, both move
    // without the set of heard Macs moving. Those redraw in place; a Mac
    // appearing or vanishing rebuilds the screen, because the buttons below
    // are gated on it.
    fun redraw() {
        val now = SystemClock.elapsedRealtime()
        val s = sightingNow()
        glyph.text = when (s?.state) {
            MacLockState.LOCKED -> "🔒"
            MacLockState.UNLOCKED -> "💻"
            // 🌫 drew as an empty box on this phone, which reads as a broken
            // image rather than as "not heard from".
            else -> "–"
        }
        subtitle.text = macCardSentence(s, lastHeard(), now, SpikeState.radio.phase(now).onAir)
    }
    redraw()
    refreshers += { redraw() }

    // Not pressable while the Mac cannot be heard (design doc §04). A lock
    // command nobody is listening for and a control screen for a machine that
    // is not there are both buttons that always fail; the subtitle already says
    // why, so they dim rather than explain themselves twice.
    fun gated(label: String, onClick: () -> Unit): TextView =
        Ui.ghostButton(context, pal, label, onClick).apply {
            isEnabled = heard
            alpha = if (heard) 1f else 0.45f
        }
    val actions = LinearLayout(context).apply { orientation = LinearLayout.HORIZONTAL }
    actions.addView(
        gated("锁定") { sendLock(context) },
        LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f),
    )
    actions.addView(
        // The control screen belongs to THIS computer (design doc §04).
        gated("控制") { controlMac(m.keyId); nav.go(Screen.CONTROL) },
        LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f).also { it.leftMargin = context.dp(10) },
    )
    card.addView(actions, Ui.lp(top = context.dp(12)))

    // Two quiet links under the buttons. Small type because they are rare, not
    // because they are unimportant.
    val links = LinearLayout(context).apply { orientation = LinearLayout.HORIZONTAL }
    links.addView(
        TextView(context).apply {
            // Calibration lives on the phone (design doc §05 §06): you walk with
            // it and cannot see the Mac. Gated like the buttons -- the Mac has to
            // be listening for the command to reach it.
            text = "重新量距离"
            setTextColor(if (heard) pal.textSecondary else pal.divider)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 12f)
            setPadding(0, context.dp(10), context.dp(18), 0)
            isClickable = heard
            if (heard) setOnClickListener { calMac(m.keyId); nav.go(Screen.CAL) }
        },
        Ui.lp(width = WRAP_CONTENT),
    )
    links.addView(
        TextView(context).apply {
            text = "移走"
            setTextColor(pal.textSecondary)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 12f)
            setPadding(0, context.dp(10), 0, 0)
            isClickable = true
            setOnClickListener {
                AlertDialog.Builder(context, Ui.dialogTheme(context))
                    .setTitle("移走「${m.name}」？")
                    .setMessage(
                        "这部手机丢掉它那把钥匙，那台 Mac 就不会再因为你走近而解锁。\n\n" +
                            "那台 Mac 上的设置不会变，它只是认不出这部手机了。想恢复，重新配对。",
                    )
                    .setNegativeButton("算了", null)
                    .setPositiveButton("移走") { _, _ ->
                        store.forgetMac(context, m.keyId)
                        // Its buttons and this phone's arrangement of them go too.
                        ConsoleCatalogue.forget(context, m.keyId)
                        ConsoleArrangement.forget(context, m.keyId)
                        // And the beacon stops carrying the key that is gone.
                        BleSpikeService.keysChanged(context)
                        Toast.makeText(context, "移走了「${m.name}」。", Toast.LENGTH_LONG).show()
                        nav.go(Screen.HOME)
                    }
                    .show()
            }
        },
        Ui.lp(width = WRAP_CONTENT),
    )
    card.addView(links)
    return card
}

/**
 * The line under a computer's name. Pure, so the three kinds of silence can be
 * tested without a device (MacStateTest).
 *
 * Heard now: what the Mac itself said, whatever this phone's radio is doing --
 * a Mac heard while the beacon is still starting is still a Mac heard. Not
 * heard: whose fault, in this order. This phone not broadcasting; the Mac
 * heard within [MacState.RECENTLY_HEARD_MS]; the ordinary silence.
 */
internal fun macCardSentence(seen: MacSighting?, lastHeard: Long?, now: Long, phoneOnAir: Boolean): String = when {
    seen?.state == MacLockState.LOCKED -> "锁着。走过去，密码框留空，按回车。"
    seen?.state == MacLockState.UNLOCKED -> "开着，不用解锁。"
    seen != null -> when (seen.beacon) {
        MacBeaconState.CAL_NEAR, MacBeaconState.CAL_WAIT, MacBeaconState.CAL_FAR ->
            "正在量距离。看手机上量距离那一屏，一两分钟就好。"
        else -> "刚量完距离，马上就说锁没锁。"
    }
    !phoneOnAir -> "这部手机的钥匙没在广播，所以听不到它。"
    lastHeard != null && now - lastHeard <= MacState.RECENTLY_HEARD_MS ->
        "刚才还听得到，现在听不到了。走远了的话，它已经自己锁上。"
    else -> "没听到它。可能不在附近、睡着了，或者没开 Outsie。"
}

/**
 * Queue the lock command and say what this phone can honestly know.
 *
 * The beacon is one-way. Nothing comes back, so the toast can only report that
 * the command went out, never that the Mac did it.
 */
private fun sendLock(context: Context) {
    // Ask first, so the toast names the real reason and nothing is queued
    // for a key that is off (design doc §11: 钥匙关着 / 没配对 → 当场说，不入队).
    val refused = BleSpikeService.canSend(context)
    val phase = SpikeState.radio.phase(SystemClock.elapsedRealtime())
    val message = when {
        // The switch is on this very screen, so the next step is named here
        // rather than as 「先去主屏」. Any other refusal already says what to do.
        refused != null && phase == RadioPhase.OFF && PresenceKey.hasAny(context) -> "手机钥匙关着。先打开上面的开关。"
        refused != null -> refused
        BleSpikeService.postCommand(context, SpikeContract.CMD_LOCK) -> "已发出。Mac 在附近的话，几秒内会锁。"
        else -> "没发出去。"
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

/** Whether Android will leave this app running in the background. Read, never assumed. */
fun isBatteryExempt(context: Context): Boolean =
    context.getSystemService(android.os.PowerManager::class.java)
        ?.isIgnoringBatteryOptimizations(context.packageName) == true

/**
 * Open the system's own dialog. From a button the person pressed, never on
 * launch: an app that asks for this the moment it starts reads as malware.
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
        runCatching {
            context.startActivity(
                android.content.Intent(android.provider.Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS),
            )
        }
    }
}
