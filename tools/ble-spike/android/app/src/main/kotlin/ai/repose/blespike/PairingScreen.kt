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
 * Screen 1 — 配对确认.
 *
 * The intended flow: the Mac and the phone exchange a key, both show the same
 * short code derived from it, and the user confirms they match. That is what
 * defeats an impersonator, because a device that never paired cannot produce
 * the key.
 *
 * NONE OF THAT IS IMPLEMENTED YET. Confirming flips a local flag. The code is
 * generated on the phone and never leaves it, so the Mac cannot be showing the
 * same one. See docs/validation/2026-09-09-e13-no-device-identity.md.
 *
 * The copy below used to describe the intended flow as though it were real --
 * telling the user their device is identified by an exchanged key, and that the
 * Mac was displaying this code. Both were false. A screen that makes a security
 * claim its code does not implement is the same failure this project has spent
 * days correcting in its own documents, except aimed at the user. The text now
 * says what actually happens, and the banner says it first.
 */
fun buildPairingScreen(context: Context, store: AppStore, nav: Nav): ScreenView {
    val pal = ReposeTheme.of(context)

    val root = screenScaffold(
        context = context,
        pal = pal,
        title = "确认这是同一台设备",
    ) { column ->
        column.addView(
            Ui.amberNote(
                context,
                pal,
                "开发预览：密钥交换尚未实现。这一步目前只是记下「已配对」，" +
                    "不校验任何东西，也挡不住冒充设备。",
            ),
            Ui.lp(top = context.dp(10)),
        )

        column.addView(
            Ui.body(context, pal, "下面这串码由手机本机生成，Mac 还看不到它。"),
            Ui.lp(top = context.dp(14)),
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
        // Seeded sample data, not a real connection. Labelled as such rather than
        // left to read as a live status line.
        statusRow.addView(
            Ui.secondary(context, pal, "MacBook Pro（工作）· 示例数据"),
            Ui.lp(width = WRAP_CONTENT),
        )
        column.addView(statusRow, Ui.lp(top = context.dp(16)))

        column.addView(
            Ui.infoNote(
                context,
                pal,
                "设计目标：不用蓝牙地址认设备（地址每几分钟自己变一次），改用配对时交换的密钥。" +
                    "目前尚未实现，所以在场判定还认不出「是不是这台手机」。",
            ),
            Ui.lp(top = context.dp(18)),
        )

        column.addView(
            Ui.primaryButton(context, pal, "继续（不校验）") {
                store.paired = true
                nav.go(Screen.HOME)
            },
            Ui.lp(top = context.dp(24)),
        )
        column.addView(
            Ui.ghostButton(context, pal, "取消") {
                Toast.makeText(context, "已取消。", Toast.LENGTH_LONG).show()
            },
            Ui.lp(top = context.dp(12)),
        )
    }

    return ScreenView(root)
}
