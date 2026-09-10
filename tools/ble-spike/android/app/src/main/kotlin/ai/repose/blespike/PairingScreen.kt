package ai.repose.blespike

import android.content.Context
import android.graphics.Typeface
import android.util.TypedValue
import android.view.Gravity
import android.view.ViewGroup.LayoutParams.WRAP_CONTENT
import android.widget.TextView
import android.widget.Toast

/**
 * Screen 1 — 密钥状态.
 *
 * This screen used to show a locally-generated code and tell the user the Mac was
 * displaying the same one and that a key identified this device. All of it was false:
 * confirming set a boolean. It was rewritten once to say so plainly, and is rewritten
 * again here now that a key actually exists.
 *
 * What it shows now is checkable. The fingerprint is a hash of the real presence key
 * `K` held in the Keystore; the Mac's provisioning tool prints the same eight
 * characters for the same key. Matching them is a genuine (if manual) confirmation
 * that both ends hold one key. Not matching means the phone is broadcasting tags this
 * Mac will reject.
 *
 * What it still is NOT: pairing. `K` arrives over a USB development channel, which
 * assumes whoever holds the cable is the owner. The MITM-resistant SAS exchange in
 * the design (§1) is not built. The banner says which of the two you are looking at,
 * because the distance between "a key exists" and "the key was established safely" is
 * exactly where this project's earlier claims went wrong.
 */
fun buildPairingScreen(context: Context, store: AppStore, nav: Nav): ScreenView {
    val pal = ReposeTheme.of(context)
    val provisioned = PresenceKey.has(SpikeContract.PRESENCE_KEY_ID)
    val fingerprint = PresenceKey.fingerprint(context)

    val root = screenScaffold(
        context = context,
        pal = pal,
        title = if (provisioned) "这台手机已有在场密钥" else "这台手机还没有在场密钥",
    ) { column ->
        column.addView(
            Ui.amberNote(
                context,
                pal,
                "开发预览：密钥通过 USB 直接写入，等于默认「拿着线的人就是机主」。" +
                    "带防中间人校验的正式配对（两端比对 6 位数字）尚未实现。",
            ),
            Ui.lp(top = context.dp(10)),
        )

        if (provisioned) {
            column.addView(
                Ui.body(context, pal, "把下面这串和 Mac 上 provision-dev-key.sh 打印的对一下。不一致就说明不是同一把钥匙。"),
                Ui.lp(top = context.dp(14)),
            )

            val codeCard = Ui.card(context, pal).apply { gravity = Gravity.CENTER_HORIZONTAL }
            codeCard.addView(
                Ui.secondary(context, pal, "密钥指纹").apply { gravity = Gravity.CENTER },
                Ui.lp(width = WRAP_CONTENT),
            )
            codeCard.addView(
                TextView(context).apply {
                    text = fingerprint ?: "????????"
                    setTextColor(pal.accent)
                    setTextSize(TypedValue.COMPLEX_UNIT_SP, 38f)
                    typeface = Typeface.create("monospace", Typeface.BOLD)
                    gravity = Gravity.CENTER
                    letterSpacing = 0.24f
                    maxLines = 1
                },
                Ui.lp(width = WRAP_CONTENT, top = context.dp(8)),
            )
            column.addView(codeCard, Ui.lp(top = context.dp(18)))

            column.addView(
                Ui.infoNote(
                    context,
                    pal,
                    "指纹是 K 的哈希，不是 K 的一部分——它可以给别人看，密钥本身不可导出，" +
                        "只能在安全芯片里参与签名。",
                ),
                Ui.lp(top = context.dp(18)),
            )

            column.addView(
                Ui.primaryButton(context, pal, "继续") {
                    store.paired = true
                    nav.go(Screen.HOME)
                },
                Ui.lp(top = context.dp(24)),
            )
            column.addView(
                Ui.ghostButton(context, pal, "删除这把密钥") {
                    PresenceKey.delete(context, SpikeContract.PRESENCE_KEY_ID)
                    store.paired = false
                    Toast.makeText(context, "已删除。信标将开始广播无效标签。", Toast.LENGTH_LONG).show()
                    nav.go(Screen.PAIRING)
                },
                Ui.lp(top = context.dp(12)),
            )
        } else {
            column.addView(
                Ui.body(
                    context,
                    pal,
                    "现在信标照常广播，但标签是用一把随机数临时凑出来的——Mac 会看见这台手机，" +
                        "然后拒绝它。这就是没有密钥时应有的样子。",
                ),
                Ui.lp(top = context.dp(14)),
            )
            column.addView(
                Ui.infoNote(
                    context,
                    pal,
                    "在 Mac 上执行：\n" +
                        "tools/ble-spike/provision-dev-key.sh\n\n" +
                        "它会生成一把 K，写进 Mac 的 root 专有文件，推送到这台手机，" +
                        "然后重启信标。完成后回到这个界面会看到指纹。",
                ),
                Ui.lp(top = context.dp(18)),
            )
            column.addView(
                Ui.primaryButton(context, pal, "重新检查") { nav.go(Screen.PAIRING) },
                Ui.lp(top = context.dp(24)),
            )
            column.addView(
                Ui.ghostButton(context, pal, "先跳过") { nav.go(Screen.HOME) },
                Ui.lp(top = context.dp(12)),
            )
        }
    }

    return ScreenView(root)
}
