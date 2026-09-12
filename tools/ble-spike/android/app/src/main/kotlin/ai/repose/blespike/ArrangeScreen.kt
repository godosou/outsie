package ai.repose.blespike

import android.content.Context
import android.graphics.Typeface
import android.util.TypedValue
import android.view.Gravity
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
import android.view.ViewGroup.LayoutParams.WRAP_CONTENT
import android.widget.LinearLayout
import android.widget.TextView

/** Which app's actions the arranging screen is showing. Survives a rebuild. */
private var arrangingApp: String? = null

fun arrangeApp(name: String?) { arrangingApp = name }

/**
 * 挑选与排序 — what THIS phone shows, and in what order.
 *
 * HIDING IS NOT FORBIDDING, AND THE SCREEN HAS TO SAY SO
 *
 * Everything here is a local display preference. It never leaves the phone, is
 * never sent back to the Mac, does not touch the signed catalogue and does not
 * change a single cmd byte. A hidden action's byte still works — reinstall the
 * app and the button is back. The real boundary is one switch per phone on the
 * Mac, and a person who mistakes this screen for that one has locked a door
 * that was never shut.
 *
 * VISIBLE FIRST, HIDDEN UNDERNEATH
 *
 * One desk's catalogue came to 87 actions. Ordering 87 rows with ▲▼ would be
 * absurd, so the two jobs are separated: the top list is what you kept, short
 * by construction and therefore orderable; everything you put away sits below
 * it, where order does not matter because nothing is drawn from there.
 *
 * ▲▼ RATHER THAN DRAG
 *
 * The rows live inside a vertical ScrollView, and a drag handle inside a
 * scroller is a gesture fight that this app has no library to arbitrate — it is
 * built on plain Views on purpose. Two buttons cannot be ambiguous about which
 * gesture they were, and they work with one thumb. The hi-fi drawing shows drag
 * handles; that difference is written down in the PRD rather than left for
 * someone to discover.
 */
