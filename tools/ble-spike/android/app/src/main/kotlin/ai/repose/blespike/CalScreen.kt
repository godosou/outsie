package ai.repose.blespike

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.view.Gravity
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast

/** Which Mac the calibration screen is for. Set by the card you came from. */
private var calTarget: Int = 0

/**
 * The calFlow in progress, kept outside the screen. MainActivity rebuilds the
 * current screen on every resume -- a lock, a call, a glance at another app
 * while you stand at the far spot -- and a calFlow that lived in the screen would
 * snap back to the start while the Mac carried on measuring.
 */
private var calFlow: CalFlow? = null

/**
 * When 「远处结束」 last went out. Outside the screen for the same reason as
 * [calFlow]: a rebuild while walking back must not fire it again at once.
 */
private var farDoneSentAt: Long = 0L

fun calMac(keyId: Int) { if (keyId != calTarget) { calFlow = null; farDoneSentAt = 0L }; calTarget = keyId }

/**
 * 量距离 — on the phone, because you are the one walking (design doc §05 §06).
 *
 * The phone tells the Mac when to start each leg and shows what to do; the
 * Mac samples for 20 s and reports its phase in the state beacon; [CalFlow]
 * follows that. The screen never claims a result the Mac has not reported --
 * except the one it can see for itself: a far spot where the Mac never
 * answered, which it carries back to the Mac as 「远处结束」.
 */
