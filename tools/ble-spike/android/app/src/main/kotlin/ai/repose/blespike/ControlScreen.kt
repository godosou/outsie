package ai.repose.blespike

import android.content.Context
import android.graphics.Typeface
import android.util.TypedValue
import android.view.Gravity
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
import android.view.ViewGroup.LayoutParams.WRAP_CONTENT
import android.widget.HorizontalScrollView
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast

/**
 * 控制 — the buttons this Mac said it would accept.
 *
 * READ-ONLY, AND THAT IS THE DESIGN
 *
 * Nothing here can be edited. Configuring a computer's keyboard shortcuts on a
 * phone is putting the hardest input on the smallest screen; the Mac owns the
 * list and this is a copy of it, kept only after its signature checked out.
 *
 * WHAT A BUTTON CAN HONESTLY SAY
 *
 * 已发出, never 已按下. The command rides the one-way beacon, so this phone knows
 * it transmitted and nothing else -- whether a key was actually pressed is the
 * Mac's to report, on the Mac. Saying otherwise here would be the screen
 * inventing the half of the story it cannot see (ui-conventions 1.1).
 */
fun buildControlScreen(context: Context, nav: Nav, console: ConsoleServer): ScreenView {
    val pal = ReposeTheme.of(context)
    lateinit var status: TextView
    var catalogue = console.received ?: ConsoleCatalogue.load(context)

    // Which computer these buttons belong to. With two Macs paired, a screen
    // full of buttons that names none of them is asking you to guess which
    // machine you are about to type into.
    val macName = catalogue?.keyId?.takeIf { it != 0 }?.let { id ->
        AppStore(context).pairedMacs(context).firstOrNull { it.keyId == id }?.name
    }

    // NO HERO ON THIS SCREEN
    //
    // A hero is an answer to 「这是什么」, and it earns its height on a screen
    // you open once. This one you open to press a button, and a 900px glyph
    // above a sync card meant the first button started below the fold — on the
    // one screen in the app where the buttons ARE the screen.
    val root = screenScaffold(
        context, pal,
        title = macName ?: "能按的键",
        // Said before the press, not after: bringing the app to the front is a
        // visible thing that happens to the Mac -- whatever was there goes
        // behind it. Finding that out by pressing is being surprised by your
        // own tool.
        lead = "按下去，那台 Mac 会先切到这个 App，再按键。",
    ) { column ->

        // One line and one button, side by side. Syncing is something you do
        // when the Mac's list changed; it does not deserve a card of its own
        // above the thing you came here for.
        val syncRow = LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        status = Ui.secondary(context, pal, "")
        syncRow.addView(status, LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f))
        syncRow.addView(
            // 「向 Mac 要一份」 described the mechanism. What the reader wants
            // is the outcome: these buttons come from the Mac, and this makes
            // them match what is on the Mac now (ui-conventions 3.4).
            // Sets no text itself. refresh() reads console.syncing -- the
            // receiving window actually being open -- and notifying is what
            // gets refresh() called; writing a line here instead meant the
            // next state event overwrote it and the button looked inert.
            Ui.ghostButton(context, pal, "同步一下") {
                console.request()
                SpikeState.notifyListeners()
            },
            Ui.lp(width = WRAP_CONTENT, left = context.dp(12)),
        )
        column.addView(syncRow, Ui.lp(top = context.dp(10)))

        val cat = catalogue
        // What this phone chose to show, out of what the Mac sent. Purely local:
        // it never goes back, never touches the signed catalogue, never changes
        // a cmd byte.
        val apps = cat?.let { ConsoleArrangement.arrange(it, ConsoleArrangement.load(context)) }.orEmpty()
        if (cat == null || cat.apps.isEmpty()) {
            // 6.4: an empty state answers what this is, not just offers a button.
            column.addView(
                Ui.infoNote(
                    context,
                    pal,
                    "还没有按钮。它们在 Mac 的「快捷键设置」里配，同步过来就出现在这里。",
                ),
                Ui.lp(top = context.dp(14)),
            )
        } else if (apps.isEmpty()) {
            // Not the same empty as "never synced", and it must not read like
            // one -- the buttons are there, this phone put them away.
            column.addView(
                Ui.infoNote(
                    context,
                    pal,
                    "${cat.apps.sumOf { it.actions.size }} 个操作都被你隐藏了。",
                ),
                Ui.lp(top = context.dp(14)),
            )
        } else {
            // ONE APP AT A TIME
            //
            // Stacked flat, the three apps on this desk came to 87 buttons --
            // longer than the phone's own app drawer, and every one of them a
            // key that gets pressed on a computer. You pick the app first
            // because that is how you already think about it: 「在终端里」,
            // then which shortcut.
            val chips = LinearLayout(context).apply { orientation = LinearLayout.HORIZONTAL }
            val holder = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
            val chipViews = ArrayList<TextView>(apps.size)

            fun show(index: Int) {
                holder.removeAllViews()
                chipViews.forEachIndexed { i, chip -> styleChip(context, pal, chip, i == index) }
                // No heading on the card: the selected chip already says which
                // app this is, and repeating it costs a row on every switch.
                val card = Ui.card(context, pal)
                apps[index].actions.forEachIndexed { i, action ->
                    card.addView(
                        actionRow(context, pal, action),
                        Ui.lp(top = if (i == 0) 0 else context.dp(10)),
                    )
                }
                holder.addView(card)
            }

            apps.forEachIndexed { i, app ->
                val chip = TextView(context).apply {
                    text = "${app.name}  ${app.actions.size}"
                    setTextSize(TypedValue.COMPLEX_UNIT_SP, 13f)
                    setPadding(context.dp(14), context.dp(8), context.dp(14), context.dp(8))
                    setOnClickListener { show(i) }
                }
                chipViews += chip
                chips.addView(chip, Ui.lp(width = WRAP_CONTENT, left = if (i == 0) 0 else context.dp(8)))
            }
            column.addView(
                HorizontalScrollView(context).apply {
                    isHorizontalScrollBarEnabled = false
                    addView(chips)
                },
                Ui.lp(top = context.dp(16)),
            )
            column.addView(holder, Ui.lp(top = context.dp(12)))
            show(0)
        }

        // Follows the thing it acts on rather than living in a settings page:
        // it changes what is on THIS screen, and nothing else anywhere.
        if (cat != null && cat.apps.isNotEmpty()) {
            column.addView(
                Ui.ghostButton(context, pal, "⚙︎  挑选与排序") { nav.go(Screen.ARRANGE) },
                Ui.lp(top = context.dp(14)),
            )
        }

        column.addView(
            Ui.amberNote(
                context,
                pal,
                "点下去只代表发出了。有没有按成要在 Mac 上看——太远、钥匙关着、" +
                    "或者没给「辅助功能」权限，都会按不了。",
            ),
            Ui.lp(top = context.dp(16)),
        )
    }

    val renderedCount = catalogue?.apps?.sumOf { it.actions.size } ?: 0

    fun refresh() {
        catalogue = console.received ?: ConsoleCatalogue.load(context)
        val n = catalogue?.apps?.sumOf { it.actions.size } ?: 0
        // A catalogue that arrives while this screen is open changes what the
        // screen IS, not just what its status line says. Updating the text and
        // leaving 「还没有列表」 under it -- with no buttons -- reads as the fetch
        // having failed (ui-conventions 2.3).
        if (n != renderedCount) {
            nav.go(Screen.CONTROL)
            return
        }
        status.text = when {
            console.lastError != null -> console.lastError!!
            console.syncing -> "正在同步。Mac 要在附近，而且电脑上开着 Outsie。"
            n > 0 -> "已经有 $n 个操作。在 Mac 上改了配置，就再同步一次。"
            else -> "还没同步过。"
        }
    }
    refresh()

    return ScreenView(root, onState = { refresh() })
}

