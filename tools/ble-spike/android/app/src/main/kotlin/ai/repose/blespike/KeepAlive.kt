package ai.repose.blespike

/**
 * What this phone has to allow for the key to keep working after the screen
 * goes dark (design doc §04 手机·主屏 「让它一直在」).
 *
 * Four things, in the order they bite. Three are readable from the system;
 * the last -- locking the app's card in the recents screen so a one-tap
 * clean-up does not swipe it away -- is not, so the person says when it is
 * done and the phone remembers. Pure, so the list is tested without a device.
 */
enum class KeepStep { BLUETOOTH, BATTERY, BACKGROUND, NOTIFICATIONS }

data class KeepAliveFacts(
    /** BLUETOOTH_ADVERTISE, CONNECT and SCAN all granted. Android calls them 「附近的设备」. */
    val bluetoothGranted: Boolean,
    /** POST_NOTIFICATIONS granted (true below Android 13, where it does not exist). */
    val notificationsGranted: Boolean,
    /** PowerManager.isIgnoringBatteryOptimizations. */
    val batteryExempt: Boolean,
    /** The person said the skin's background switches are set. Only asked on skins that have them. */
    val backgroundAcknowledged: Boolean,
    /** Build.MANUFACTURER, any case. */
    val manufacturer: String,
)

/** One line of the checklist: what it gives you first, then what it needs. */
data class KeepItem(
    val step: KeepStep,
    val title: String,
    val gives: String,
    val done: Boolean,
    /** The button when not done. */
    val action: String,
    /** Whether the key works without it. */
    val optional: Boolean = false,
)

object KeepAlive {

    /**
     * Skins that freeze or kill a background service on their own rules, over
     * and above Android's battery optimisation. What a car-key app walks you
     * through on these phones: let it start by itself, let it run in the
     * background, do not limit its power use.
     */
    private val GUIDED_BRANDS = listOf("realme", "oppo", "oneplus", "xiaomi", "redmi", "poco", "huawei", "honor", "vivo", "iqoo", "meizu", "samsung")

    fun needsBackgroundStep(manufacturer: String): Boolean =
        GUIDED_BRANDS.any { manufacturer.trim().lowercase().contains(it) }

    /** The exact switches, in the words that skin uses. Opens from the app's own settings page. */
    fun backgroundGuide(manufacturer: String): String {
        val m = manufacturer.trim().lowercase()
        val steps = when {
            listOf("realme", "oppo", "oneplus").any { m.contains(it) } ->
                "在「应用详情」里点「耗电管理」，选「完全允许后台行为」。回到「应用详情」，把「管理闲置应用」关掉。" +
                    "再到 设置 → 应用 → 自启动管理，允许 ${Brand.NAME}。"
            listOf("xiaomi", "redmi", "poco").any { m.contains(it) } ->
                "在这个页面把「自启动」打开；点「省电策略」，选「无限制」。"
            listOf("huawei", "honor").any { m.contains(it) } ->
                "在这个页面点「应用启动管理」，关掉「自动管理」，把「允许自启动」「允许关联启动」「允许后台活动」都打开。"
            listOf("vivo", "iqoo").any { m.contains(it) } ->
                "在这个页面把「自启动」打开；点「电池」，把「后台高耗电」允许。"
            m.contains("samsung") ->
                "在这个页面点「电池」，选「不受限制」；关掉「进入休眠状态」。"
            else -> "在这个页面允许它自启动和后台运行。"
        }
        return "$steps 再打开最近任务，把 ${Brand.NAME} 的卡片锁定，一键清理就不会划掉它。做完回来点「我做好了」。"
    }

    /**
     * Pictures for a step on this skin, as (drawable name, caption). Taken on
     * the real phone, so the person sees the page they are about to see.
     */
    fun pictures(step: KeepStep, manufacturer: String): List<Pair<String, String>> {
        val m = manufacturer.trim().lowercase()
        val coloros = listOf("realme", "oppo", "oneplus").any { m.contains(it) }
        return when {
            step == KeepStep.BACKGROUND && coloros -> listOf(
                "keepalive_coloros_appinfo" to "「应用详情」：点「耗电管理」；下面的「管理闲置应用」关掉。",
                "keepalive_coloros_power" to "「耗电管理」：选「完全允许后台行为」。",
            )
            else -> emptyList()
        }
    }

    fun items(f: KeepAliveFacts): List<KeepItem> {
        val out = mutableListOf(
            KeepItem(
                KeepStep.BLUETOOTH, "让 Mac 认出这部手机",
                "钥匙靠蓝牙广播。Android 把这个权限叫「附近的设备」。",
                done = f.bluetoothGranted, action = "允许",
            ),
            KeepItem(
                KeepStep.BATTERY, "息屏之后也在",
                "不让系统为了省电把它关掉。关掉了，Mac 认不出你，也不会有提示。",
                done = f.batteryExempt, action = "去设置",
            ),
        )
        if (needsBackgroundStep(f.manufacturer)) {
            out += KeepItem(
                KeepStep.BACKGROUND, "开机自己起来，后台不被冻结",
                "这种手机有自己的一套省电规则，要单独允许它自启动和后台运行。车钥匙那类 App 也是这么设的。",
                done = f.backgroundAcknowledged, action = "去设置",
            )
        }
        out += KeepItem(
            KeepStep.NOTIFICATIONS, "通知栏留一行",
            "钥匙在后台跑着时，通知栏有一行提醒你。不允许也能用。",
            done = f.notificationsGranted, action = "允许", optional = true,
        )
        return out
    }

    /** The checklist can leave the screen: nothing required is missing. */
    fun allDone(f: KeepAliveFacts): Boolean = items(f).all { it.done || it.optional }

    /** The one line under 「更多」 once the card is gone. */
    fun summary(f: KeepAliveFacts): String {
        val missing = items(f).count { !it.done && !it.optional }
        return if (missing == 0) "后台常驻：都设好了。" else "后台常驻：还有 $missing 件没设。"
    }
}
