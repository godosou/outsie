package ai.repose.mobile.unlock.pairing

internal fun interface DebugGattTimeoutTicket {
    fun cancel()
}

internal fun interface DebugGattTimeoutScheduler {
    fun schedule(delayMillis: Long, callback: () -> Unit): DebugGattTimeoutTicket
}

internal class DebugGattTimeoutGuard(
    private val scheduler: DebugGattTimeoutScheduler,
    private val delayMillis: Long,
    private val onTimeout: () -> Unit,
) {
    private val monitor = Any()
    private var generation = 0L
    private var active = false
    private var ticket: DebugGattTimeoutTicket? = null

    init {
        require(delayMillis > 0)
    }

    fun arm() {
        val (token, previous) = synchronized(monitor) {
            active = true
            ticket.also { ticket = null }.let { previous ->
                ++generation to previous
            }
        }
        previous?.cancelSafely()
        val scheduled = try {
            scheduler.schedule(delayMillis) { fire(token) }
        } catch (error: RuntimeException) {
            synchronized(monitor) {
                if (active && generation == token) {
                    active = false
                    generation += 1
                }
            }
            throw error
        }
        val stale = synchronized(monitor) {
            if (active && generation == token) {
                ticket = scheduled
                false
            } else {
                true
            }
        }
        if (stale) scheduled.cancelSafely()
    }

    fun cancel() {
        val cancelled = synchronized(monitor) {
            active = false
            generation += 1
            ticket.also { ticket = null }
        }
        cancelled?.cancelSafely()
    }

    private fun fire(token: Long) {
        val deliver = synchronized(monitor) {
            if (!active || generation != token) {
                false
            } else {
                active = false
                ticket = null
                true
            }
        }
        if (deliver) onTimeout()
    }
}

private fun DebugGattTimeoutTicket.cancelSafely() {
    try {
        cancel()
    } catch (_: RuntimeException) {
        // The generation fence already revoked the callback's authority.
    }
}

internal class DebugGattHandshakeDeadline(
    scheduler: DebugGattTimeoutScheduler,
    delayMillis: Long,
    expectedAcceptedStatus: ByteArray,
    private val onTimeout: () -> Unit,
) {
    private val monitor = Any()
    private val expectedAcceptedStatus = expectedAcceptedStatus.copyOf()
    private val timeoutGuard = DebugGattTimeoutGuard(scheduler, delayMillis, ::handleTimeout)
    private var state = State.NEW

    fun arm() {
        synchronized(monitor) {
            check(state == State.NEW)
            state = State.PENDING
        }
        try {
            timeoutGuard.arm()
        } catch (error: RuntimeException) {
            synchronized(monitor) { state = State.CLOSED }
            throw error
        }
    }

    fun permitStatus(value: ByteArray): Boolean {
        val accepted = value.contentEquals(expectedAcceptedStatus)
        val permitted = synchronized(monitor) {
            when (state) {
                State.PENDING, State.ACCEPTED -> {
                    if (accepted) state = State.ACCEPTED
                    true
                }
                State.NEW, State.TIMED_OUT, State.CLOSED -> false
            }
        }
        if (permitted && accepted) timeoutGuard.cancel()
        return permitted
    }

    fun close() {
        synchronized(monitor) { state = State.CLOSED }
        timeoutGuard.cancel()
        expectedAcceptedStatus.fill(0)
    }

    private fun handleTimeout() {
        val deliver = synchronized(monitor) {
            if (state != State.PENDING) {
                false
            } else {
                state = State.TIMED_OUT
                true
            }
        }
        if (deliver) onTimeout()
    }

    private enum class State { NEW, PENDING, ACCEPTED, TIMED_OUT, CLOSED }
}
