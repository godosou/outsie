package ai.repose.mobile.unlock.companion

const val DevicePresenceNoAssociation: Int = -1

enum class PresenceEventKind(val platformValue: Int) {
    BLE_APPEARED(0),
    BLE_DISAPPEARED(1),
    BT_CONNECTED(2),
    BT_DISCONNECTED(3),
    SELF_MANAGED_APPEARED(4),
    SELF_MANAGED_DISAPPEARED(5),
    ASSOCIATION_REMOVED(6),
    ;

    companion object {
        fun fromPlatformValue(value: Int): PresenceEventKind? = entries.firstOrNull {
            it.platformValue == value
        }
    }
}

data class PresenceTransition(
    val associationId: Int,
    val event: PresenceEventKind,
)

class PresenceEventRouter(
    private val activeAssociationId: () -> Int?,
    private val enqueue: (PresenceTransition) -> Unit,
    private val removeAssociation: (Int) -> Unit = {},
) {
    private val lock = Any()
    private var lastAccepted: PresenceTransition? = null

    fun route(associationId: Int, platformEvent: Int): Boolean = synchronized(lock) {
        val event = PresenceEventKind.fromPlatformValue(platformEvent) ?: return false
        val transition = PresenceTransition(associationId, event)
        if (
            associationId == DevicePresenceNoAssociation ||
            associationId != activeAssociationId()
        ) {
            return false
        }

        if (event == PresenceEventKind.ASSOCIATION_REMOVED) {
            removeAssociation(associationId)
            lastAccepted = null
            return true
        }

        if (transition == lastAccepted) return false
        enqueue(transition)
        lastAccepted = transition
        return true
    }
}
