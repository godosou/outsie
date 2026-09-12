package ai.repose.blespike

import android.content.ClipData
import android.content.Context
import android.graphics.Typeface
import android.util.TypedValue
import android.view.DragEvent
import android.view.Gravity
import android.view.View
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
import android.view.ViewGroup.LayoutParams.WRAP_CONTENT
import android.widget.FrameLayout
import android.widget.HorizontalScrollView
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast

/** Which Mac the control screen is for. Set by the card you came from (design doc §04). */
private var controlTarget: Int = 0

fun controlMac(keyId: Int) { controlTarget = keyId }

/**
 * 控制 · 某台电脑 — one Mac's buttons, as this phone wants to see them.
 *
 * ONE MAC (design doc §04)
 *
 * This screen belongs to the computer whose card you pressed 「控制」 on. Its
 * title is that Mac's name; its buttons are that Mac's catalogue and nobody
 * else's. With no catalogue for THIS Mac it says so, and does not borrow one:
 * the buttons look the same, the bytes are another machine's.
 *
 * A GRID, AND 整理 IN PLACE (§10)
 *
 * One tile per action, three across. 「整理」 turns the same grid into the
 * editor: a × badge hides a tile, the hidden ones gather underneath with a ＋,
 * and a long press lifts a tile so it can be dropped onto another. There is no
 * separate arranging page — the thing you are arranging is right there.
 *
 * WHAT A BUTTON CAN HONESTLY SAY
 *
 * 已发出, never 已按下. The command rides the one-way beacon, so this phone
 * knows it transmitted and nothing else.
 */
