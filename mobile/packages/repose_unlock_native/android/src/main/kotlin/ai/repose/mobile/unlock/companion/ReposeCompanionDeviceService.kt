package ai.repose.mobile.unlock.companion

import android.annotation.TargetApi
import android.companion.CompanionDeviceService
import android.companion.DevicePresenceEvent
import ai.repose.mobile.unlock.ReposeUnlockRuntime

@TargetApi(36)
class ReposeCompanionDeviceService : CompanionDeviceService() {
    override fun onCreate() {
        super.onCreate()
        ReposeUnlockRuntime.initialize(applicationContext)
    }

    override fun onDevicePresenceEvent(event: DevicePresenceEvent) {
        ReposeUnlockRuntime.enqueuePresenceEvent(
            applicationContext,
            event.associationId,
            event.event,
        )
    }
}
