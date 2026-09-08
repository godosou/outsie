package ai.repose.mobile.unlock.console

import ai.repose.mobile.unlock.protocol.PairingPayloadV1

internal class ConsoleCredential(val id: String, val name: String, val associationId: Int, val session: ByteArray, val secret: ByteArray) {
    fun wipe() { session.fill(0); secret.fill(0) }
}

/** Debug pairing authority, memory only. Never sent through MethodChannel. */
internal object DebugConsoleCredentials {
    private val entries = linkedMapOf<String, ConsoleCredential>()
    private val listeners = mutableSetOf<(String) -> Unit>()
    @Synchronized fun register(payload: PairingPayloadV1, associationId: Int) {
        val id = payload.macId.joinToString("") { "%02x".format(it.toInt() and 255) }
        remove(id)
        entries[id] = ConsoleCredential(id, payload.macName, associationId, payload.sessionId, payload.pairingSecret)
    }
    @Synchronized fun devices(): List<Map<String, String>> = entries.values.map { mapOf("id" to it.id, "name" to it.name) }
    @Synchronized fun get(id: String): ConsoleCredential? = entries[id]?.let { ConsoleCredential(it.id, it.name, it.associationId, it.session.copyOf(), it.secret.copyOf()) }
    @Synchronized fun remove(id: String) {
        entries.remove(id)?.wipe()
        listeners.toList().forEach { it(id) }
    }
    @Synchronized fun clear() { entries.keys.toList().forEach(::remove) }
    @Synchronized fun addListener(listener: (String) -> Unit) { listeners.add(listener) }
    @Synchronized fun removeListener(listener: (String) -> Unit) { listeners.remove(listener) }
}
