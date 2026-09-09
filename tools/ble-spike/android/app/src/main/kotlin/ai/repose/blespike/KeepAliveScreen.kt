package ai.repose.blespike

import android.app.AlertDialog
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.Settings
import android.util.TypedValue
import android.view.Gravity
import android.view.ViewGroup.LayoutParams.WRAP_CONTENT
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast

/**
 * Screen 3 — 保持后台. Explains why Repose must keep running and wires the two rows to the
 * real Android settings intents, with graceful fallbacks if a device lacks the screen.
 */
fun buildKeepAliveScreen(context: Context, nav: Nav): ScreenView {
    val pal = ReposeTheme.of(context)

    val root = screenScaffold(
        context = context,
        pal = pal,
        title = "让 Repose 一直醒着",
        lead = "锁屏时也要能被 Mac 认出，得允许它后台运行、不被省电策略冻结。",
        onBack = { nav.back() },
    ) { column ->

        val card = Ui.card(context, pal)

        card.addView(
            actionRow(
                context, pal,
                title = "关闭电池优化",
                action = "去设置",
            ) { openBatteryOptimization(context) },
        )
        card.addView(
            Ui.divider(context, pal),
            Ui.lp(height = context.dp(1), top = context.dp(14), bottom = context.dp(14)),
        )
        card.addView(
            actionRow(
                context, pal,
                title = "锁定在后台",
                action = "怎么做",
            ) { showLockInstructions(context, pal) },
        )
        column.addView(card, Ui.lp(top = context.dp(20)))

        column.addView(
            Ui.amberNote(
                context,
                pal,
                "没做这一步，手机进入深度休眠后 Mac 会认不出它——那时回车会失败，改用密码。",
            ),
            Ui.lp(top = context.dp(18)),
        )

        column.addView(
            Ui.primaryButton(context, pal, "打开系统设置") { openAppDetails(context) },
            Ui.lp(top = context.dp(22)),
        )
    }

    return ScreenView(root)
}

private fun actionRow(
    context: Context,
    pal: Palette,
    title: CharSequence,
    action: CharSequence,
    onClick: () -> Unit,
): LinearLayout = LinearLayout(context).apply {
    orientation = LinearLayout.HORIZONTAL
    gravity = Gravity.CENTER_VERTICAL
    minimumHeight = context.dp(48)
    addView(Ui.heading(context, pal, title), LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f))
    addView(
        TextView(context).apply {
            text = "$action ›"
            setTextColor(pal.accent)
            setTextSize(TypedValue.COMPLEX_UNIT_SP, 15f)
            typeface = android.graphics.Typeface.create("sans-serif-medium", android.graphics.Typeface.NORMAL)
            setPadding(context.dp(8), context.dp(8), context.dp(4), context.dp(8))
            isClickable = true
            isFocusable = true
            setOnClickListener { onClick() }
        },
        Ui.lp(width = WRAP_CONTENT),
    )
}

private fun openBatteryOptimization(context: Context) {
    // The system-wide "ignore battery optimizations" list. Falls back to app details.
    val intent = Intent(Settings.ACTION_IGNORE_BATTERY_OPTIMIZATION_SETTINGS)
    try {
        context.startActivity(intent)
    } catch (_: ActivityNotFoundException) {
        openAppDetails(context)
    }
}

private fun openAppDetails(context: Context) {
    val intent = Intent(
        Settings.ACTION_APPLICATION_DETAILS_SETTINGS,
        Uri.fromParts("package", context.packageName, null),
    )
    try {
        context.startActivity(intent)
    } catch (_: ActivityNotFoundException) {
        Toast.makeText(context, "这台设备找不到应用设置页。", Toast.LENGTH_SHORT).show()
    }
}

private fun showLockInstructions(context: Context, pal: Palette) {
    AlertDialog.Builder(context, Ui.dialogTheme(context))
        .setTitle("锁定在后台")
        .setMessage(
            "打开最近任务（多任务）界面，找到 Repose 的卡片，" +
                "点卡片顶部的图标或长按，选择「锁定」或「加锁」。" +
                "锁定后，一键清理不会把 Repose 划掉。\n\n" +
                "不同手机叫法略有不同——小米叫「锁定」，华为叫「加锁」，OPPO/realme 叫「锁定后台」。",
        )
        .setPositiveButton("知道了", null)
        .show()
}
