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
        title = "",
        showTitle = false,
    ) { column ->

        if (provisioned) {
            column.addView(
                heroCard(
                    context, pal,
                    chip = "这把钥匙",
                    glyph = "🔑",
                    headline = "已经配对好了",
                    body = "配对过的 Mac 认得这台手机。",
                ),
                Ui.lp(top = context.dp(6)),
            )

            val card = sectionCard(context, pal, "🔖", "配对编号")
            card.addView(
                TextView(context).apply {
                    text = fingerprint ?: "????????"
                    setTextColor(pal.accent)
                    setTextSize(TypedValue.COMPLEX_UNIT_SP, 32f)
                    typeface = Typeface.create("monospace", Typeface.BOLD)
                    letterSpacing = 0.2f
                    maxLines = 1
                },
                Ui.lp(top = context.dp(12)),
            )
            card.addView(
                Ui.secondary(context, pal, "Mac 上显示的应该是同一串。"),
                Ui.lp(top = context.dp(8)),
            )
            column.addView(card, Ui.lp(top = context.dp(14)))

            val reach = sectionCard(context, pal, "💻", "哪些 Mac 认得它")
            reach.addView(
                Ui.secondary(
                    context,
                    pal,
                    "这台手机不知道有几台 Mac 配过它 —— 它只往外发信号，收不到回音。" +
                        "要看某一台 Mac 的情况，去那台 Mac 上的 Repose。",
                ),
                Ui.lp(top = context.dp(12)),
            )
            column.addView(reach, Ui.lp(top = context.dp(14)))

            column.addView(
                Ui.amberNote(
                    context,
                    pal,
                    "手机丢了怎么办：取消配对，所有 Mac 立刻就不认它了。" +
                        "但这要你手上有这台手机 —— 远程停用还没做。人不在手机旁边时，" +
                        "去 Mac 上把手机钥匙关掉。",
                ),
                Ui.lp(top = context.dp(14)),
            )

            // The only way back to the pairing screen once a key exists.
            // MainActivity sends a paired phone straight to HOME and the bottom
            // nav has no PAIRING tab, so without this the exchange is reachable
            // exactly once -- and replacing a key would mean deleting it first
            // and hoping.
            column.addView(
                Ui.primaryButton(context, pal, "重新配对") { nav.go(Screen.PAIRING) },
                Ui.lp(top = context.dp(20)),
            )
            column.addView(
                Ui.ghostButton(context, pal, "取消配对（立即生效）") {
                    AlertDialog.Builder(context, Ui.dialogTheme(context))
                        .setTitle("取消配对？")
                        .setMessage(
                            "取消后，所有 Mac 立刻就认不出这台手机了。" +
                                "想再用，重新配对一次就行。",
                        )
                        .setPositiveButton("取消配对") { _, _ ->
                            PresenceKey.delete(context, SpikeContract.PRESENCE_KEY_ID)
                            store.paired = false
                            Toast.makeText(context, "已取消配对。", Toast.LENGTH_LONG).show()
                            nav.go(Screen.MACS)
                        }
                        .setNegativeButton("取消", null)
                        .show()
                },
                Ui.lp(top = context.dp(10)),
            )
        } else {
            column.addView(
                heroCard(
                    context, pal,
                    chip = "这把钥匙",
                    glyph = "🔗",
                    headline = "还没有配对",
                    body = "所以现在还不是任何 Mac 的钥匙。",
                    muted = true,
                ),
                Ui.lp(top = context.dp(6)),
            )
            val card = sectionCard(context, pal, "🔢", "去配对")
            card.addView(
                Ui.body(
                    context,
                    pal,
                    "Mac 会看见这台手机，但认不出它是你的。",
                ),
                Ui.lp(top = context.dp(12)),
            )
            card.addView(
                Ui.secondary(
                    context,
                    pal,
                    "配一次：两边各显示一串六位数字，看一眼一样就好。",
                ),
                Ui.lp(top = context.dp(8)),
            )
            column.addView(card, Ui.lp(top = context.dp(14)))
            column.addView(
                Ui.primaryButton(context, pal, "去配对") { nav.go(Screen.PAIRING) },
                Ui.lp(top = context.dp(20)),
            )
        }
    }

    return ScreenView(root)
}
