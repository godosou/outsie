package ai.repose.blespike

import android.app.AlertDialog
import android.content.Context
import android.graphics.Typeface
import android.util.TypedValue
import android.view.Gravity
import android.view.ViewGroup.LayoutParams.WRAP_CONTENT
import android.widget.TextView
import android.widget.Toast

/**
 * Screen 4 — 这把钥匙.
 *
 * It used to be a list of Macs this phone could unlock, with a 停用 button on each and
 * a note promising that a lost phone could be shut off remotely by signing in on
 * another device. Three problems, all the same shape:
 *
 *   - the list was seeded placeholders; no pairing store exists, so the phone has
 *     never known of any Mac
 *   - 停用 flipped a local boolean that nothing reads, so it stopped nothing
 *   - remote deactivation is not built at all, and a person who has just lost their
 *     phone is the worst possible audience for a safeguard that does not exist
 *
 * What this phone does have is one genuine kill switch. The Mac accepts a beacon only
 * if it carries a tag minted with the shared key; delete the key and this phone cannot
 * produce one, so every Mac refuses it from the next advertisement onward. That is
 * local, immediate, and real, so the screen is built around it.
 */
fun buildMacsScreen(context: Context, store: AppStore, nav: Nav): ScreenView {
    val pal = ReposeTheme.of(context)
    val provisioned = PresenceKey.has(SpikeContract.PRESENCE_KEY_ID)
    val fingerprint = PresenceKey.fingerprint(context)

    val root = screenScaffold(
        context = context,
        pal = pal,
        title = "这把钥匙",
        lead = if (provisioned) {
            "任何配了同一把密钥的 Mac 都认这台手机。"
        } else {
            "还没有密钥，所以还不是任何 Mac 的钥匙。"
        },
    ) { column ->

        if (provisioned) {
            val card = Ui.card(context, pal)
            card.addView(Ui.secondary(context, pal, "密钥指纹"))
            card.addView(
                TextView(context).apply {
                    text = fingerprint ?: "????????"
                    setTextColor(pal.accent)
                    setTextSize(TypedValue.COMPLEX_UNIT_SP, 30f)
                    typeface = Typeface.create("monospace", Typeface.BOLD)
                    letterSpacing = 0.2f
                    maxLines = 1
                },
                Ui.lp(top = context.dp(8)),
            )
            card.addView(
                Ui.secondary(context, pal, "配对完成时 Mac 上会打印同一串；也可以用 pair.sh 重新配一次核对。"),
                Ui.lp(top = context.dp(10)),
            )
            column.addView(card, Ui.lp(top = context.dp(20)))

            column.addView(
                Ui.infoNote(
                    context,
                    pal,
                    "这台手机不知道有几台 Mac 配了这把密钥——信标是单向广播，没有 Mac 会回话。" +
                        "要看某一台 Mac 的状态，去那台 Mac 上的 Repose。",
                ),
                Ui.lp(top = context.dp(18)),
            )

            column.addView(
                Ui.amberNote(
                    context,
                    pal,
                    "手机丢了怎么办：删掉下面这把密钥，这台手机立刻算不出有效标签，所有 Mac 都会拒绝它。" +
                        "但这需要你能拿到这台手机——远程停用还没有做。人不在手机旁边时，" +
                        "该做的是去 Mac 上关掉手机钥匙。",
                ),
                Ui.lp(top = context.dp(18)),
            )

            // The only way back to the pairing screen once a key exists.
            // MainActivity sends a paired phone straight to HOME and the bottom
            // nav has no PAIRING tab, so without this the exchange is reachable
            // exactly once -- and replacing a key would mean deleting it first
            // and hoping.
            column.addView(
                Ui.primaryButton(context, pal, "重新配对") { nav.go(Screen.PAIRING) },
                Ui.lp(top = context.dp(22)),
            )
            column.addView(
                Ui.ghostButton(context, pal, "删除密钥（立即失效）") {
                    AlertDialog.Builder(context, Ui.dialogTheme(context))
                        .setTitle("删除这把密钥？")
                        .setMessage(
                            "删除后这台手机会继续广播，但标签不再有效，任何 Mac 都会拒绝它。" +
                                "要重新可用，需要在 Mac 上再下发一把。",
                        )
                        .setPositiveButton("删除") { _, _ ->
                            PresenceKey.delete(context, SpikeContract.PRESENCE_KEY_ID)
                            store.paired = false
                            Toast.makeText(context, "已删除。下一个广播窗口起就会被拒绝。", Toast.LENGTH_LONG).show()
                            nav.go(Screen.MACS)
                        }
                        .setNegativeButton("取消", null)
                        .show()
                },
                Ui.lp(top = context.dp(22)),
            )
        } else {
            column.addView(
                Ui.body(
                    context,
                    pal,
                    "信标照常在广播，但标签是用一把随机数临时凑出来的，任何 Mac 都会拒绝。" +
                        "去配对：两端各显示一串六位数字，核对一致就成。",
                ),
                Ui.lp(top = context.dp(20)),
            )
            column.addView(
                Ui.primaryButton(context, pal, "去看密钥状态") { nav.go(Screen.PAIRING) },
                Ui.lp(top = context.dp(22)),
            )
        }
    }

    return ScreenView(root)
}
