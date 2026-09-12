package ai.repose.blespike

import android.content.Context
import android.graphics.Typeface
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.view.ViewGroup.LayoutParams.WRAP_CONTENT
import android.widget.LinearLayout
import android.widget.TextView
import android.widget.Toast

/**
 * Screen 1 — 配对, phone side (design doc §05「配对，然后量距离」).
 *
 * Three views, in priority order:
 *
 *   1. A pairing window is open  -> the six digits, or "waiting for the Mac"
 *   2. A key exists              -> 配好了 and 量一次距离 right after a pairing;
 *                                   otherwise the Macs this phone already opens
 *   3. Neither                   -> start pairing
 *
 * The window beats the key on purpose. Somebody adding a second Mac needs the
 * digits in front of them, not the list of Macs they are in the middle of
 * adding to.
 *
 * This screen has now been rewritten five times, twice for the same reason: it
 * said things the code did not do. The first showed a locally-invented code and
 * claimed the Mac was showing it too. The second said plainly that no key
 * exchange existed. One does now, and the copy has to be equally careful in the
 * other direction -- the six digits really are the whole of the MITM defence, so
 * the screen's job is to make comparing them feel like the point, not a
 * formality to tap past.
 *
 * The fourth pass was about shape rather than truth: four stacked paragraphs on
 * a flat background read as a debug build. Hero, cards, and a step row now say
 * where you are.
 *
 * The fifth pass is the design doc's wording (§01 rules, §05 phone frames):
 * short sentences, no protocol explanations on the surface, technical detail
 * under 「更多」, and one 「返回」 instead of 取消 / 继续 / 先跳过. It also drops
 * the old single-Mac name field: this phone can hold a key per Mac, and the
 * screen names them from [AppStore.pairedMacs].
 */

/**
 * Set by 「一样，完成配对」, read once by the very next build.
 *
 * A key in the Keystore answers "is this phone paired", not "did you just pair".
 * Both the person who just watched the digits match and the person who came
 * from 「＋ 添加电脑」 land on the same view, and only the first should be told
 * 配好了 and sent to measure.
 */
private var justPaired = false

