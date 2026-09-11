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
 * Screen 1 — 配对.
 *
 * Three views, in priority order:
 *
 *   1. A pairing window is open  -> the six digits, or "waiting for the Mac"
 *   2. A key exists              -> its short id, and a way to re-pair
 *   3. Neither                   -> start pairing
 *
 * The window beats the key on purpose. Somebody re-pairing needs the digits in
 * front of them, not the id of the key they are in the middle of replacing.
 *
 * This screen has now been rewritten four times, twice for the same reason: it
 * said things the code did not do. The first showed a locally-invented code and
 * claimed the Mac was showing it too. The second said plainly that no key
 * exchange existed. One does now, and the copy has to be equally careful in the
 * other direction -- the six digits really are the whole of the MITM defence, so
 * the screen's job is to make comparing them feel like the point, not a
 * formality to tap past.
 *
 * The fourth pass was about shape rather than truth: four stacked paragraphs on
 * a flat background read as a debug build, and a debug build is not a thing
 * anyone should hand their lock screen to. Hero, cards, and a step row now say
 * where you are.
 */
fun buildPairingScreen(context: Context, store: AppStore, nav: Nav): ScreenView {
    val pal = ReposeTheme.of(context)
    val provisioned = PresenceKey.has(SpikeContract.PRESENCE_KEY_ID)
    val fingerprint = PresenceKey.fingerprint(context)
    val windowOpen = Pairing.isOpen || Pairing.digits != null

    val root = screenScaffold(
        context = context,
        pal = pal,
        title = "",
        showTitle = false,
    ) { column ->
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
 * these the same six digits the Mac is showing? So the digits are the largest
 * thing on the screen and nothing competes with them — no id, no diagnostics,
 * no second call to action.
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
            heroCard(
                context, pal,
                chip = "第 2 步，共 2 步",
                glyph = "👀",
                headline = "核对这六位数字",
                body = "Mac 上现在也应该显示同一串。",
            ),
            Ui.lp(top = context.dp(6)),
        )
        val card = Ui.card(context, pal).apply { gravity = Gravity.CENTER_HORIZONTAL }
        card.addView(
            TextView(context).apply {
                text = digits
                setTextColor(pal.accent)
                setTextSize(TypedValue.COMPLEX_UNIT_SP, 46f)
                typeface = Typeface.create("monospace", Typeface.BOLD)
                gravity = Gravity.CENTER
                letterSpacing = 0.3f
                maxLines = 1
            },
            Ui.lp(width = WRAP_CONTENT),
        )
        column.addView(card, Ui.lp(top = context.dp(14)))
        column.addView(
            Ui.amberNote(
                context,
                pal,
                "为什么两边都要点：任何「对方已确认」的消息都要走无线，而中间人能把它拆开重发。" +
                    "你的手指是两台设备之间唯一伪造不了的通道。\n\n" +
                    "两边不一样就按「不一样」，换个地方从头重配 —— 不要就着这次再试。",
            ),
            Ui.lp(top = context.dp(16)),
        )
        column.addView(
            Ui.primaryButton(context, pal, "一样，完成配对") {
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
            Ui.lp(top = context.dp(20)),
        )
        column.addView(
            Ui.ghostButton(context, pal, "不一样，停下") {
                Pairing.reject()
                Toast.makeText(context, "已中止。", Toast.LENGTH_LONG).show()
                nav.go(Screen.PAIRING)
            },
            Ui.lp(top = context.dp(10)),
        )
    } else {
        column.addView(
            heroCard(
                context, pal,
                chip = "第 1 步，共 2 步",
                glyph = "📡",
                headline = "手机准备好了",
                body = "等 Mac 那边开始。",
            ),
            Ui.lp(top = context.dp(6)),
        )
        val how = sectionCard(context, pal, "💻", "在 Mac 上")
        how.addView(
            Ui.body(
                context,
                pal,
                "打开 ${Brand.NAME} → 设置 → 手机钥匙，点「配对手机」。",
            ),
            Ui.lp(top = context.dp(12)),
        )
        how.addView(
            Ui.secondary(
                context,
                pal,
                "两边会各显示一串六位数字，看一眼是不是一样，再分别确认。" +
                    "三分钟内没配好就会自己停下，重新开始即可。",
            ),
            Ui.lp(top = context.dp(10)),
        )
        column.addView(how, Ui.lp(top = context.dp(14)))
        Pairing.lastError?.let {
            column.addView(Ui.amberNote(context, pal, it), Ui.lp(top = context.dp(14)))
        }
        column.addView(
            Ui.ghostButton(context, pal, "取消") {
                Pairing.stop()
                nav.go(Screen.PAIRING)
            },
            Ui.lp(top = context.dp(20)),
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
    val macName = AppStore(context).pairedMac
    column.addView(
        heroCard(
            context, pal,
            chip = "已配对",
            glyph = "🔑",
            headline = macName?.let { "已经和「$it」配对" } ?: "已经配对好了",
            body = "这台 Mac 认得你的手机。",
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

    column.addView(
        Ui.infoNote(
            context,
            pal,
            "钥匙存在手机的安全芯片里，谁也拿不出来，包括这个 App。",
        ),
        Ui.lp(top = context.dp(14)),
    )
    column.addView(techDetails(context, pal, fingerprint), Ui.lp(top = context.dp(14)))
    column.addView(
        Ui.primaryButton(context, pal, "继续") {
            store.paired = true
            nav.go(Screen.HOME)
        },
        Ui.lp(top = context.dp(20)),
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
        Ui.lp(top = context.dp(10)),
    )
}

private fun renderNoKey(context: Context, pal: Palette, nav: Nav, column: LinearLayout) {
    column.addView(
        heroCard(
            context, pal,
            chip = "还没开始",
            glyph = "🔗",
            headline = "先和你的 Mac 配对",
            body = "配好之后，人在电脑前就能直接回车解锁。",
            muted = true,
        ),
        Ui.lp(top = context.dp(6)),
    )
    column.addView(
        stepRow(context, pal, listOf("配对", "开着", "靠近"), activeIndex = 0),
        Ui.lp(top = context.dp(14)),
    )
    val card = sectionCard(context, pal, "🔢", "配对怎么做")
    card.addView(
        Ui.body(
            context,
            pal,
            "两边各显示一串六位数字，你看一眼是不是一样。",
        ),
        Ui.lp(top = context.dp(12)),
    )
    card.addView(
        Ui.secondary(
            context,
            pal,
            "这一眼就是全部的安全保障：不一样，就说明中间有人在冒充。" +
                "现在 Mac 会看见这台手机，但认不出它是你的，所以不会解锁。",
        ),
        Ui.lp(top = context.dp(10)),
    )
    column.addView(card, Ui.lp(top = context.dp(14)))
    Pairing.lastError?.let {
        column.addView(Ui.amberNote(context, pal, it), Ui.lp(top = context.dp(14)))
    }
    column.addView(
        Ui.primaryButton(context, pal, "开始配对") {
            Pairing.start(context) { nav.go(Screen.PAIRING) }
            nav.go(Screen.PAIRING)
        },
        Ui.lp(top = context.dp(20)),
    )
    column.addView(
        Ui.ghostButton(context, pal, "先跳过") { nav.go(Screen.HOME) },
        Ui.lp(top = context.dp(10)),
    )
}