fun buildControlScreen(context: Context, nav: Nav, console: ConsoleServer, store: AppStore): ScreenView {
    val pal = ReposeTheme.of(context)
    val target = controlTarget
    val mac = store.pairedMacs(context).firstOrNull { it.keyId == target }
    var catalogue = ConsoleCatalogue.own(console.received, target) ?: ConsoleCatalogue.load(context, target)
    var arr = ConsoleArrangement.load(context, target)
    var editing = false
    var appName: String? = catalogue?.apps?.firstOrNull()?.name

    lateinit var status: TextView
    // What was on the shelf when 「同步」 was last pressed here, so a window
    // that closed on another Mac's answer can be told apart from one that
    // closed on nothing. The server files each list under the key that signed
    // it (§08), so the wrong Mac's list never lands in this slot -- but it
    // does close the window, and without this the screen would fall silent.
    var asked = false
    var receivedWhenAsked: ConsoleCatalogue? = null
    var chips: LinearLayout? = null
    var gridHolder: LinearLayout? = null
    var editBtn: TextView? = null
    var resetBtn: TextView? = null
    var editNote: View? = null

    fun persist(next: ConsoleArrangement) {
        arr = next
        ConsoleArrangement.save(context, target, next)
    }

    lateinit var renderGrid: () -> Unit

    val root = screenScaffold(
        context, pal,
        title = mac?.name ?: "一台 Mac",
        // Said before the press: bringing the app to the front is a visible thing
        // that happens to the Mac. Finding that out by pressing is being
        // surprised by your own tool.
        lead = "按一下，那台 Mac 会先切到这个 App，再替你按键。",
        onBack = { nav.go(Screen.HOME) },
    ) { column ->
        val syncRow = LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        status = Ui.secondary(context, pal, "")
        syncRow.addView(status, LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f))
        syncRow.addView(
            Ui.ghostButton(context, pal, "同步") {
                asked = true
                receivedWhenAsked = console.received
                console.request()
                SpikeState.notifyListeners()
            },
            Ui.lp(width = WRAP_CONTENT, left = context.dp(12)),
        )
        column.addView(syncRow, Ui.lp(top = context.dp(10)))

        val cat = catalogue
        if (cat == null || cat.apps.isEmpty()) {
            column.addView(
                Ui.infoNote(context, pal, "这台电脑还没同步过按钮。在 Mac 的「快捷键设置」里配好，同步过来就在这里。"),
                Ui.lp(top = context.dp(14)),
            )
        } else {
            val row = LinearLayout(context).apply { orientation = LinearLayout.HORIZONTAL }
            chips = row
            column.addView(
                HorizontalScrollView(context).apply { isHorizontalScrollBarEnabled = false; addView(row) },
                Ui.lp(top = context.dp(14)),
            )
            val holder = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
            gridHolder = holder
            column.addView(holder, Ui.lp(top = context.dp(12)))

            val tools = LinearLayout(context).apply { orientation = LinearLayout.HORIZONTAL }
            editBtn = link(context, pal, "整理") { editing = !editing; renderGrid() }
            resetBtn = link(context, pal, "恢复成 Mac 上的顺序") { persist(ConsoleArrangement()); renderGrid() }
            tools.addView(editBtn, LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f))
            tools.addView(resetBtn, Ui.lp(width = WRAP_CONTENT))
            column.addView(tools, Ui.lp(top = context.dp(12), left = context.dp(4), right = context.dp(4)))

            // Hiding is not forbidding, and the screen says so where the hiding
            // happens (§10). Someone who reads 整理 as a security control will
            // stop looking for the real one.
            editNote = Ui.amberNote(context, pal, "收起来只是这里不显示。要真的不让这部手机按键，去 Mac 上关掉它。")
            column.addView(editNote, Ui.lp(top = context.dp(12)))
        }

        column.addView(
            Ui.infoNote(context, pal, "按钮只负责发出去。按没按成，要看那台 Mac。"),
            Ui.lp(top = context.dp(14)),
        )
    }

    renderGrid = fun() {
        val cat = catalogue ?: return
        val row = chips ?: return
        val holder = gridHolder ?: return
        if (appName == null || cat.apps.none { it.name == appName }) appName = cat.apps.firstOrNull()?.name
        val name = appName ?: return

        row.removeAllViews()
        cat.apps.forEachIndexed { i, app ->
            row.addView(
                chip(context, pal, app.name, app.name == name) {
                    appName = app.name
                    renderGrid()
                    // The name is a button too: the Mac switches to that App,
                    // no key pressed. On the device people tapped 「飞书」 and
                    // expected exactly that, twice.
                    app.cmdByte?.let { sendByte(context, it) }
                },
                Ui.lp(width = WRAP_CONTENT, left = if (i == 0) 0 else context.dp(8)),
            )
        }

        holder.removeAllViews()
        val all = ConsoleArrangement.arrange(cat, arr, includeHidden = true).first { it.name == name }
        val visible = all.actions.filter { it.cmdByte !in arr.hidden }
        val hidden = all.actions.filter { it.cmdByte in arr.hidden }

        if (visible.isEmpty()) {
            holder.addView(Ui.secondary(context, pal, "这个 App 的按钮都被你收起来了。"), Ui.lp(left = context.dp(4)))
        }
        holder.addView(grid(context, pal, visible, editing, hiddenSection = false) { act, v ->
            if (editing) {
                // A tap while arranging must say so. On the device the tiles
                // were pressed in 整理 mode and nothing happened, which read
                // as 「控制不行」 (ui-conventions 2.3: an action that changes
                // nothing on screen invites you to do it again).
                v.setOnClickListener {
                    Toast.makeText(context, "整理中。先点「完成」，再按。", Toast.LENGTH_SHORT).show()
                }
                // Only the badge hides; the tile body is what you grab to drag.
                v.setOnLongClickListener {
                    it.startDragAndDrop(ClipData.newPlainText("cmd", act.cmdByte.toString()), View.DragShadowBuilder(it), act.cmdByte, 0)
                    true
                }
                v.setOnDragListener { view, e ->
                    when (e.action) {
                        DragEvent.ACTION_DRAG_STARTED -> true
                        DragEvent.ACTION_DRAG_ENTERED -> { view.alpha = 0.6f; true }
                        DragEvent.ACTION_DRAG_EXITED, DragEvent.ACTION_DRAG_ENDED -> { view.alpha = 1f; true }
                        DragEvent.ACTION_DROP -> {
                            val from = e.localState as? Int ?: return@setOnDragListener false
                            persist(ConsoleArrangement.dropped(cat, name, arr, from, act.cmdByte))
                            renderGrid()
                            true
                        }
                        else -> true
                    }
                }
            } else {
                v.setOnClickListener { send(context, act) }
            }
        }.also { g ->
            if (editing) attachBadges(g, "×") { cmd -> persist(arr.toggle(cmd)); renderGrid() }
        })

        if (editing && hidden.isNotEmpty()) {
            holder.addView(
                TextView(context).apply {
                    text = "收起来的"
                    setTextColor(pal.textSecondary)
                    setTextSize(TypedValue.COMPLEX_UNIT_SP, 10f)
                    letterSpacing = 0.1f
                    typeface = Typeface.MONOSPACE
                },
                Ui.lp(top = context.dp(16), left = context.dp(4)),
            )
            holder.addView(grid(context, pal, hidden, true, hiddenSection = true) { _, _ -> }.also { g ->
                attachBadges(g, "＋") { cmd -> persist(arr.toggle(cmd)); renderGrid() }
            }, Ui.lp(top = context.dp(6)))
        }

        editBtn?.text = if (editing) "完成" else "整理"
        resetBtn?.visibility = if (editing) View.VISIBLE else View.GONE
        editNote?.visibility = if (editing) View.VISIBLE else View.GONE
    }
    renderGrid()

    val renderedCount = catalogue?.apps?.sumOf { it.actions.size } ?: 0

    fun refresh() {
        val fresh = ConsoleCatalogue.own(console.received, target)
        val n = fresh?.apps?.sumOf { it.actions.size } ?: renderedCount
        // A catalogue for THIS Mac arriving while the screen is open changes
        // what the screen is, not just its status line. Rebuild.
        if (fresh != null && n != renderedCount) {
            nav.go(Screen.CONTROL)
            return
        }
        // Every wait says what is happening, what ends it and how long; every
        // failure says what to do (design doc §01). The window is
        // ConsoleServer.WINDOW_MS, sixty seconds: the sentence says so, and
        // when it closes on nothing the server's own sentence lands in
        // lastError -- this screen adds the one next step there is.
        val error = console.lastError
        val answeredByAnother = asked && !console.syncing && error == null &&
            console.received !== receivedWhenAsked && fresh == null
        val sameAgain = asked && !console.syncing && error == null &&
            console.received !== receivedWhenAsked && fresh != null
        status.text = when {
            // Only the window closing on nothing has a next step worth naming;
            // 「蓝牙不支持」 and 「不是你的 Mac 发的」 are not fixed by pressing again.
            error != null -> if (error.startsWith("没同步成")) "${error}好了，再按一次「同步」。" else error
            console.syncing -> "正在从 Mac 拿列表，拿到按钮就出现。Mac 要在附近，而且开着 Outsie。最多等一分钟，没拿到这里会说。"
            answeredByAnother -> "拿到的是另一台 Mac 的列表。这一台还没有，再按一次「同步」。"
            sameAgain -> "同步好了，和上次一样。${renderedCount} 个操作。"
            catalogue != null -> "${renderedCount} 个操作。在 Mac 上改过，就再同步一次。"
            else -> "还没同步过。"
        }
    }
    refresh()

    return ScreenView(root, onState = { refresh() })
}

