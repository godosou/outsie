package ai.repose.blespike

import android.content.Context
import android.graphics.Typeface
import android.util.TypedValue
import android.view.Gravity
import android.view.ViewGroup.LayoutParams.WRAP_CONTENT
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast

/**
 * Screen 1 — 密钥状态 / 配对.
 *
 * Three views, in priority order:
 *
 *   1. A pairing window is open  -> the six digits, or "waiting for the Mac"
 *   2. A key exists              -> its fingerprint, and a way to re-pair
 *   3. Neither                   -> start pairing
 *
 * The window beats the key on purpose. Somebody re-pairing needs the digits in
 * front of them, not the fingerprint of the key they are in the middle of
 * replacing.
 *
 * This screen has now been rewritten three times, twice for the same reason: it
 * said things the code did not do. The first showed a locally-invented code and
 * claimed the Mac was showing it too. The second said plainly that no key
 * exchange existed. One does now, and the copy has to be equally careful in the
 * other direction -- the six digits really are the whole of the MITM defence, so
 * the screen's job is to make comparing them feel like the point, not a
 * formality to tap past.
 */
fun buildPairingScreen(context: Context, store: AppStore, nav: Nav): ScreenView {
    val pal = ReposeTheme.of(context)
    val provisioned = PresenceKey.has(SpikeContract.PRESENCE_KEY_ID)
    val fingerprint = PresenceKey.fingerprint(context)
    val windowOpen = Pairing.isOpen || Pairing.digits != null

    val title = when {
        windowOpen -> "正在配对"
        provisioned -> "这台手机已有在场密钥"
        else -> "这台手机还没有在场密钥"
    }

    val root = screenScaffold(context = context, pal = pal, title = title) { column ->
        if (!windowOpen) {
            column.addView(
                Ui.amberNote(
                    context,
                    pal,
                    if (provisioned) {
                        "这把密钥可能来自两条路：USB 下发（开发用，防不了中间人），" +
                            "或者两端比对六位数字的配对。界面分不出来 —— 不确定就重新配对一次。"
                    } else {
                        "配对时两端会各显示一串六位数字，由你核对它们一致。" +
                            "这一步就是防中间人的全部：不一致就说明有人在中间。"
                    },
                ),
                Ui.lp(top = context.dp(10)),
            )
        }

        when {
            windowOpen -> renderPairingWindow(context, pal, store, nav, column)
            provisioned -> renderProvisioned(context, pal, store, nav, column, fingerprint)
            else -> renderNoKey(context, pal, nav, column)
        }
    }

    // The key and the digits both arrive from the GATT server after this screen
    // was built, so the first render is always taken before anything happened.
    // Rebuild when either answer changes; it settles in one pass.
    return ScreenView(root, onState = {
        val stillOpen = Pairing.isOpen || Pairing.digits != null
        if (stillOpen != windowOpen ||
            PresenceKey.has(SpikeContract.PRESENCE_KEY_ID) != provisioned
        ) {
            nav.go(Screen.PAIRING)
        }
    })
}

/**
 * The comparison, or the wait before it.
 *
 * Everything here exists to make one question easy to answer correctly: are
 * these the same six digits the Mac is showing?
 */