/** Selected or not, in one place, so the two states cannot drift apart. */
private fun styleChip(context: Context, pal: Palette, chip: TextView, selected: Boolean) {
    chip.background = Ui.rounded(if (selected) pal.accentSoft else pal.surfaceMuted, context.dpF(999f))
    chip.setTextColor(if (selected) pal.textPrimary else pal.textSecondary)
    chip.typeface = Typeface.create(
        if (selected) "sans-serif-medium" else "sans-serif",
        Typeface.NORMAL,
    )
}

/** One button: what it does, which keys it is, and a press that only claims to send. */
private fun actionRow(context: Context, pal: Palette, action: ConsoleAction): LinearLayout =
    LinearLayout(context).apply {
        orientation = LinearLayout.HORIZONTAL
        gravity = Gravity.CENTER_VERTICAL
        addView(
            LinearLayout(context).apply {
                orientation = LinearLayout.VERTICAL
                addView(
                    TextView(context).apply {
                        text = listOfNotNull(action.icon, action.name).joinToString("  ")
                        setTextColor(pal.textPrimary)
                        setTextSize(TypedValue.COMPLEX_UNIT_SP, 16f)
                        typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
                    },
                )
                if (action.keys.isNotBlank()) {
                    addView(
                        Ui.secondary(context, pal, action.keys),
                        Ui.lp(top = context.dp(2)),
                    )
                }
            },
            LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f),
        )
        addView(
            Ui.primaryButton(context, pal, "按一下") {
                val queued = BleSpikeService.postCommand(context, action.cmdByte)
                Toast.makeText(
                    context,
                    when {
                        !queued -> "还没有配对，Mac 不会接受。"
                        !SpikeState.serviceRunning -> "手机钥匙关着，先去主屏打开。"
                        // 已发出, not 已按下 -- see the note at the top.
                        else -> "已发出。"
                    },
                    Toast.LENGTH_LONG,
                ).show()
            },
            Ui.lp(width = WRAP_CONTENT, left = context.dp(12)),
        )
        layoutParams = LinearLayout.LayoutParams(MATCH_PARENT, WRAP_CONTENT)
    }
