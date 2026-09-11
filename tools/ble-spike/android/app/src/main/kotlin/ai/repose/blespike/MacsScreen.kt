package ai.repose.blespike

import android.app.AlertDialog
import android.content.Context
import android.graphics.Typeface
import android.util.TypedValue
import android.view.Gravity
import android.view.ViewGroup.LayoutParams.WRAP_CONTENT
import android.widget.LinearLayout
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
    val provisioned = PresenceKey.hasAny(context)
    val fingerprint = PresenceKey.fingerprint(context)

    val root = screenScaffold(
        context = context,
        pal = pal,
        title = "",
        showTitle = false,
    ) { column ->

        if (provisioned) {
            val macName = store.pairedMac
            column.addView(
                heroCard(
                    context, pal,
                    chip = "这把钥匙",
                    glyph = "🔑",
                    headline = macName?.let { "「$it」的钥匙" } ?: "已经配对好了",
                    body = "配对过的 Mac 认得这台手机。",
                ),
                Ui.lp(top = context.dp(6)),
            )

            val card = sectionCard(context, pal, "💻", "配对的电脑")
            card.addView(
                TextView(context).apply {
                    text = macName ?: "（这台 Mac 没有报名字）"
                    setTextColor(if (macName != null) pal.textPrimary else pal.textSecondary)
                    setTextSize(TypedValue.COMPLEX_UNIT_SP, 19f)
                    typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
                },
                Ui.lp(top = context.dp(12)),
            )
            card.addView(
                Ui.secondary(context, pal, "名字是那台电脑自己报的，只是方便你认，不用拿它做核对。"),
                Ui.lp(top = context.dp(8)),
            )
            column.addView(card, Ui.lp(top = context.dp(14)))

            val reach = sectionCard(context, pal, "💻", "配过的 Mac")

            // The phone's own record, one row per key it holds.
            //
            // Three things this list must not be mistaken for, all of them said
            // on screen rather than assumed:
            //   - it is not the Macs' state; a Mac whose owner deleted the key
            //     on the Mac side cannot tell this phone, and will still be here
            //   - removing a row is not "that Mac forgets me"; it is this phone
            //     giving up the key, which is the half it actually controls
            //   - deleting the key below takes ALL of them, not just one
            val macs = store.pairedMacs(context)
            for (m in macs) {
                val row = LinearLayout(context).apply {
                    orientation = LinearLayout.HORIZONTAL
                    gravity = Gravity.CENTER_VERTICAL
                }
                row.addView(
                    LinearLayout(context).apply {
                        orientation = LinearLayout.VERTICAL
                        addView(
                            TextView(context).apply {
                                text = m.name
                                setTextColor(pal.textPrimary)
                                setTextSize(TypedValue.COMPLEX_UNIT_SP, 16f)
                                typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
                            },
                        )
                        addView(
                            Ui.secondary(
                                context,
                                pal,
                                // The id is absent until this Mac has been heard
                                // from once, and saying so beats showing a blank.
                                // Three different truths, and they are not the
                                // same one worded differently: heard and
                                // identified, held but never heard from, and a
                                // key older than this list.
                                (m.macId?.let { "编号 $it" }
                                    ?: if (m.pairedAt.isBlank()) "这条列表出现之前配的"
                                    else "还没听到它报编号") +
                                    " · 钥匙位 ${m.keyId}",
                            ),
                            Ui.lp(top = context.dp(2)),
                        )
                    },
                    LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f),
                )
                row.addView(
                    Ui.ghostButton(context, pal, "移走") {
                        AlertDialog.Builder(context)
                            .setTitle("从这台手机上移走「${m.name}」？")
                            .setMessage(
                                "这台手机会丢掉它那把钥匙，那台 Mac 就不会再因为你走近而解锁。\n\n" +
                                    "这不是在那台 Mac 上做的操作——它那边的设置不会变，" +
                                    "只是从此认不出这台手机。想恢复，重新配对一次。",
                            )
                            .setNegativeButton("算了", null)
                            .setPositiveButton("移走") { _, _ ->
                                store.forgetMac(context, m.keyId)
                                Toast.makeText(context, "已经移走「${m.name}」", Toast.LENGTH_LONG).show()
                                nav.go(Screen.MACS)
                            }
                            .show()
                    },
                    Ui.lp(width = WRAP_CONTENT, left = context.dp(12)),
                )
                reach.addView(row, Ui.lp(top = context.dp(12)))
            }


            reach.addView(
                Ui.secondary(
                    context,
                    pal,
                    // 「共用同一把钥匙」 stopped being true with pair-v3: each Mac
                    // derives its own, and the phone's keys never leave its
                    // secure element. 「取消配对」 still takes them all, because
                    // it deletes every one.
                    "这是这台手机自己的记录，不是那些 Mac 的状态——在 Mac 那边删掉钥匙，" +
                        "这里也不会变。每台 Mac 用的是各自的一把钥匙，互不相干；" +
                        "下面的「取消配对」会把它们全部删掉，所有 Mac 一起失效。" +
                        "编号对应的是哪台电脑，看那台 Mac 上「技术细节」里的同一串。",
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
            column.addView(techDetails(context, pal, fingerprint), Ui.lp(top = context.dp(14)))
            column.addView(
                // It does not re-pair; it opens the pairing screen, where the
                // first thing you see is what you are already paired with and a
                // second button also called 重新配对. Pressing a button and being
                // asked the same question is how people conclude they missed a
                // step (ui-conventions 2.5).
                Ui.primaryButton(context, pal, "看这次配对") { nav.go(Screen.PAIRING) },
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
                            // Every slot, not just the first: with two Macs paired, deleting one
                            // key would leave the phone still opening the other while the
                            // screen says the pairing is gone.
                            PresenceKey.activeIds(context).forEach { PresenceKey.delete(context, it) }
                            AppStore(context).keyIds = emptyList()
                            store.paired = false
                            // The name outliving the key would leave the screen
                            // naming a Mac this phone can no longer open.
                            store.pairedMac = null
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
