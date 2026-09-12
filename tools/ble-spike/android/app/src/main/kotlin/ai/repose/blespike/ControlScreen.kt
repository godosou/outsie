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
                chip(context, pal, app.name, app.name == name) { appName = app.name; renderGrid() },
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
        status.text = when {
            console.lastError != null -> console.lastError!!
            console.syncing -> "正在从 Mac 拿列表。Mac 要在附近，而且开着 Outsie。"
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
        isClickable = true
        isLongClickable = editing && !hiddenSection
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
private fun send(context: Context, act: ConsoleAction) {
    val queued = BleSpikeService.postCommand(context, act.cmdByte)
    Toast.makeText(
        context,
        when {
            !queued -> "还没有配对，Mac 不会接受。"
            !SpikeState.serviceRunning -> "手机钥匙关着。先去主屏打开。"
            else -> "已发出。"
        },
        Toast.LENGTH_SHORT,
    ).show()
}