fun buildPairingScreen(context: Context, store: AppStore, nav: Nav): ScreenView {
    val pal = ReposeTheme.of(context)
    val provisioned = PresenceKey.hasAny(context)
    val fingerprint = PresenceKey.fingerprint(context)
    val windowOpen = Pairing.isOpen || Pairing.digits != null
    val fresh = justPaired
    justPaired = false

    // 已配对 answers "is there a key in the Keystore", which is NOT "did the
    // pairing you just did work". A failed pairing leaves an older key
    // untouched, so this screen went on saying 已经配对好了 to somebody who had
    // just watched it fail -- and who had, in their words, done nothing at all.
    val justFailed = Pairing.lastError != null && !windowOpen

    // One 返回, and only where there is somewhere to go. With no key the home
    // screen has nothing on it, so this screen is the way in, not a detour;
    // the old 「先跳过」 dropped people exactly there.
    val onBack: (() -> Unit)? = when {
        provisioned -> {
            {
                Pairing.stop()
                nav.go(Screen.HOME)
            }
        }
        windowOpen -> {
            {
                Pairing.stop()
                nav.go(Screen.PAIRING)
            }
        }
        else -> null
    }

    val root = screenScaffold(
        context = context,
        pal = pal,
        title = "",
        showTitle = false,
        onBack = onBack,
    ) { column ->
        // Said first, and said plainly: this is the screen where somebody finds
        // out whether the thing they just did worked.
        if (justFailed) {
            column.addView(
                Ui.amberNote(
                    context,
                    pal,
                    (Pairing.lastError ?: "这次没配成。") +
                        if (provisioned) {
                            "\n\n上一次的钥匙还在，照样能用。要是那台 Mac 刚重新配过，就对不上了，再配一次。"
                        } else {
                            ""
                        },
                ),
                Ui.lp(top = context.dp(6)),
            )
        }

        when {
            windowOpen -> renderPairingWindow(context, pal, store, nav, column)
            provisioned -> renderProvisioned(context, pal, store, nav, column, fingerprint, fresh)
            else -> renderNoKey(context, pal, nav, column)
        }
    }

    // The key and the digits both arrive from the GATT server after this screen
    // was built, so the first render is always taken before anything happened.
    // Rebuild when either answer changes; it settles in one pass.
    return ScreenView(root, onState = {
        val stillOpen = Pairing.isOpen || Pairing.digits != null
        if (stillOpen != windowOpen ||
            PresenceKey.hasAny(context) != provisioned
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
                body = "Mac 上现在也显示着同一串。",
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
            Ui.secondary(context, pal, "两边一样，中间就没有人冒充。").apply {
                gravity = Gravity.CENTER
            },
            Ui.lp(top = context.dp(12)),
        )
        column.addView(
            Ui.primaryButton(context, pal, "一样，完成配对") {
                if (Pairing.confirm()) {
                    justPaired = true
                    Toast.makeText(context, "配好了。", Toast.LENGTH_LONG).show()
                } else {
                    Toast.makeText(
                        context,
                        Pairing.lastError ?: "这次没配成。",
                        Toast.LENGTH_LONG,
                    ).show()
                }
                nav.go(Screen.PAIRING)
            },
            Ui.lp(top = context.dp(20)),
        )
        // A real answer, not a cancel (design doc §05): this session is void,
        // and the next attempt starts from nothing.
        column.addView(
            Ui.ghostButton(context, pal, "不一样，停下") {
                Pairing.reject()
                Toast.makeText(context, "停下了。这次作废，从头再配。", Toast.LENGTH_LONG).show()
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
                headline = "准备好了",
                body = "在 Mac 上点「配一部新手机」，几秒钟这里就会自己接上。",
            ),
            Ui.lp(top = context.dp(6)),
        )
        val next = sectionCard(context, pal, "🔢", "接下来")
        next.addView(
            Ui.body(context, pal, "两边会各显示六位数字。看一眼是不是一样。"),
            Ui.lp(top = context.dp(12)),
        )
        // Both ends give up on their own clocks -- this window after three
        // minutes of nothing (SpikeContract.PAIRING_WINDOW_SECONDS), the Mac
        // after three minutes of not finding the phone -- and someone who
        // waited it out needs to know that both stopped, not just this one,
        // or they restart here and wait for a Mac that is no longer looking.
        next.addView(
            Ui.secondary(context, pal, "三分钟没接上，两边都会自己停下。这里停，Mac 也不找了。再开始一次就行，先点哪边都行。"),
            Ui.lp(top = context.dp(10)),
        )
        column.addView(next, Ui.lp(top = context.dp(14)))
        Pairing.lastError?.let {
            column.addView(Ui.amberNote(context, pal, it), Ui.lp(top = context.dp(14)))
        }
    }
}

/**
 * A key exists. Right after a pairing this is the doc's 配好了 frame and its one
 * button, 量一次距离. Any other time it is the list of Macs this phone opens,
 * with a way to add one.
 */
private fun renderProvisioned(
    context: Context,
    pal: Palette,
    store: AppStore,
    nav: Nav,
    column: LinearLayout,
    fingerprint: String?,
    fresh: Boolean,
) {
    val macs = store.pairedMacs(context)
    // ISO-8601 timestamps order as strings; a record with no timestamp sorts
    // first, so the Mac that was just written wins.
    val newest = macs.maxByOrNull { it.pairedAt }
    val failed = Pairing.lastError != null
    val showFresh = fresh && !failed && newest != null

    column.addView(
        heroCard(
            context, pal,
            chip = if (showFresh) "手机这边" else "你的 Mac",
            glyph = if (showFresh) "✅" else "🔑",
            headline = when {
                showFresh -> "配好了"
                failed -> "这次没配成"
                macs.size == 1 -> "这台 Mac 认得你的手机"
                else -> "这 ${macs.size} 台 Mac 都认得你的手机"
            },
            body = when {
                showFresh -> "接下来量一次距离，Mac 才知道你什么时候算「在」。"
                failed -> "要再配一次，在 Mac 上点「配一部新手机」。"
                else -> "要再配一台，在那台 Mac 上点「配一部新手机」。"
            },
        ),
        Ui.lp(top = context.dp(6)),
    )

    val card = sectionCard(context, pal, "💻", if (macs.size == 1) "配好的 Mac" else "配好的 ${macs.size} 台 Mac")
    for (mac in macs) {
        card.addView(
            TextView(context).apply {
                text = mac.name
                setTextColor(pal.textPrimary)
                setTextSize(TypedValue.COMPLEX_UNIT_SP, 19f)
                typeface = Typeface.create("sans-serif-medium", Typeface.NORMAL)
            },
            Ui.lp(top = context.dp(12)),
        )
    }
    card.addView(
        Ui.secondary(context, pal, "名字是它自己报的，只是方便你认。"),
        Ui.lp(top = context.dp(8)),
    )
    column.addView(card, Ui.lp(top = context.dp(14)))

    if (showFresh) {
        val target = newest!!
        column.addView(
            Ui.primaryButton(context, pal, "量一次距离") {
                calMac(target.keyId)
                nav.go(Screen.CAL)
            },
            Ui.lp(top = context.dp(20)),
        )
    } else {
        // Add without destroying first. The only route used to be "delete, then
        // hope pairing works", which leaves a Mac trusting nothing if anything
        // goes wrong in between -- and makes testing a change mean breaking a
        // setup that works.
        column.addView(
            Ui.primaryButton(context, pal, if (failed) "再配一次" else "再配一台 Mac") {
                Pairing.start(context) { nav.go(Screen.PAIRING) }
                nav.go(Screen.PAIRING)
            },
            Ui.lp(top = context.dp(20)),
        )
    }

    // ---- 更多: what you read once ----
    //
    // The fingerprint is technical detail; the surface stays in plain words
    // (design doc §01 rule 4). Same fold as the home screen.
    val more = LinearLayout(context).apply {
        orientation = LinearLayout.VERTICAL
        visibility = View.GONE
    }
    val moreToggle = TextView(context).apply {
        text = "▸ 更多"
        setTextColor(pal.textSecondary)
        setTextSize(TypedValue.COMPLEX_UNIT_SP, 13f)
        setPadding(context.dp(4), context.dp(16), context.dp(4), context.dp(4))
        isClickable = true
        setOnClickListener {
            val open = more.visibility == View.VISIBLE
            more.visibility = if (open) View.GONE else View.VISIBLE
            text = if (open) "▸ 更多" else "▾ 更多"
        }
    }
    column.addView(moreToggle, Ui.lp(width = WRAP_CONTENT))
    more.addView(
        Ui.infoNote(context, pal, "每台 Mac 一把钥匙，都存在这部手机里，导不出去。"),
        Ui.lp(top = context.dp(6)),
    )
    more.addView(techDetails(context, pal, fingerprint), Ui.lp(top = context.dp(6)))
    column.addView(more)
}

private fun renderNoKey(context: Context, pal: Palette, nav: Nav, column: LinearLayout) {
    column.addView(
        heroCard(
            context, pal,
            chip = "还没开始",
            glyph = "🔗",
            headline = "手机就是钥匙",
            body = "人在电脑前，回车就能解锁。先和你的 Mac 配一次。",
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
        Ui.body(context, pal, "两边各显示六位数字。你看一眼是不是一样。"),
        Ui.lp(top = context.dp(12)),
    )
    card.addView(
        Ui.secondary(context, pal, "两边一样，中间就没有人冒充。"),
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
}
