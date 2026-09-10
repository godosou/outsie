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
 * Host for the Repose 手机钥匙 product shell. One Activity, four screens, a small manual
 * navigator, and a two-item bottom nav. The advertise toggle on the home screen drives the
 * existing [BleSpikeService] — the product's "advertise on/off" IS start/stop advertising.
 */
class MainActivity : Activity(), Nav {

    private companion object {
        const val REQUEST_PERMISSIONS = 7
    }

    private lateinit var store: AppStore
    private lateinit var contentFrame: FrameLayout
    private lateinit var bottomNav: LinearLayout
    private lateinit var navHome: LinearLayout
    private lateinit var navMacs: LinearLayout

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
        bottomNav = buildBottomNav(pal)

        root.addView(contentFrame)
        root.addView(bottomNav)
        setContentView(root)
        applyBarIconContrast() // after setContentView: the decor view now exists.

        SpikeState.addListener(onStateChanged)

        go(if (store.paired) Screen.HOME else Screen.PAIRING)

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
        if (intent?.getBooleanExtra("autostart", false) == true) {
            SpikeState.event("autostart requested (test seam)")
            requestPermissionsThenStart()
        }
    }

    override fun onDestroy() {
        SpikeState.removeListener(onStateChanged)
        super.onDestroy()
    }

    // ---- Navigation ----

    override fun go(screen: Screen) {
        current = screen
        val pal = ReposeTheme.of(this)
        val view = when (screen) {
            Screen.PAIRING -> buildPairingScreen(this, store, this)
            Screen.HOME -> buildHomeScreen(this, store, this) { enable -> onAdvertiseChange(enable) }
            Screen.KEEPALIVE -> buildKeepAliveScreen(this, this)
            Screen.MACS -> buildMacsScreen(this, store, this)
        }
        currentView = view
        contentFrame.removeAllViews()
        contentFrame.addView(
            view.root,
            FrameLayout.LayoutParams(MATCH_PARENT, MATCH_PARENT),
        )
        val showNav = screen == Screen.HOME || screen == Screen.MACS
        bottomNav.visibility = if (showNav) View.VISIBLE else View.GONE
        setNavSelected(pal, screen)
    }

    override fun back() {
        when (current) {
            Screen.KEEPALIVE -> go(Screen.HOME)
            Screen.MACS -> go(Screen.HOME)
            else -> finish()
        }
    }

    @Deprecated("Deprecated in Java")
    override fun onBackPressed() {
        when (current) {
            Screen.KEEPALIVE, Screen.MACS -> go(Screen.HOME)
            else -> super.onBackPressed()
        }
    }

    // ---- Bottom navigation ----

    private fun buildBottomNav(pal: Palette): LinearLayout {
        val bar = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setBackgroundColor(pal.surface)
        }
        bar.addView(
            View(this).apply { setBackgroundColor(pal.divider) },
            LinearLayout.LayoutParams(MATCH_PARENT, dp(1)),
        )
        val row = LinearLayout(this).apply {
            orientation = LinearLayout.HORIZONTAL
        }
        navHome = navItem(pal, "🛡", "主屏") { go(Screen.HOME) }
        navMacs = navItem(pal, "🔑", "这把钥匙") { go(Screen.MACS) }
        row.addView(navHome, LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f))
        row.addView(navMacs, LinearLayout.LayoutParams(0, WRAP_CONTENT, 1f))
        bar.addView(row)
        return bar
    }

    private fun navItem(pal: Palette, glyph: String, label: String, onClick: () -> Unit): LinearLayout =
        LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER
            setPadding(0, dp(10), 0, dp(10))
            minimumHeight = dp(60)
            isClickable = true
            isFocusable = true
            setOnClickListener { onClick() }
            addView(
                TextView(context).apply {
                    text = glyph
                    gravity = Gravity.CENTER
                    setTextSize(TypedValue.COMPLEX_UNIT_SP, 20f)
                    tag = "glyph"
                },
            )
            addView(
                TextView(context).apply {
                    text = label
                    gravity = Gravity.CENTER
                    setTextSize(TypedValue.COMPLEX_UNIT_SP, 12f)
                    tag = "label"
                },
                LinearLayout.LayoutParams(WRAP_CONTENT, WRAP_CONTENT).apply { topMargin = dp(3) },
            )
        }

    private fun setNavSelected(pal: Palette, screen: Screen) {
        tint(navHome, if (screen == Screen.HOME) pal.accent else pal.textSecondary)
        tint(navMacs, if (screen == Screen.MACS) pal.accent else pal.textSecondary)
    }

    private fun tint(item: LinearLayout, color: Int) {
        (item.findViewWithTag<TextView>("glyph"))?.setTextColor(color)
        (item.findViewWithTag<TextView>("label"))?.setTextColor(color)
    }

    // ---- Advertise toggle -> BleSpikeService ----

    private fun onAdvertiseChange(enable: Boolean) {
        if (enable) requestPermissionsThenStart() else stopAdvertising()
    }

    private fun stopAdvertising() {
        stopService(Intent(this, BleSpikeService::class.java))
        SpikeState.event("stop requested")
    }

    private fun missingPermissions(): List<String> {
        val wanted = mutableListOf(
            Manifest.permission.BLUETOOTH_ADVERTISE,
            Manifest.permission.BLUETOOTH_CONNECT,
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

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode != REQUEST_PERMISSIONS) return
        // POST_NOTIFICATIONS being denied is survivable; the BLE ones are not.
        val blocked = permissions.filterIndexed { index, name ->
            grantResults.getOrNull(index) != PackageManager.PERMISSION_GRANTED &&
                name != Manifest.permission.POST_NOTIFICATIONS
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
