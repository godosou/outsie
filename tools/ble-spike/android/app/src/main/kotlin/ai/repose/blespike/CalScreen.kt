package ai.repose.blespike

import android.content.Context
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast

/** Which Mac the calibration screen is for. Set by the card you came from. */
private var calTarget: Int = 0

fun calMac(keyId: Int) { calTarget = keyId }

/**
 * 量距离 — on the phone, because you are the one walking (design doc §05 §06).
 *
 * The phone tells the Mac when to start each leg and shows what to do; the
 * Mac samples and reports its phase in the state beacon; [CalFlow] follows
 * that. The screen never claims a result the Mac has not reported.
 */
fun buildCalScreen(context: Context, nav: Nav, store: AppStore): ScreenView {
    val pal = ReposeTheme.of(context)
    val target = calTarget
    val mac = store.pairedMacs(context).firstOrNull { it.keyId == target }
    val macId = mac?.macId?.toIntOrNull(16)
    var flow: CalFlow? = null
    var shown: CalStep = CalStep.INTRO
    val handler = Handler(Looper.getMainLooper())
    lateinit var body: LinearLayout
    lateinit var render: () -> Unit

    fun send(cmd: Int): Boolean {
        val queued = BleSpikeService.postCommand(context, cmd)
        if (!queued) Toast.makeText(context, "还没有配对，Mac 不会接受。", Toast.LENGTH_LONG).show()
        else if (!SpikeState.serviceRunning) Toast.makeText(context, "手机钥匙关着。先去主屏打开。", Toast.LENGTH_LONG).show()
        return queued && SpikeState.serviceRunning
    }

    val root = screenScaffold(
        context, pal,
        title = mac?.name ?: "一台 Mac",
        onBack = { nav.go(Screen.HOME) },
    ) { column ->
        body = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
        column.addView(body, Ui.lp(top = context.dp(6)))
    }

    fun secs(): Long = ((SystemClock.elapsedRealtime() - (flow?.since ?: 0L)) / 1000L).coerceAtLeast(0)

    render = fun() {
        body.removeAllViews()
        val step = flow?.step ?: CalStep.INTRO
        shown = step
        when (step) {
            CalStep.INTRO -> {
                body.addView(heroCard(context, pal, chip = "量距离", glyph = "📏", headline = "教它认出你的距离",
                    body = "Mac 靠信号强弱猜你在不在。每个房间都不一样，所以要在你平常用它的地方量一次。"))
                body.addView(Ui.infoNote(context, pal, "重新量的时候，旧的先照常用着；量不出结果，旧的也不会变。"), Ui.lp(top = context.dp(12)))
                body.addView(Ui.primaryButton(context, pal, "先坐到平常的位置") {
                    if (send(SpikeContract.CMD_CALIBRATE_NEAR)) { flow = CalFlow.start(SystemClock.elapsedRealtime()); render() }
                }, Ui.lp(top = context.dp(14)))
            }
            CalStep.NEAR -> {
                body.addView(heroCard(context, pal, chip = "第一段", glyph = "🪑", headline = "坐着别动",
                    body = "正在量你在座位上时的信号。看这块屏就好，Mac 那边什么都不用做。"))
                body.addView(Ui.secondary(context, pal, "${secs()} 秒了。"), Ui.lp(top = context.dp(12), left = context.dp(4)))
            }
            CalStep.WALK -> {
                body.addView(heroCard(context, pal, chip = "第二段", glyph = "🚶", headline = "现在拿着手机走开",
                    body = "走到你平常会离开的距离。门口、茶水间、另一个房间都行。到了再按下面这个。"))
                body.addView(Ui.infoNote(context, pal, "路上不量。量的是你站定之后的信号，走动中的那一段混进去，两边就会像。"), Ui.lp(top = context.dp(12)))
                body.addView(Ui.primaryButton(context, pal, "到了，开始量") {
                    if (send(SpikeContract.CMD_CALIBRATE_FAR)) { flow = flow?.arrived(SystemClock.elapsedRealtime()); render() }
                }, Ui.lp(top = context.dp(14)))
            }
            CalStep.FAR -> {
                body.addView(heroCard(context, pal, chip = "第二段", glyph = "🧍", headline = "站着别动",
                    body = "正在量你走开之后的信号。"))
                body.addView(Ui.secondary(context, pal, "${secs()} 秒了。别看屏，站着就行。"), Ui.lp(top = context.dp(12), left = context.dp(4)))
            }
            CalStep.OK -> {
                body.addView(heroCard(context, pal, chip = "量好了", glyph = "✅", headline = "分得很清楚",
                    body = "在座位上和走开了，Mac 现在分得出来。从这次起就用新的。"))
                body.addView(Ui.primaryButton(context, pal, "好") { nav.go(Screen.HOME) }, Ui.lp(top = context.dp(14)))
            }
            CalStep.FAIL_ALIKE, CalStep.FAIL_SILENT -> {
                val alike = step == CalStep.FAIL_ALIKE
                body.addView(heroCard(context, pal, chip = "这次没量出来", glyph = "🤔",
                    headline = if (alike) "两边太像了" else "Mac 没听到手机",
                    body = if (alike) "在座位上和走开了，信号差不多。这次的量不出结果，不能用。"
                           else "量的时候 Mac 收不到这部手机的信号。", muted = true))
                body.addView(Ui.infoNote(context, pal,
                    if (alike) "常见原因：走得不够远，手机在包里，中间隔着墙或金属。换个走法再来一次通常就好。上次量的那组还在用，一个数都没变。"
                    else "手机钥匙开着吗？靠近一点再试。上次量的那组还在用，一个数都没变。"), Ui.lp(top = context.dp(12)))
                body.addView(Ui.primaryButton(context, pal, "再量一次") {
                    if (send(SpikeContract.CMD_CALIBRATE_NEAR)) { flow = CalFlow.start(SystemClock.elapsedRealtime()); render() }
                }, Ui.lp(top = context.dp(14)))
                body.addView(Ui.ghostButton(context, pal, "先用上次的") { nav.go(Screen.HOME) }, Ui.lp(top = context.dp(10)))
            }
            CalStep.NO_ANSWER -> {
                body.addView(heroCard(context, pal, chip = "没回音", glyph = "📡", headline = "Mac 没回音",
                    body = "它要在附近，而且开着 Outsie。", muted = true))
                body.addView(Ui.primaryButton(context, pal, "再试一次") {
                    if (send(SpikeContract.CMD_CALIBRATE_NEAR)) { flow = CalFlow.start(SystemClock.elapsedRealtime()); render() }
                }, Ui.lp(top = context.dp(14)))
                body.addView(Ui.ghostButton(context, pal, "返回") { nav.go(Screen.HOME) }, Ui.lp(top = context.dp(10)))
            }
        }
    }
    render()

    // Once a second: follow the Mac's beacon, and keep the elapsed seconds
    // honest. The screen re-renders only when the step changes, and refreshes
    // the counter in place otherwise.
    var live = true
    val tick = object : Runnable {
        override fun run() {
            val f = flow
            if (f != null) {
                val now = SystemClock.elapsedRealtime()
                val next = f.on(macId?.let { MacState.beaconOf(it, now) }, now)
                flow = next
                if (next.step != shown) render()
                else if (next.step == CalStep.NEAR || next.step == CalStep.FAR) {
                    (body.getChildAt(1) as? TextView)?.text =
                        if (next.step == CalStep.NEAR) "${secs()} 秒了。" else "${secs()} 秒了。别看屏，站着就行。"
                }
            }
            if (live) handler.postDelayed(this, 1000)
        }
    }
    handler.postDelayed(tick, 1000)
    // The screen is rebuilt on every navigation; a ticker that outlived its
    // screen would keep following the beacon for a body nobody can see.
    root.addOnAttachStateChangeListener(object : android.view.View.OnAttachStateChangeListener {
        override fun onViewAttachedToWindow(v: android.view.View) {}
        override fun onViewDetachedFromWindow(v: android.view.View) { live = false; handler.removeCallbacks(tick) }
    })

    return ScreenView(root, onState = { })
}
