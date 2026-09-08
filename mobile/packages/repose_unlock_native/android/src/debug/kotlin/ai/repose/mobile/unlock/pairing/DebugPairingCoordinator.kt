package ai.repose.mobile.unlock.pairing

import ai.repose.mobile.unlock.protocol.PairingUriFormatException
import ai.repose.mobile.unlock.protocol.PairingUriV1
import ai.repose.mobile.unlock.protocol.PairingPayloadV1
import ai.repose.mobile.unlock.console.DebugConsoleCredentials
import java.nio.ByteBuffer
import java.nio.charset.CodingErrorAction
import java.nio.charset.StandardCharsets

internal enum class DebugPairingFailure {
    INVALID_QR,
    QR_EXPIRED,
    QR_ALREADY_USED,
    ASSOCIATION_UNAVAILABLE,
    BUSY,
    NOT_ACCEPTED,
    SESSION_MISMATCH,
    DEVICE_NOT_FOUND,
    TRANSPORT_UNAVAILABLE,
}

internal class DebugPairingException(
    val reason: DebugPairingFailure,
) : IllegalStateException("Debug pairing failed closed: ${reason.name}")

internal data class DebugPhoneIdentity(
    val deviceId: String,
    val displayName: String,
) {
    init {
        require(DEVICE_ID.matches(deviceId))
        val encodedName = displayName.toByteArray(StandardCharsets.UTF_8)
        require(encodedName.size in 1..80)
        require(displayName.none { it == '|' || Character.isISOControl(it) })
    }

    private companion object {
        val DEVICE_ID = Regex("[A-Za-z0-9_-]{1,64}")
    }
}

internal data class DebugGattPairingRequest(
    val associationId: Int,
    val controlValue: ByteArray,
    val expectedAcceptedStatus: ByteArray,
)

internal interface DebugGattPairingObserver {
    fun onStatus(value: ByteArray)

    fun onDisconnected()

    fun onFailure()
}

internal fun interface DebugGattPairingConnection {
    fun close()
}

internal fun interface DebugGattPairingTransport {
    fun connect(
        request: DebugGattPairingRequest,
        observer: DebugGattPairingObserver,
    ): DebugGattPairingConnection
}

internal data class DebugPairingSession(
    val sessionId: String,
    val deviceName: String,
    val expiresAtEpochMillis: Long,
)

internal data class DebugPairedDevice(
    val id: String,
    val displayName: String,
)

internal data class DebugPairingSnapshot(
    val pending: DebugPairingSession?,
    val devices: List<DebugPairedDevice>,
)