fun buildArrangeScreen(context: Context, nav: Nav, console: ConsoleServer): ScreenView {
    val pal = ReposeTheme.of(context)
    val catalogue = console.received ?: ConsoleCatalogue.load(context)
    var arr = ConsoleArrangement.load(context)

    lateinit var appBox: LinearLayout
    lateinit var actionBox: LinearLayout
    lateinit var actionLabel: TextView
    lateinit var footer: TextView

    fun persist(next: ConsoleArrangement) {
        arr = next
        ConsoleArrangement.save(context, next)
    }

    val root = screenScaffold(
        context, pal,
        title = "挑选与排序",
        lead = "只改这部手机显示什么、什么顺序。Mac 上不会变。",
        onBack = { nav.go(Screen.CONTROL) },
    ) { column ->

        if (catalogue == null || catalogue.apps.isEmpty()) {
            column.addView(
                Ui.infoNote(context, pal, "还没同步过按钮，没有东西可以排。"),
                Ui.lp(top = context.dp(14)),
            )
            appBox = LinearLayout(context)
            actionBox = LinearLayout(context)
            actionLabel = TextView(context)
            footer = Ui.secondary(context, pal, "")
            return@screenScaffold
        }

        // Said at the top, not in a footnote: someone who reads this screen as
        // a security control will stop looking for the real one.
        column.addView(
            Ui.amberNote(
                context, pal,
                "隐藏只是不显示，那个键照样能发。要禁止这部手机按键，" +
                    "去 Mac 上关掉它的「让它按快捷键」。",
            ),
            Ui.lp(top = context.dp(14)),
        )

        column.addView(sectionLabel(context, pal, "APP"), Ui.lp(top = context.dp(20), left = context.dp(4)))
        appBox = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
        column.addView(Ui.card(context, pal).apply { addView(appBox) }, Ui.lp(top = context.dp(10)))

        // Names the app rather than saying 「这个 APP」: the row above is
        // highlighted, but a highlight is a weaker answer to 「哪个」 than the
        // word itself, and this list is long enough to scroll it off screen.
        actionLabel = sectionLabel(context, pal, "")
        column.addView(actionLabel, Ui.lp(top = context.dp(22), left = context.dp(4)))
        actionBox = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
        column.addView(Ui.card(context, pal).apply { addView(actionBox) }, Ui.lp(top = context.dp(10)))

        footer = Ui.secondary(context, pal, "")
        column.addView(footer, Ui.lp(top = context.dp(14), left = context.dp(4)))

        // A preference that can empty itself. Without it, the only way back from
        // a mistake is remembering exactly what you hid.
        column.addView(
            Ui.ghostButton(context, pal, "恢复成 Mac 上的顺序") {
                persist(ConsoleArrangement())
                arrangingApp = null
                nav.go(Screen.ARRANGE)
            },
            Ui.lp(top = context.dp(14)),
        )
    }

    fun render() {
        val cat = catalogue ?: return
        val all = ConsoleArrangement.arrange(cat, arr, includeHidden = true)
        if (arrangingApp == null || all.none { it.name == arrangingApp }) {
            arrangingApp = all.firstOrNull()?.name
        }

        appBox.removeAllViews()
        all.forEachIndexed { i, app ->
            val shown = app.name !in arr.hiddenApps
            val visibleCount = app.actions.count { it.cmdByte !in arr.hiddenActions }
            appBox.addView(
                row(
                    context, pal,
                    title = app.name,
                    sub = when {
                        !shown -> "已隐藏"
                        visibleCount == app.actions.size -> "${app.actions.size} 个操作，都显示"
                        else -> "${app.actions.size} 个操作，显示 $visibleCount 个"
                    },
                    dimmed = !shown,
                    selected = app.name == arrangingApp,
                    canUp = i > 0,
                    canDown = i < all.size - 1,
                    onUp = {
                        persist(arr.copy(appOrder = ConsoleArrangement.moved(all.map { it.name }, i, i - 1)))
                        render()
                    },
                    onDown = {
                        persist(arr.copy(appOrder = ConsoleArrangement.moved(all.map { it.name }, i, i + 1)))
                        render()
                    },
                    toggleLabel = if (shown) "隐藏" else "显示",
                    onToggle = { persist(arr.toggleApp(app.name)); render() },
                    onPick = { arrangingApp = app.name; render() },
                ),
                Ui.lp(top = if (i == 0) 0 else context.dp(6)),
            )
        }

        actionBox.removeAllViews()
        val app = all.firstOrNull { it.name == arrangingApp }
        if (app == null) {
            actionLabel.text = "操作"
            actionBox.addView(Ui.secondary(context, pal, "没有可以排的 App。"))
            return
        }
        // A hidden app's actions are not going to be drawn no matter what any
        // single row says, so the whole block goes grey together. Dimming only
        // the app's own row left a list of bright, live-looking buttons
        // underneath a switch that had just turned all of them off.
        val appHidden = app.name in arr.hiddenApps
        actionLabel.alpha = if (appHidden) 0.55f else 1f
        actionBox.alpha = if (appHidden) 0.55f else 1f
        actionLabel.text = "「${app.name}」里的操作"
        if (appHidden) {
            actionBox.addView(
                Ui.secondary(context, pal, "这个 App 已隐藏，下面这些都不会显示。"),
            )
        }
        val visible = app.actions.filter { it.cmdByte !in arr.hiddenActions }
        val hidden = app.actions.filter { it.cmdByte in arr.hiddenActions }

        if (visible.isEmpty()) {
            actionBox.addView(
                Ui.secondary(context, pal, "全部隐藏了，上一页不会出现这个 App。"),
            )
        }
        visible.forEachIndexed { i, action ->
            actionBox.addView(
                row(
                    context, pal,
                    title = listOfNotNull(action.icon, action.name).joinToString("  "),
                    sub = action.keys.ifBlank { "没配按键" },
                    dimmed = false,
                    selected = false,
                    canUp = i > 0,
                    canDown = i < visible.size - 1,
                    // The whole order is written down, visible ones first and
                    // hidden after — so a move cannot shuffle anything the
                    // person did not touch, and unhiding puts it back where it
                    // was rather than at the Mac's position.
                    onUp = { persist(arr.copy(actionOrder = reordered(app, arr, i, i - 1))); render() },
                    onDown = { persist(arr.copy(actionOrder = reordered(app, arr, i, i + 1))); render() },
                    toggleLabel = "隐藏",
                    onToggle = { persist(arr.toggleAction(action.cmdByte)); render() },
                    onPick = null,
                ),
                Ui.lp(top = if (i == 0 && !appHidden) 0 else context.dp(6)),
            )
        }
        if (hidden.isNotEmpty()) {
            actionBox.addView(sectionLabel(context, pal, "已隐藏"), Ui.lp(top = context.dp(14)))
            hidden.forEach { action ->
                actionBox.addView(
                    row(
                        context, pal,
                        title = listOfNotNull(action.icon, action.name).joinToString("  "),
                        sub = action.keys.ifBlank { "没配按键" },
                        dimmed = true,
                        selected = false,
                        canUp = false,
                        canDown = false,
                        onUp = {}, onDown = {},
                        toggleLabel = "显示",
                        onToggle = { persist(arr.toggleAction(action.cmdByte)); render() },
                        onPick = null,
                    ),
                    Ui.lp(top = context.dp(6)),
                )
            }
        }

        footer.text = if (arr.isDefault) {
            "现在和 Mac 上一样。"
        } else {
            "已经排过了。同步不会冲掉。"
        }
    }
    render()

    return ScreenView(root)
}

