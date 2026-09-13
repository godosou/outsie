package ai.repose.blespike

/**
 * What the service actually knows about the key's radio. Written by the
 * service, read by the screen. Nothing here is derived -- the derivation is
 * [phase], computed at read time so a fact that changed underneath the
 * service (Bluetooth switched off, say) changes the answer at once.
 */
data class RadioFacts(
    /** The foreground service is alive. Set in onCreate, cleared in onDestroy. */
    val running: Boolean,
    /** At least one key slot exists on this phone. */
    val hasKeys: Boolean,
    /** The phone's Bluetooth adapter is on, as last reported by the system. */
    val bluetoothOn: Boolean,
    /** startAdvertising's success callback has fired for at least one slot. */
    val advertising: Boolean,
    /** The last thing that went wrong, in the user's words, or null. */
    val failure: String?,
    /** When the service last asked the radio to start, or null if it has not. */
    val startingSince: Long?,
) {
    /**
     * The one word the screen keys on, in the order that matters: a service
     * that is not running is off no matter what the radio was doing; a phone
     * with no key cannot be a key; a phone with Bluetooth off cannot start;
     * only then does the radio's own progress count.
     */
    fun phase(now: Long): RadioPhase = when {
        !running -> RadioPhase.OFF
        !hasKeys -> RadioPhase.NO_KEY
        !bluetoothOn -> RadioPhase.BLUETOOTH_OFF
        failure != null -> RadioPhase.FAILED
        advertising -> RadioPhase.ON
        startingSince != null && now - startingSince > STARTING_GRACE_MS -> RadioPhase.FAILED
        else -> RadioPhase.STARTING
    }

    companion object {
        /** How long 「正在开始广播」 may honestly last. Advertising starts in well under a second. */
        const val STARTING_GRACE_MS = 10_000L

        val NOTHING = RadioFacts(
            running = false, hasKeys = false, bluetoothOn = true,
            advertising = false, failure = null, startingSince = null,
        )
    }
}

/** The six things 「当你的钥匙」 can be (design doc §04 手机·主屏). */
enum class RadioPhase(
    /** What the switch shows: the person's intent, not the radio's luck. */
    val switchOn: Boolean,
    /** Whether a command posted now would reach the air and a Mac could be heard. */
    val onAir: Boolean,
) {
    OFF(switchOn = false, onAir = false),
    NO_KEY(switchOn = false, onAir = false),
    BLUETOOTH_OFF(switchOn = true, onAir = false),
    STARTING(switchOn = true, onAir = false),
    FAILED(switchOn = true, onAir = false),
    ON(switchOn = true, onAir = true);

    /**
     * One sentence for the row under the switch. Every waiting state says what
     * ends the wait; every failure says what to do (design doc §01).
     */
    fun sentence(failure: String?): String = when (this) {
        OFF -> "关着。现在谁都认不出这部手机。"
        NO_KEY -> "还没配对，Mac 认不出这部手机。"
        BLUETOOTH_OFF -> "手机的蓝牙关着。打开蓝牙，它自己接着广播。"
        STARTING -> "正在开始广播…几秒就好。"
        ON -> "开着。附近的 Mac 认得你。"
        FAILED -> if (failure.isNullOrBlank()) "广播没开起来。关掉再开一次。" else "广播没开起来：$failure。关掉再开一次。"
    }
}