/** Three tiles across, rows as needed. The last row is padded so tiles keep their width. */
private fun grid(
    context: Context,
    pal: Palette,
    actions: List<ConsoleAction>,
    editing: Boolean,
    hiddenSection: Boolean,
    wire: (ConsoleAction, View) -> Unit,
): LinearLayout {
    val g = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
    actions.chunked(3).forEachIndexed { r, rowActions ->
        val row = LinearLayout(context).apply { orientation = LinearLayout.HORIZONTAL }
        rowActions.forEachIndexed { i, act ->
            val t = tile(context, pal, act, editing, hiddenSection)
            wire(act, t)
            row.addView(t, LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f).also { if (i > 0) it.leftMargin = context.dp(8) })
        }
        repeat(3 - rowActions.size) {
            row.addView(View(context), LinearLayout.LayoutParams(0, 1, 1f).also { it.leftMargin = context.dp(8) })
        }
        g.addView(row, Ui.lp(top = if (r == 0) 0 else context.dp(8)))
    }
    return g
}

/** One tile: icon over name. Tagged with its cmd byte so a badge can find it. */
private fun tile(context: Context, pal: Palette, act: ConsoleAction, editing: Boolean, hiddenSection: Boolean): FrameLayout {
    val body = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        gravity = Gravity.CENTER
        background = Ui.rounded(if (hiddenSection) pal.surfaceMuted else pal.surface, context.dpF(14f), pal.divider, context.dp(1))
        setPadding(context.dp(8), context.dp(12), context.dp(8), context.dp(10))
        addView(TextView(context).apply {
            text = act.icon ?: "⌨"
            setTextColor(pal.accent)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 22f)
            gravity = Gravity.CENTER
        })
        addView(TextView(context).apply {
            text = act.name
            setTextColor(pal.textPrimary)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 11.5f)
            typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
            gravity = Gravity.CENTER
            maxLines = 2
        }, Ui.lp(top = context.dp(7)))
        alpha = if (hiddenSection) 0.55f else 1f
        // NOT clickable. The listeners are wired onto the FrameLayout around
        // this body, and a clickable child swallows the touch before it gets
        // there -- so no tile ever sent anything. Found on the device
        // 2026-09-12 (「控制还是不行，我不在整理模式下」): the phone logged
        // no queued command at all while the person tapped.
        isClickable = false
        isLongClickable = false
    }
    return FrameLayout(context).apply {
        tag = act.cmdByte
        clipChildren = false
        clipToPadding = false
        addView(body, FrameLayout.LayoutParams(MATCH_PARENT, WRAP_CONTENT))
    }
}