internal class DebugPairingCoordinator(
    private val transport: DebugGattPairingTransport,
    private val nowEpochMillis: () -> ULong,
    private val associationId: () -> Int?,
    private val phoneIdentity: DebugPhoneIdentity,
) {
    private val monitor = Any()
    private val pairedDevices = linkedMapOf<String, DebugPairedDevice>()
    private val consumedSessions = mutableSetOf<String>()
    private var nextToken = 0L
    private var active: Transaction? = null

    fun begin(qrPayload: String): DebugPairingSession {
        val payload = try {
            PairingUriV1.decode(qrPayload)
        } catch (_: PairingUriFormatException) {
            throw DebugPairingException(DebugPairingFailure.INVALID_QR)
        }
        if (payload.expiresAtEpochMillis > Long.MAX_VALUE.toULong()) {
            throw DebugPairingException(DebugPairingFailure.INVALID_QR)
        }
        val now = nowEpochMillis()
        if (now >= payload.expiresAtEpochMillis) {
            throw DebugPairingException(DebugPairingFailure.QR_EXPIRED)
        }
        val currentAssociationId = associationId()
            ?: throw DebugPairingException(DebugPairingFailure.ASSOCIATION_UNAVAILABLE)
        val sessionId = payload.sessionId.toLowerHex()
        val macId = payload.macId.toLowerHex()
        val session = DebugPairingSession(
            sessionId = sessionId,
            deviceName = payload.macName,
            expiresAtEpochMillis = payload.expiresAtEpochMillis.toLong(),
        )
        val controlValue = (
            "RPD1|$sessionId|${phoneIdentity.deviceId}|${phoneIdentity.displayName}"
        ).toByteArray(StandardCharsets.UTF_8)
        val expectedAcceptedStatus =
            "ACCEPTED|$sessionId".toByteArray(StandardCharsets.US_ASCII)
        val transaction = synchronized(monitor) {
            expireLocked(now)
            if (consumedSessions.contains(sessionId)) {
                throw DebugPairingException(DebugPairingFailure.QR_ALREADY_USED)
            }
            if (active != null) throw DebugPairingException(DebugPairingFailure.BUSY)
            Transaction(
                token = ++nextToken,
                session = session,
                macId = macId,
                payload = payload,
                associationId = currentAssociationId,
                expiresAtEpochMillis = payload.expiresAtEpochMillis,
            ).also { active = it }
        }
        val observer = observer(transaction.token, transaction.session.sessionId)
        val connection = try {
            transport.connect(
                DebugGattPairingRequest(
                    currentAssociationId,
                    controlValue,
                    expectedAcceptedStatus,
                ),
                observer,
            )
        } catch (_: RuntimeException) {
            synchronized(monitor) {
                if (active?.token == transaction.token) active = null
            }
            throw DebugPairingException(DebugPairingFailure.TRANSPORT_UNAVAILABLE)
        }
        synchronized(monitor) {
            val current = active
            if (current?.token == transaction.token) {
                current.connection = connection
            } else {
                connection.closeSafely()
            }
        }
        return session
    }

    fun snapshot(): DebugPairingSnapshot = synchronized(monitor) {
        expireLocked(nowEpochMillis())
        DebugPairingSnapshot(
            pending = active?.session,
            devices = pairedDevices.values.toList(),
        )
    }

    fun confirm(sessionId: String) {
        synchronized(monitor) {
            val transaction = active
                ?: throw DebugPairingException(DebugPairingFailure.SESSION_MISMATCH)
            if (transaction.session.sessionId != sessionId) {
                throw DebugPairingException(DebugPairingFailure.SESSION_MISMATCH)
            }
            if (nowEpochMillis() >= transaction.expiresAtEpochMillis) {
                closeActiveLocked()
                throw DebugPairingException(DebugPairingFailure.QR_EXPIRED)
            }
            if (!transaction.accepted) {
                throw DebugPairingException(DebugPairingFailure.NOT_ACCEPTED)
            }
            pairedDevices[transaction.macId] = DebugPairedDevice(
                id = transaction.macId,
                displayName = transaction.session.deviceName,
            )
            DebugConsoleCredentials.register(transaction.payload, transaction.associationId)
            consumedSessions += transaction.session.sessionId
            closeActiveLocked()
        }
    }

    fun revoke(deviceId: String) {
        synchronized(monitor) {
            if (pairedDevices.remove(deviceId) == null) {
                throw DebugPairingException(DebugPairingFailure.DEVICE_NOT_FOUND)
            }
            DebugConsoleCredentials.remove(deviceId)
            if (active?.macId == deviceId) closeActiveLocked()
        }
    }

    fun disconnect() {
        synchronized(monitor) {
            closeActiveLocked()
        }
    }

    private fun observer(token: Long, sessionId: String): DebugGattPairingObserver =
        object : DebugGattPairingObserver {
            override fun onStatus(value: ByteArray) {
                synchronized(monitor) {
                    val transaction = active
                    if (transaction?.token != token) return
                    when (parseStatus(value, sessionId)) {
                        PairingStatus.WAITING -> Unit
                        PairingStatus.ACCEPTED -> transaction.accepted = true
                        PairingStatus.INVALID -> closeActiveLocked()
                    }
                }
            }

            override fun onDisconnected() {
                failIfCurrent(token)
            }

            override fun onFailure() {
                failIfCurrent(token)
            }
        }

    private fun failIfCurrent(token: Long) {
        synchronized(monitor) {
            if (active?.token == token) closeActiveLocked()
        }
    }

    private fun expireLocked(now: ULong) {
        if (active?.let { now >= it.expiresAtEpochMillis } == true) closeActiveLocked()
    }

    private fun closeActiveLocked() {
        val connection = active?.connection
        active = null
        connection?.closeSafely()
    }

    private class Transaction(
        val token: Long,
        val session: DebugPairingSession,
        val macId: String,
        val payload: PairingPayloadV1,
        val associationId: Int,
        val expiresAtEpochMillis: ULong,
        var accepted: Boolean = false,
        var connection: DebugGattPairingConnection? = null,
    )

    private enum class PairingStatus { WAITING, ACCEPTED, INVALID }

    private companion object {
        fun parseStatus(value: ByteArray, expectedSession: String): PairingStatus {
            val text = try {
                StandardCharsets.US_ASCII.newDecoder()
                    .onMalformedInput(CodingErrorAction.REPORT)
                    .onUnmappableCharacter(CodingErrorAction.REPORT)
                    .decode(ByteBuffer.wrap(value.copyOf()))
                    .toString()
            } catch (_: Exception) {
                return PairingStatus.INVALID
            }
            return when (text) {
                "WAITING|$expectedSession" -> PairingStatus.WAITING
                "ACCEPTED|$expectedSession" -> PairingStatus.ACCEPTED
                else -> PairingStatus.INVALID
            }
        }
    }
}

private fun ByteArray.toLowerHex(): String = joinToString(separator = "") { byte ->
    "%02x".format(byte.toInt() and 0xff)
}

private fun DebugGattPairingConnection.closeSafely() {
    try {
        close()
    } catch (_: RuntimeException) {
        // Closing is best effort after authority has already been removed.
    }
}
