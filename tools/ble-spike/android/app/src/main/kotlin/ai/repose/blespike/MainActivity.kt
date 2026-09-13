package ai.repose.blespike

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.drawable.ColorDrawable
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.util.TypedValue
import android.view.Gravity
import android.view.View
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
import android.view.ViewGroup.LayoutParams.WRAP_CONTENT
import android.view.WindowInsetsController
import android.widget.FrameLayout
import android.widget.LinearLayout
import android.widget.TextView

/**
 * Host for the Outsie 手机钥匙 product shell. One Activity, four screens (配对、主屏、控制、
 * 量距离), a small manual navigator. No tab bar (design doc §03): 主屏 is the only destination, 控制 opens
 * from a computer's card, and everything else has a 「返回」. The advertise toggle on the home screen drives the
 * existing [BleSpikeService] — the product's "advertise on/off" IS start/stop advertising.
 */
class MainActivity : Activity(), Nav {

    private companion object {
        const val REQUEST_PERMISSIONS = 7
        /** From the 「让它一直在」 card: grant, then just redraw. Nothing starts. */
        const val REQUEST_PERMISSIONS_ONLY = 8
    }

    private lateinit var store: AppStore

    /**
     * Held by the activity, not the screen: the window it opens outlives a
     * rebuild (onResume rebuilds the screen), and a catalogue arriving during
     * one must not land in an object nobody is holding any more.
     */
    private val console by lazy { ConsoleServer(this) }
    private lateinit var contentFrame: FrameLayout

    private val main = Handler(Looper.getMainLooper())
    private var current: Screen = Screen.HOME
    private var currentView: ScreenView? = null

    private val onStateChanged: () -> Unit = { main.post { currentView?.onState?.invoke() } }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        store = AppStore(this)
        val pal = ReposeTheme.of(this)