/** A round badge in each tile's top-right corner. */
private fun attachBadges(g: LinearLayout, glyph: String, onTap: (Int) -> Unit) {
    val context = g.context
    val pal = ReposeTheme.of(context)
    for (r in 0 until g.childCount) {
        val row = g.getChildAt(r) as? LinearLayout ?: continue
        for (i in 0 until row.childCount) {
            val t = row.getChildAt(i) as? FrameLayout ?: continue
            val cmd = t.tag as? Int ?: continue
            t.addView(
                TextView(context).apply {
                    text = glyph
                    gravity = Gravity.CENTER
                    setTextColor(pal.surface)
                    setTextSize(TypedValue.COMPLEX_UNIT_SP, 12f)
                    background = Ui.rounded(pal.accent, context.dpF(11f))
                    setOnClickListener { onTap(cmd) }
                },
                FrameLayout.LayoutParams(context.dp(22), context.dp(22), Gravity.TOP or Gravity.END).also {
                    it.topMargin = -context.dp(6); it.rightMargin = -context.dp(6)
                },
            )
        }
    }
}

private fun chip(context: Context, pal: Palette, label: String, on: Boolean, onClick: () -> Unit): TextView =
    TextView(context).apply {
        text = label
        setTextSize(TypedValue.COMPLEX_UNIT_SP, 13f)
        setPadding(context.dp(14), context.dp(8), context.dp(14), context.dp(8))
        background = Ui.rounded(if (on) pal.accentSoft else pal.surfaceMuted, context.dpF(999f))
        setTextColor(if (on) pal.textPrimary else pal.textSecondary)
        typeface = Typeface.create(if (on) "sans-serif-medium" else "sans-serif", Typeface.NORMAL)
        setOnClickListener { onClick() }
    }

private fun link(context: Context, pal: Palette, label: String, onClick: () -> Unit): TextView =
    TextView(context).apply {
        text = label
        setTextColor(pal.accent)
        setTextSize(TypedValue.COMPLEX_UNIT_SP, 12.5f)
        isClickable = true
        setOnClickListener { onClick() }
    }

/** Queue the byte and say what this phone can honestly know, which is not much. */
private fun send(context: Context, act: ConsoleAction) = sendByte(context, act.cmdByte)

private fun sendByte(context: Context, byte: Int) {
    // Ask first, so the toast names the real reason and nothing is queued for
    // a key that is off (design doc §09).
    val refused = BleSpikeService.canSend(context)
    val message = when {
        refused != null -> refused
        BleSpikeService.postCommand(context, byte) -> "已发出。"
        else -> "没发出去。"
    }
    Toast.makeText(context, message, Toast.LENGTH_SHORT).show()
}
