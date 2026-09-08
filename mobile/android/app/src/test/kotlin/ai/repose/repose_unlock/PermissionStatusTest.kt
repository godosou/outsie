package ai.repose.repose_unlock

import org.junit.Assert.assertEquals
import org.junit.Test

class PermissionStatusTest {
    @Test fun unusedPermissionsAreNeverPresentedAsDenied() {
        assertEquals("notRequired", permissionStatus(declared = false, granted = false, requested = true, rationale = false))
    }
    @Test fun firstRequestIsAvailableWithoutRationale() {
        assertEquals("denied", permissionStatus(declared = true, granted = false, requested = false, rationale = false))
    }
    @Test fun deniedButRetryablePermissionIsNotSentToSettings() {
        assertEquals("denied", permissionStatus(declared = true, granted = false, requested = true, rationale = true))
    }
    @Test fun permanentDenialRequiresSettings() {
        assertEquals("permanentlyDenied", permissionStatus(declared = true, granted = false, requested = true, rationale = false))
    }
    @Test fun grantOverridesPreviousDenial() {
        assertEquals("granted", permissionStatus(declared = true, granted = true, requested = true, rationale = false))
    }
}
