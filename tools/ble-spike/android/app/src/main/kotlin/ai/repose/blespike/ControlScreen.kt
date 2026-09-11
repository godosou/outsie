package ai.repose.blespike

import android.content.Context
import android.graphics.Typeface
import android.util.TypedValue
import android.view.Gravity
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
import android.view.ViewGroup.LayoutParams.WRAP_CONTENT
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

    val root = screenScaffold(context, pal, title = "", showTitle = false) { column ->
        column.addView(
            heroCard(
                context, pal,
                chip = "从这里按",
                glyph = "🎛",
                headline = "这台 Mac 能按的键",
                body = "列表是 Mac 给的，改要去 Mac 上改。",
            ),
            Ui.lp(top = context.dp(6)),
        )

        val statusCard = sectionCard(context, pal, "📡", "取列表")
        status = Ui.secondary(context, pal, "")
        statusCard.addView(status, Ui.lp(top = context.dp(12)))
        statusCard.addView(
            Ui.ghostButton(context, pal, "向 Mac 要一份") {
                if (console.request()) {
                    status.text = "正在等 Mac 送过来。它要在附近，而且电脑上那一页要打开着。"
                } else {
                    status.text = console.lastError ?: "没能开始"
                }
            },
            Ui.lp(top = context.dp(12)),
        )
        column.addView(statusCard, Ui.lp(top = context.dp(14)))

        val cat = catalogue
        if (cat == null || cat.apps.isEmpty()) {
            // 6.4: an empty state answers what this is, not just offers a button.
            column.addView(
                Ui.infoNote(
                    context,
                    pal,
                    "还没有列表。列表是那台 Mac 上「快捷控制」里配好的操作——" +
                        "取过来之后，这里会出现对应的按钮，点一下 Mac 就按下那组键。",
                ),
                Ui.lp(top = context.dp(14)),
            )
        } else {
            for (app in cat.apps) {
                val card = sectionCard(context, pal, "💻", app.name)
                for (action in app.actions) {
                    card.addView(
                        actionRow(context, pal, action),
                        Ui.lp(top = context.dp(10)),
                    )
                }
                column.addView(card, Ui.lp(top = context.dp(14)))
            }
        }

        column.addView(
            Ui.amberNote(
                context,
                pal,
                "点下去只代表发出了。Mac 有没有真的按下，要在那台 Mac 上看——" +
                    "它离得太远、钥匙关着、或者没给「辅助功能」权限时，都会收不到或者按不了。",
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
            n > 0 -> "已经有 $n 个操作。改了 Mac 上的配置就再取一次。"
            else -> "还没取过。"
        }
    }
    refresh()

    return ScreenView(root, onState = { refresh() })
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
                        !queued -> "还没有配对，Mac 不会接受这条指令。"
                        !SpikeState.serviceRunning -> "手机钥匙是关着的，先打开主屏上的开关。"
                        // 已发出, not 已按下 -- see the note at the top.
                        else -> "已发出。Mac 在附近的话，几秒内就会按下。"
                    },
                    Toast.LENGTH_LONG,
                ).show()
            },
            Ui.lp(width = WRAP_CONTENT, left = context.dp(12)),
        )
        layoutParams = LinearLayout.LayoutParams(MATCH_PARENT, WRAP_CONTENT)
    }