        // Paint behind the system bars so there is no light flash in dark mode.
        window.setBackgroundDrawable(ColorDrawable(pal.background))

        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            fitsSystemWindows = true // API 35 forces edge-to-edge; this pads for the bars.
            setBackgroundColor(pal.background)
            layoutParams = LinearLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT)
        }

        contentFrame = FrameLayout(this).apply {
            layoutParams = LinearLayout.LayoutParams(MATCH_PARENT, 0, 1f)
        }

        root.addView(contentFrame)
        setContentView(root)
        applyBarIconContrast() // after setContentView: the decor view now exists.

        SpikeState.addListener(onStateChanged)

        go(if (PresenceKey.hasAny(this)) Screen.HOME else Screen.PAIRING)

        restoreAdvertisingIfWanted()

        // A test seam, not a feature: `am start ... --ez autostart true` begins
        // advertising without a human finding a toggle. The impersonation test has to
        // drive both this app and its imposter twin identically, and locating a button
        // by its on-screen label made the test depend on the wording of a UI that is
        // still being redesigned -- a relabelled control would have looked exactly like
        // a device that was correctly refused.
        //
        // It grants nothing new: MainActivity is the launcher activity, so anything
        // that can send this could already tap the toggle. Permissions are still asked
        // for the same way, so a first run still needs a human.
        maybeAutostart(intent)
    }

    // An already-running activity gets onNewIntent, not onCreate, so the seam
    // only worked on a cold start -- and every "it did not advertise" after that
    // looked like a radio problem rather than an intent that was never read.
    override fun onNewIntent(intent: Intent?) {
        super.onNewIntent(intent)
        setIntent(intent)
        maybeAutostart(intent)
    }

    private fun maybeAutostart(intent: Intent?) {
        if (intent?.getBooleanExtra("autostart", false) == true) {
            SpikeState.event("autostart requested (test seam)")
            requestPermissionsThenStart()
        }
        // The same seam, for a shortcut byte: `am start ... --ei command 16`.
        //
        // It exists because the Mac half of 快捷控制 has to be testable before
        // the phone has a catalogue to draw buttons from -- and because a test
        // that drives the radio by tapping a button whose label is still being
        // redesigned fails for the wrong reason.
        //
        // It grants nothing: anything that can send this could already tap the
        // buttons, and the command is still refused by the Mac unless the tag
        // verifies under a paired key.
        val cmd = intent?.getIntExtra("command", 0) ?: 0
        if (cmd >= SpikeContract.CMD_SHORTCUT_BASE) {
            android.util.Log.i(BleSpikeService.TAG, "test seam: command $cmd")
            SpikeState.event("command $cmd requested (test seam)")
            BleSpikeService.postCommand(this, cmd)
        }
    }

    /**
     * Rebuild the current screen when we come back to the front.
     *
     * Some of what the screens show is granted OUTSIDE this app -- the battery
     * exemption, and the permissions behind it. Android has no callback for
     * those. Without this, tapping 「去设置，别关掉它」, granting it, and coming
     * back leaves the warning still on screen, which reads as the grant having
     * failed (ui-conventions 2.3: an action that changes nothing on screen
     * invites you to do it again).
     */
    override fun onResume() {
        super.onResume()
        go(current)
    }

    override fun onDestroy() {
        // Leaving a connectable window open after the screen is gone would make
        // this phone reachable for a minute with nobody watching.
        console.stop()
        SpikeState.removeListener(onStateChanged)
        super.onDestroy()
    }

    // ---- Navigation ----

    override fun go(screen: Screen) {
        current = screen
        val pal = ReposeTheme.of(this)
        val view = when (screen) {
            Screen.PAIRING -> buildPairingScreen(this, store, this)
            Screen.HOME -> buildHomeScreen(this, store, this, { enable -> onAdvertiseChange(enable) }, ::requestKeyPermissions)
            Screen.CONTROL -> buildControlScreen(this, this, console, store)
            Screen.CAL -> buildCalScreen(this, this, store)
            Screen.KEEPALIVE -> buildKeepAliveScreen(this, store, this, ::requestKeyPermissions)
        }
        currentView = view
        contentFrame.removeAllViews()
        contentFrame.addView(
            view.root,
            FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT),
        )
    }

    @Deprecated("Deprecated in Java")
    override fun onBackPressed() {
        when (current) {
            Screen.CONTROL, Screen.CAL, Screen.KEEPALIVE -> go(Screen.HOME)
            else -> super.onBackPressed()
        }
    }

    // ---- Bottom navigation ----

    // ---- Advertise toggle -> BleSpikeService ----

    private fun onAdvertiseChange(enable: Boolean) {
        // Record the decision before acting on it. The service can be killed by
        // the system at any time; what must survive is that the user wanted
        // their phone to be a key.
        store.advertiseWanted = enable
        if (enable) requestPermissionsThenStart() else stopAdvertising()
    }

    /**
     * Put the beacon back if the user had it on.
     *
     * Android reclaims processes, and until now the toggle lived only in memory
     * -- so a phone that had been sitting in a pocket came back with its key
     * switched off and nothing on screen having changed its mind. The Mac just
     * went back to asking for a password, which is safe and completely silent.
     *
     * Only when the permissions are already granted: waking up and throwing a
     * permission dialog at someone who merely opened the app is worse than not
     * restarting, and the toggle then shows off, which is true.
     */
    private fun restoreAdvertisingIfWanted() {
        if (!store.advertiseWanted) return
        if (SpikeState.advertising) return
        if (missingPermissions().any { it != Manifest.permission.POST_NOTIFICATIONS &&
                it != Manifest.permission.BLUETOOTH_SCAN }) {
            SpikeState.event("was on, but a permission is missing; not restarting")
            return
        }
        SpikeState.event("restarting the beacon: it was on before")
        startSpike()
    }

    private fun stopAdvertising() {
        stopService(Intent(this, BleSpikeService::class.java))
        SpikeState.event("stop requested")
    }

    private fun missingPermissions(): List<String> {
        val wanted = mutableListOf(
            Manifest.permission.BLUETOOTH_ADVERTISE,
            Manifest.permission.BLUETOOTH_CONNECT,
            // For the Mac's own state beacon. Survivable if refused -- the
            // phone simply never learns whether the Mac is locked, and the
            // screen says so rather than guessing.
            Manifest.permission.BLUETOOTH_SCAN,
        )
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            wanted += Manifest.permission.POST_NOTIFICATIONS
        }
        return wanted.filter { checkSelfPermission(it) != PackageManager.PERMISSION_GRANTED }
    }

    private fun requestPermissionsThenStart() {
        val missing = missingPermissions()
        if (missing.isEmpty()) {
            startSpike()
        } else {
            SpikeState.event("requesting ${missing.size} permission(s)")
            requestPermissions(missing.toTypedArray(), REQUEST_PERMISSIONS)
        }
    }

    /** The checklist's 「允许」: the system dialog for whatever is still missing. */
    private fun requestKeyPermissions() {
        val missing = missingPermissions()
        if (missing.isEmpty()) return
        requestPermissions(missing.toTypedArray(), REQUEST_PERMISSIONS_ONLY)
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == REQUEST_PERMISSIONS_ONLY) {
            go(current) // the card reads the grants fresh
            return
        }
        if (requestCode != REQUEST_PERMISSIONS) return
        // POST_NOTIFICATIONS being denied is survivable; the BLE ones are not.
        val blocked = permissions.filterIndexed { index, name ->
            grantResults.getOrNull(index) != PackageManager.PERMISSION_GRANTED &&
                name != Manifest.permission.POST_NOTIFICATIONS &&
                // Refusing this costs the Mac-state sentence, not the feature.
                name != Manifest.permission.BLUETOOTH_SCAN
        }
        if (blocked.isEmpty()) {
            startSpike()
        } else {
            SpikeState.event("denied: ${blocked.joinToString()}")
            currentView?.onState?.invoke() // snap the toggle back off
        }
    }

    private fun startSpike() {
        startForegroundService(Intent(this, BleSpikeService::class.java))
        SpikeState.event("start requested")
    }

    // ---- Theming ----

    private fun applyBarIconContrast() {
        val light = !ReposeTheme.isNight(this)
        val controller = window.insetsController ?: return
        val mask = WindowInsetsController.APPEARANCE_LIGHT_STATUS_BARS or
            WindowInsetsController.APPEARANCE_LIGHT_NAVIGATION_BARS
        controller.setSystemBarsAppearance(if (light) mask else 0, mask)
    }
}
