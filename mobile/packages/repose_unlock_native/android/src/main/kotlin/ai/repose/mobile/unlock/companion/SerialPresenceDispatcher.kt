package ai.repose.mobile.unlock.companion

internal class SerialPresenceDispatcher(
    private val execute: (() -> Unit) -> Unit,
    reconcileAtStartup: () -> Unit,
    private val route: (associationId: Int, event: Int) -> Unit,
) {
    init {
        execute(reconcileAtStartup)
    }

    fun enqueue(associationId: Int, event: Int) {
        execute { route(associationId, event) }
    }

    fun executeControl(task: () -> Unit) {
        execute(task)
    }
}
