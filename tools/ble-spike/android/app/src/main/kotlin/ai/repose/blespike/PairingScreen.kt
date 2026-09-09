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
 * Screen 1 — 配对确认. Shows the pairing code the Mac must echo. No real crypto tonight:
 * confirming just flips the local "paired" flag and drops the user onto the guarding home.
 */
fun buildPairingScreen(context: Context, store: AppStore, nav: Nav): ScreenView {
    val pal = ReposeTheme.of(context)

    val root = screenScaffold(
        context = context,
        pal = pal,
        title = "确认这是同一台设备",
    ) { column ->
        column.addView(
            Ui.body(context, pal, "Mac 上现在应当显示下面这串。不一致就别继续。"),
            Ui.lp(top = context.dp(10)),
        )

        // The code — the single most important thing on the screen.
        val codeCard = Ui.card(context, pal).apply {
            gravity = Gravity.CENTER_HORIZONTAL
        }
        codeCard.addView(
            Ui.secondary(context, pal, "配对码").apply { gravity = Gravity.CENTER },
            Ui.lp(width = WRAP_CONTENT),
        )
        codeCard.addView(
            TextView(context).apply {
                text = store.pairingCode
                setTextColor(pal.accent)
                setTextSize(TypedValue.COMPLEX_UNIT_SP, 38f)
                typeface = Typeface.create("monospace", Typeface.BOLD)
                gravity = Gravity.CENTER
                letterSpacing = 0.28f
                maxLines = 1
            },
            Ui.lp(width = WRAP_CONTENT, top = context.dp(8)),
        )
        column.addView(codeCard, Ui.lp(top = context.dp(18)))

        // Connection status line.
        val statusRow = LinearLayout(context).apply {
            orientation = LinearLayout.HORIZONTAL
            gravity = Gravity.CENTER_VERTICAL
        }
        statusRow.addView(
            TextView(context).apply {
                text = "●"
                setTextColor(pal.accent)
                textSize = 12f
            },
            Ui.lp(width = WRAP_CONTENT, right = context.dp(8)),
        )
        statusRow.addView(Ui.secondary(context, pal, "已连上 MacBook Pro（工作）"), Ui.lp(width = WRAP_CONTENT))
        column.addView(statusRow, Ui.lp(top = context.dp(16)))

        column.addView(
            Ui.infoNote(
                context,
                pal,
                "Repose 不用蓝牙地址认设备——地址每几分钟自己变一次。认的是配对时交换的那把钥匙。",
            ),
            Ui.lp(top = context.dp(18)),
        )

        column.addView(
            Ui.primaryButton(context, pal, "一致，完成配对") {
                store.paired = true
                nav.go(Screen.HOME)
            },
            Ui.lp(top = context.dp(24)),
        )
        column.addView(
            Ui.ghostButton(context, pal, "不一致，取消") {
                Toast.makeText(context, "已取消。两串不一致时，别完成配对。", Toast.LENGTH_LONG).show()
            },
            Ui.lp(top = context.dp(12)),
        )
    }

    return ScreenView(root)
}