fun buildCalScreen(context: Context, nav: Nav, store: AppStore): ScreenView {
    val pal = ReposeTheme.of(context)
    val target = calTarget
    val mac = store.pairedMacs(context).firstOrNull { it.keyId == target }
    val macId = mac?.macId?.toIntOrNull(16)
    var shown: CalStep = CalStep.INTRO
    val handler = Handler(Looper.getMainLooper())
    lateinit var body: LinearLayout
    lateinit var render: () -> Unit
    // What the ticker updates in place between renders.
    var countdown: CountdownView? = null
    var caption: TextView? = null

    // Ask before posting: postCommand refuses for either reason with the same
    // `false`, and only canSend says which one, so the toast names the real one.
    fun send(cmd: Int): Boolean {
        val reason = BleSpikeService.canSend(context)
        if (reason != null) {
            Toast.makeText(context, reason, Toast.LENGTH_LONG).show()
            return false
        }
        return BleSpikeService.postCommand(context, cmd)
    }

    // 「远处结束」, on entering RETURN and again every 20 s while there. The
    // first one may say why it was refused; the repeats stay quiet, because a
    // toast every 20 s about a key that is off helps nobody walk.
    fun sendFarDone(now: Long, quiet: Boolean) {
        farDoneSentAt = now
        if (quiet) BleSpikeService.postCommand(context, SpikeContract.CMD_CALIBRATE_FAR_DONE)
        else send(SpikeContract.CMD_CALIBRATE_FAR_DONE)
    }

    val root = screenScaffold(
        context, pal,
        title = mac?.name ?: "一台 Mac",
        onBack = { nav.go(Screen.HOME) },
    ) { column ->
        body = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
        column.addView(body, Ui.lp(top = context.dp(6)))
    }

    fun captionFor(f: CalFlow, now: Long): String {
        val left = f.remainingMs(now)
        return when (f.step) {
            CalStep.NEAR -> if (left == null) "正在叫 Mac 开始…" else if (left > 0) "看这块屏就好。Mac 那边什么都不用做。" else "快好了，等 Mac 回话。"
            CalStep.FAR -> if (left != null && left > 0) "别看屏，站着就行。" else if (f.legSeen != null) "快好了，等 Mac 回话。" else "Mac 没听到，正在定下来…"
            else -> ""
        }
    }

    fun legBlock(f: CalFlow, now: Long) {
        val ring = CountdownView(context, pal).also { countdown = it }
        val holder = LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER_HORIZONTAL
            addView(ring, Ui.lp(width = context.dp(168), height = context.dp(168)))
        }
        ring.set(f.remainingMs(now))
        body.addView(holder, Ui.lp(top = context.dp(22)))
        body.addView(
            Ui.secondary(context, pal, captionFor(f, now)).apply {
                gravity = Gravity.CENTER_HORIZONTAL
                caption = this
            },
            Ui.lp(top = context.dp(14), left = context.dp(4), right = context.dp(4)),
        )
    }

    render = fun() {
        body.removeAllViews()
        countdown = null
        caption = null
        val now = SystemClock.elapsedRealtime()
        val f = calFlow
        val step = f?.step ?: CalStep.INTRO
        shown = step
        // Read each time, not once: the first success on this screen flips it,
        // and a failure right after must already talk about「上次的」.
        val calibrated = store.calibratedOnce(target)
        when (step) {
            CalStep.INTRO -> {
                body.addView(heroCard(context, pal, chip = "量距离", glyph = "📏", headline = "教它认出你的距离",
                    body = "Mac 靠信号强弱猜你在不在。每个房间都不一样，所以要在你平常用它的地方量一次。坐着量 20 秒，走开再量 20 秒。"))
                body.addView(Ui.infoNote(context, pal,
                    if (calibrated) "量过一次。重新量的时候，旧的先照常用着；量不出结果，旧的也不会变。"
                    else "这台电脑还没量过。"), Ui.lp(top = context.dp(12)))
                body.addView(Ui.primaryButton(context, pal, "先坐到平常的位置") {
                    if (send(SpikeContract.CMD_CALIBRATE_NEAR)) { calFlow = CalFlow.start(SystemClock.elapsedRealtime()); render() }
                }, Ui.lp(top = context.dp(14)))
            }
            CalStep.NEAR -> {
                body.addView(heroCard(context, pal, chip = "第一段", glyph = "🪑", headline = "坐着别动",
                    body = "正在量你在座位上时的信号。看这块屏就好。"))
                legBlock(f!!, now)
            }
            CalStep.WALK -> {
                body.addView(heroCard(context, pal, chip = "第二段", glyph = "🚶", headline = "现在拿着手机走开",
                    body = "走到你平常会离开的距离。门口、茶水间、另一个房间都行。到了再按下面这个。"))
                body.addView(Ui.infoNote(context, pal, "路上不量。量的是你站定之后的信号，走动中的那一段混进去，两边就会像。走到 Mac 听不见的地方也行，那最清楚。"), Ui.lp(top = context.dp(12)))
                body.addView(Ui.primaryButton(context, pal, "到了，开始量") {
                    if (send(SpikeContract.CMD_CALIBRATE_FAR)) { calFlow = calFlow?.arrived(SystemClock.elapsedRealtime()); render() }
                }, Ui.lp(top = context.dp(14)))
            }
            CalStep.FAR -> {
                body.addView(heroCard(context, pal, chip = "第二段", glyph = "🧍", headline = "站着别动",
                    body = "正在量你走开之后的信号。"))
                legBlock(f!!, now)
            }
            CalStep.RETURN -> {
                body.addView(heroCard(context, pal, chip = "第二段", glyph = "🚶", headline = "走回去就完成",
                    body = "走开之后 Mac 听不到手机了。这最清楚。走回 Mac 旁边，它就把这次记下来。"))
                body.addView(Ui.infoNote(context, pal, "回到座位上还没反应的话，等一小会儿。手机每 20 秒跟它说一次。"), Ui.lp(top = context.dp(12)))
                // On entry only: a rebuild while walking back must not fire it
                // again at once. The ticker handles the repeats.
                if (now - farDoneSentAt >= CalFlow.RETURN_REPOST_MS) sendFarDone(now, quiet = false)
            }
            CalStep.OK -> {
                // The Mac reported a result it will use, so from now on this
                // Mac has a「上次的」to fall back on.
                store.markCalibrated(target)
                body.addView(heroCard(context, pal, chip = "量好了", glyph = "✅", headline = "分得很清楚",
                    body = "在座位上和走开了，Mac 现在分得出来。从这次起就用新的。"))
                body.addView(Ui.primaryButton(context, pal, "好") { nav.go(Screen.HOME) }, Ui.lp(top = context.dp(14)))
            }
            CalStep.FAIL_ALIKE, CalStep.FAIL_SILENT -> {
                val alike = step == CalStep.FAIL_ALIKE
                body.addView(heroCard(context, pal, chip = "这次没量出来", glyph = "🤔",
                    headline = if (alike) "两边太像了" else "Mac 没听到手机",
                    body = if (alike) "在座位上和走开了，信号差不多。这次的量不出结果，不能用。"
                           else "坐着的那 20 秒，Mac 收不到这部手机的信号。", muted = true))
                // A first attempt has nothing to fall back on but the defaults;
                // 「先用上次的」would promise numbers that were never measured.
                val stillUsing = if (calibrated) "上次量的那组还在用，一个数都没变。" else "现在用的是默认值。"
                body.addView(Ui.infoNote(context, pal,
                    if (alike) "常见原因：走得不够远，手机在包里，中间隔着墙或金属。换个走法再来一次通常就好。$stillUsing"
                    else "手机钥匙开着吗？靠近一点再试。$stillUsing"), Ui.lp(top = context.dp(12)))
                body.addView(Ui.primaryButton(context, pal, "再量一次") {
                    if (send(SpikeContract.CMD_CALIBRATE_NEAR)) { calFlow = CalFlow.start(SystemClock.elapsedRealtime()); render() }
                }, Ui.lp(top = context.dp(14)))
                body.addView(Ui.ghostButton(context, pal, if (calibrated) "先用上次的" else "先用默认值") { nav.go(Screen.HOME) }, Ui.lp(top = context.dp(10)))
            }
            CalStep.NO_ANSWER -> {
                body.addView(heroCard(context, pal, chip = "没回音", glyph = "📡", headline = "Mac 没回音",
                    body = "它要在附近，而且开着 Outsie。", muted = true))
                body.addView(Ui.primaryButton(context, pal, "再试一次") {
                    if (send(SpikeContract.CMD_CALIBRATE_NEAR)) { calFlow = CalFlow.start(SystemClock.elapsedRealtime()); render() }
                }, Ui.lp(top = context.dp(14)))
                body.addView(Ui.ghostButton(context, pal, "返回") { nav.go(Screen.HOME) }, Ui.lp(top = context.dp(10)))
            }
        }
    }
    render()

    // Five times a second: follow the Mac's beacon, keep the ring honest, and
    // keep saying 「远处结束」 while walking back. The screen re-renders only
    // when the step changes, and refreshes the ring and caption in place
    // otherwise.
    var live = true
    val tick = object : Runnable {
        override fun run() {
            val f = calFlow
            if (f != null) {
                val now = SystemClock.elapsedRealtime()
                // By the slot first, for the same reason the home card does:
                // the Mac's phase arrives under the key, and a slot paired by an
                // older build has no stored id to look it up by.
                val phase = MacState.sightingFor(target, now)?.beacon ?: macId?.let { MacState.beaconOf(it, now) }
                val next = f.on(phase, now)
                calFlow = next
                if (next.step != shown) {
                    render()
                } else {
                    countdown?.set(next.remainingMs(now))
                    caption?.let { c -> captionFor(next, now).let { t -> if (c.text != t) c.text = t } }
                    if (next.step == CalStep.RETURN && now - farDoneSentAt >= CalFlow.RETURN_REPOST_MS) {
                        sendFarDone(now, quiet = true)
                    }
                }
            }
            if (live) handler.postDelayed(this, TICK_MS)
        }
    }
    handler.postDelayed(tick, TICK_MS)
    // The screen is rebuilt on every navigation; a ticker that outlived its
    // screen would keep following the beacon for a body nobody can see.
    root.addOnAttachStateChangeListener(object : android.view.View.OnAttachStateChangeListener {
        override fun onViewAttachedToWindow(v: android.view.View) {}
        override fun onViewDetachedFromWindow(v: android.view.View) { live = false; handler.removeCallbacks(tick) }
    })

    return ScreenView(root, onState = { })
}

private const val TICK_MS = 200L
