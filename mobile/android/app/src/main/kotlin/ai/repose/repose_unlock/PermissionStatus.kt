package ai.repose.repose_unlock

internal fun permissionStatus(declared: Boolean, granted: Boolean, requested: Boolean, rationale: Boolean): String = when {
    !declared -> "notRequired"
    granted -> "granted"
    requested && !rationale -> "permanentlyDenied"
    else -> "denied"
}
