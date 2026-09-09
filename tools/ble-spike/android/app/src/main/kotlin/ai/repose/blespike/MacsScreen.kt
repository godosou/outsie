package ai.repose.blespike

import android.app.AlertDialog
import android.content.Context
import android.util.TypedValue
import android.view.Gravity
import android.view.ViewGroup.LayoutParams.WRAP_CONTENT
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast

/**
 * Screen 4 — 我解锁的 Mac. Lists the Macs this phone is a key for, each revocable. The
 * entries are local placeholders tonight — no real pairing store yet. Revoking flips the
 * local enabled flag and rebuilds the screen so the change shows immediately.
 */
fun buildMacsScreen(context: Context, store: AppStore, nav: Nav): ScreenView {
    val pal = ReposeTheme.of(context)

    val root = screenScaffold(
        context = context,
        pal = pal,
        title = "我解锁的 Mac",
        lead = "这台手机是这些 Mac 的钥匙。",
    ) { column ->

        val card = Ui.card(context, pal)
        val macs = store.macs()
        macs.forEachIndexed { index, mac ->
            if (index > 0) {
                card.addView(
                    Ui.divider(context, pal),
                    Ui.lp(height = context.dp(1), top = context.dp(14), bottom = context.dp(14)),
                )
            }
            card.addView(macRow(context, pal, store, nav, mac))
        }
        column.addView(card, Ui.lp(top = context.dp(20)))

        column.addView(
            Ui.amberNote(
                context,
                pal,
                "丢了手机？在任意另一台设备上登录就能远程停用；停用只对选中的那台 Mac 生效，且立即生效。",
            ),
            Ui.lp(top = context.dp(18)),
        )

        column.addView(
            Ui.ghostButton(context, pal, "丢了这台手机？全部停用") {
                AlertDialog.Builder(context, Ui.dialogTheme(context))
                    .setTitle("全部停用？")
                    .setMessage("这会立即停用这台手机对所有 Mac 的解锁。之后需要重新配对才能恢复。")
                    .setPositiveButton("全部停用") { _, _ ->
                        store.disableAll()
                        Toast.makeText(context, "已全部停用，立即生效。", Toast.LENGTH_SHORT).show()
                        nav.go(Screen.MACS)
                    }
                    .setNegativeButton("取消", null)
                    .show()
            },
            Ui.lp(top = context.dp(22)),
        )
    }

    return ScreenView(root)
}

private fun macRow(
    context: Context,
    pal: Palette,
    store: AppStore,
    nav: Nav,
    mac: MacDevice,
): LinearLayout = LinearLayout(context).apply {
    orientation = LinearLayout.HORIZONTAL
    gravity = Gravity.CENTER_VERTICAL
    minimumHeight = context.dp(52)

    addView(
        glyphCircle(
            context,
            if (mac.enabled) pal.accentSoft else pal.surfaceMuted,
            "💻",
            44,
            20f,
            if (mac.enabled) pal.accent else pal.textSecondary,
        ),
        Ui.lp(width = WRAP_CONTENT, right = context.dp(14)),
    )

    val info = LinearLayout(context).apply { orientation = LinearLayout.VERTICAL }
    info.addView(
        TextView(context).apply {
            text = mac.name
            setTextColor(if (mac.enabled) pal.textPrimary else pal.textSecondary)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 16f)
            typeface = android.graphics.Typeface.create("sans-serif-medium", android.graphics.Typeface.NORMAL)
        },
    )
    info.addView(
        Ui.secondary(context, pal, if (mac.enabled) mac.lastSeen else "已停用"),
        Ui.lp(top = context.dp(3)),
    )
    addView(info, LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f))

    if (mac.enabled) {
        addView(
            TextView(context).apply {
                text = "停用"
                gravity = Gravity.CENTER
                setTextColor(pal.accent)
                setTextSize(TypedValue.COMPLEX_UNIT_SP, 15f)
                typeface = android.graphics.Typeface.create("sans-serif-medium", android.graphics.Typeface.NORMAL)
                val h = context.dp(16)
                setPadding(h, context.dp(10), h, context.dp(10))
                background = Ui.rounded(0x00000000, context.dpF(12f), pal.divider, context.dp(1))
                isClickable = true
                isFocusable = true
                setOnClickListener {
                    store.setEnabled(mac.id, false)
                    Toast.makeText(context, "已停用 ${mac.name}，立即生效。", Toast.LENGTH_SHORT).show()
                    nav.go(Screen.MACS)
                }
            },
            Ui.lp(width = WRAP_CONTENT),
        )
    } else {
        // Keep the shell usable — let a revoked placeholder be switched back on.
        addView(
            TextView(context).apply {
                text = "重新启用"
                gravity = Gravity.CENTER
                setTextColor(pal.textSecondary)
                setTextSize(TypedValue.COMPLEX_UNIT_SP, 15f)
                val h = context.dp(14)
                setPadding(h, context.dp(10), h, context.dp(10))
                isClickable = true
                isFocusable = true
                setOnClickListener {
                    store.setEnabled(mac.id, true)
                    nav.go(Screen.MACS)
                }
            },
            Ui.lp(width = WRAP_CONTENT),
        )
    }
}