/**
 * The order to store after moving the item at [from] to [to].
 *
 * Visible items first in their current order, then the hidden ones — so
 * un-hiding something puts it back next to where it was, instead of jumping to
 * wherever the Mac happens to list it.
 */
private fun reordered(app: ConsoleApp, arr: ConsoleArrangement, from: Int, to: Int): List<Int> {
    val visible = app.actions.map { it.cmdByte }.filter { it !in arr.hiddenActions }
    val hidden = app.actions.map { it.cmdByte }.filter { it in arr.hiddenActions }
    // Other apps' preferences must survive: one list holds every app's bytes.
    val mine = (visible + hidden).toSet()
    val others = arr.actionOrder.filter { it !in mine }
    return ConsoleArrangement.moved(visible, from, to) + hidden + others
}

private fun sectionLabel(context: Context, pal: Palette, text: String) = TextView(context).apply {
    this.text = text
    setTextColor(pal.textSecondary)
    setTextSize(TypedValue.COMPLEX_UNIT_SP, 10f)
    letterSpacing = 0.1f
    typeface = Typeface.MONOSPACE
}

/** One arrangeable thing: move it, turn it off, and (for apps) open it. */
private fun row(
    context: Context,
    pal: Palette,
    title: String,
    sub: String,
    dimmed: Boolean,
    selected: Boolean,
    canUp: Boolean,
    canDown: Boolean,
    onUp: () -> Unit,
    onDown: () -> Unit,
    toggleLabel: String,
    onToggle: () -> Unit,
    onPick: (() -> Unit)?,
): LinearLayout = LinearLayout(context).apply {
    orientation = LinearLayout.HORIZONTAL
    gravity = Gravity.CENTER_VERTICAL
    alpha = if (dimmed) 0.55f else 1f
    background = if (selected) {
        Ui.rounded(pal.accentSoft, context.dpF(12f), pal.accent, context.dp(2))
    } else {
        Ui.rounded(pal.surfaceMuted, context.dpF(12f))
    }
    setPadding(context.dp(10), context.dp(8), context.dp(10), context.dp(8))

    // Not drawn when it cannot work (ui-conventions 2.1): the first row has no
    // 「up」 and the last has no 「down」, so a disabled-looking arrow that does
    // nothing never appears.
    addView(arrow(context, pal, "▲", canUp, onUp))
    addView(arrow(context, pal, "▼", canDown, onDown), Ui.lp(width = WRAP_CONTENT, left = context.dp(2)))

    addView(
        LinearLayout(context).apply {
            orientation = LinearLayout.VERTICAL
            addView(
                TextView(context).apply {
                    text = title
                    setTextColor(pal.textPrimary)
                    setTextSize(TypedValue.COMPLEX_UNIT_SP, 14f)
                    typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
                },
            )
            addView(Ui.secondary(context, pal, sub), Ui.lp(top = context.dp(1)))
            if (onPick != null) {
                isClickable = true
                setOnClickListener { onPick() }
            }
        },
        LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f).also { it.leftMargin = context.dp(10) },
    )

    addView(
        TextView(context).apply {
            text = toggleLabel
            setTextColor(pal.accent)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 12.5f)
            setPadding(context.dp(10), context.dp(6), context.dp(10), context.dp(6))
            background = Ui.rounded(pal.surface, context.dpF(9f), pal.divider, context.dp(1))
            isClickable = true
            setOnClickListener { onToggle() }
        },
        Ui.lp(width = WRAP_CONTENT, left = context.dp(8)),
    )

    layoutParams = LinearLayout.LayoutParams(MATCH_PARENT, WRAP_CONTENT)
}

private fun arrow(context: Context, pal: Palette, glyph: String, enabled: Boolean, onClick: () -> Unit): TextView =
    TextView(context).apply {
        text = if (enabled) glyph else " "
        gravity = Gravity.CENTER
        setTextColor(pal.accent)
        setTextSize(TypedValue.COMPLEX_UNIT_SP, 13f)
        layoutParams = LinearLayout.LayoutParams(context.dp(30), context.dp(30))
        if (enabled) {
            background = Ui.rounded(pal.surface, context.dpF(8f), pal.divider, context.dp(1))
            isClickable = true
            setOnClickListener { onClick() }
        }
    }