private fun renderPairingWindow(
    context: Context,
    pal: Palette,
    store: AppStore,
    nav: Nav,
    column: LinearLayout,
) {
    val digits = Pairing.digits
    if (digits != null) {
        column.addView(
            Ui.body(context, pal, "Mac 上现在应当显示同样的六位数字。"),
            Ui.lp(top = context.dp(14)),
        )
        val card = Ui.card(context, pal).apply { gravity = Gravity.CENTER_HORIZONTAL }
        card.addView(
            TextView(context).apply {
                text = digits
                setTextColor(pal.accent)
                setTextSize(TypedValue.COMPLEX_UNIT_SP, 44f)
                typeface = Typeface.create("monospace", Typeface.BOLD)
                gravity = Gravity.CENTER
                letterSpacing = 0.3f
                maxLines = 1
            },
            Ui.lp(width = WRAP_CONTENT),
        )
        column.addView(card, Ui.lp(top = context.dp(18)))
        column.addView(
            Ui.amberNote(
                context,
                pal,
                "不一致就说明有人在中间。这种时候按「不一致」，并且不要重试同一次 —— " +
                    "允许重试等于让对方一直猜下去，六位数字就不再是六位数字。",
            ),
            Ui.lp(top = context.dp(18)),
        )
        column.addView(
            Ui.primaryButton(context, pal, "一致，完成配对") {
                if (Pairing.confirm()) {
                    store.paired = true
                    Toast.makeText(context, "配对完成。", Toast.LENGTH_LONG).show()
                } else {
                    Toast.makeText(
                        context,
                        Pairing.lastError ?: "配对没有完成。",
                        Toast.LENGTH_LONG,
                    ).show()
                }
                nav.go(Screen.PAIRING)
            },
            Ui.lp(top = context.dp(22)),
        )
        column.addView(
            Ui.ghostButton(context, pal, "不一致，中止") {
                Pairing.reject()
                Toast.makeText(context, "已中止。", Toast.LENGTH_LONG).show()
                nav.go(Screen.PAIRING)
            },
            Ui.lp(top = context.dp(12)),
        )
    } else {
        column.addView(
            Ui.body(context, pal, "配对窗口已打开，等待 Mac 连接。"),
            Ui.lp(top = context.dp(14)),
        )
        column.addView(
            Ui.infoNote(
                context,
                pal,
                "在 Mac 上执行：\n  tools/ble-spike/pair.sh\n\n" +
                    "两边都会显示一串六位数字，核对一致再确认。窗口 " +
                    "${SpikeContract.PAIRING_WINDOW_SECONDS} 秒后自动关闭，" +
                    "这段时间在场信标会暂停。",
            ),
            Ui.lp(top = context.dp(18)),
        )
        Pairing.lastError?.let {
            column.addView(Ui.amberNote(context, pal, it), Ui.lp(top = context.dp(14)))
        }
        column.addView(
            Ui.ghostButton(context, pal, "取消") {
                Pairing.stop()
                nav.go(Screen.PAIRING)
            },
            Ui.lp(top = context.dp(22)),
        )
    }
}

private fun renderProvisioned(
    context: Context,
    pal: Palette,
    store: AppStore,
    nav: Nav,
    column: LinearLayout,
    fingerprint: String?,
) {
    column.addView(
        Ui.body(context, pal, "把下面这串和 Mac 上显示的指纹对一下。不一致就说明不是同一把钥匙。"),
        Ui.lp(top = context.dp(14)),
    )
    val card = Ui.card(context, pal).apply { gravity = Gravity.CENTER_HORIZONTAL }
    card.addView(
        Ui.secondary(context, pal, "密钥指纹").apply { gravity = Gravity.CENTER },
        Ui.lp(width = WRAP_CONTENT),
    )
    card.addView(
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
    column.addView(card, Ui.lp(top = context.dp(18)))
    column.addView(
        Ui.infoNote(
            context,
            pal,
            "指纹是 K 的哈希，不是 K 的一部分——可以给别人看。密钥本身不可导出，" +
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
    // Replace without destroying first. The only route used to be "delete, then
    // hope pairing works", which leaves a Mac trusting nothing if anything goes
    // wrong in between -- and makes testing a change mean breaking a setup that
    // works.
    column.addView(
        Ui.ghostButton(context, pal, "重新配对") {
            Pairing.start(context) { nav.go(Screen.PAIRING) }
            nav.go(Screen.PAIRING)
        },
        Ui.lp(top = context.dp(12)),
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
}

private fun renderNoKey(context: Context, pal: Palette, nav: Nav, column: LinearLayout) {
    column.addView(
        Ui.body(
            context,
            pal,
            "现在信标照常广播，但标签是用一把随机数临时凑出来的——Mac 会看见这台手机，" +
                "然后拒绝它。这就是没有密钥时应有的样子。",
        ),
        Ui.lp(top = context.dp(14)),
    )
    Pairing.lastError?.let {
        column.addView(Ui.amberNote(context, pal, it), Ui.lp(top = context.dp(14)))
    }
    column.addView(
        Ui.primaryButton(context, pal, "开始配对") {
            Pairing.start(context) { nav.go(Screen.PAIRING) }
            nav.go(Screen.PAIRING)
        },
        Ui.lp(top = context.dp(22)),
    )
    column.addView(
        Ui.ghostButton(context, pal, "先跳过") { nav.go(Screen.HOME) },
        Ui.lp(top = context.dp(12)),
    )
}
