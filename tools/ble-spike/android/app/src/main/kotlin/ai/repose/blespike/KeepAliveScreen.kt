package ai.repose.blespike

import android.app.AlertDialog
import android.content.Context
import android.graphics.Typeface
import android.util.TypedValue
import android.view.Gravity
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
import android.view.ViewGroup.LayoutParams.WRAP_CONTENT
import android.widget.ImageView
import android.widget.LinearLayout
import android.widget.TextView

/**
 * 让它一直在 — one page, one step at a time, with the real settings screens
 * pictured (design doc §04 手机·主屏). Reached from the row under the key
 * switch; the home screen itself carries nothing but that row.
 */
fun buildKeepAliveScreen(context: Context, store: AppStore, nav: Nav, onRequestPermissions: () -> Unit): ScreenView {
    val pal = ReposeTheme.of(context)
    val keep = keepAliveFacts(context, store)
    val root = screenScaffold(
        context, pal,
        title = "让它一直在",
        lead = "息屏、锁屏、放进口袋，钥匙都要在。这几件事这部手机要允许一次，做过就不用再管。",
        onBack = { nav.go(Screen.HOME) },
    ) { column ->
        KeepAlive.items(keep).forEachIndexed { index, item ->
            val card = Ui.card(context, pal)
            card.addView(TextView(context).apply {
                text = "${index + 1} · ${item.title}" + if (item.done) "　✓" else ""
                setTextColor(if (item.done) pal.textSecondary else pal.textPrimary)
                setTextSize(TypedValue.COMPLEX_UNIT_SP, 16f)
                typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
            })
            card.addView(Ui.secondary(context, pal, item.gives), Ui.lp(top = context.dp(6)))
            if (!item.done) {
                for ((name, caption) in KeepAlive.pictures(item.step, keep.manufacturer)) {
                    val id = context.resources.getIdentifier(name, "drawable", context.packageName)
                    if (id != 0) {
                        card.addView(ImageView(context).apply {
                            setImageResource(id)
                            adjustViewBounds = true
                            scaleType = ImageView.ScaleType.FIT_CENTER
                            background = Ui.rounded(pal.surfaceMuted, context.dpF(12f))
                            clipToOutline = true
                        }, Ui.lp(top = context.dp(12), height = context.dp(300)))
                        card.addView(Ui.secondary(context, pal, caption).apply { gravity = Gravity.CENTER_HORIZONTAL }, Ui.lp(top = context.dp(6)))
                    }
                }
                if (item.step == KeepStep.BACKGROUND) {
                    card.addView(Ui.infoNote(context, pal, KeepAlive.backgroundGuide(keep.manufacturer)), Ui.lp(top = context.dp(12)))
                }
                val row = LinearLayout(context).apply { orientation = LinearLayout.HORIZONTAL }
                row.addView(Ui.primaryButton(context, pal, item.action) {
                    when (item.step) {
                        KeepStep.BLUETOOTH, KeepStep.NOTIFICATIONS -> onRequestPermissions()
                        KeepStep.BATTERY -> requestBatteryExempt(context)
                        KeepStep.BACKGROUND -> openAppSettings(context)
                    }
                }, LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f))
                if (item.step == KeepStep.BACKGROUND) {
                    row.addView(Ui.ghostButton(context, pal, "我做好了") {
                        store.keepAliveBackgroundAcknowledged = true
                        nav.go(Screen.KEEPALIVE)
                    }, LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f).also { it.leftMargin = context.dp(10) })
                }
                card.addView(row, Ui.lp(top = context.dp(14)))
            }
            column.addView(card, Ui.lp(top = context.dp(if (index == 0) 4 else 12)))
        }
        column.addView(
            Ui.secondary(context, pal, "设好之后回到主屏，那一行会写「都设好了」。哪天钥匙又不灵了，先回来看这一页。"),
            Ui.lp(top = context.dp(16), left = context.dp(4), right = context.dp(4)),
        )
    }
    return ScreenView(root, onState = {})
}
