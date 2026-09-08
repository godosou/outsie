package ai.repose.blespike

import android.Manifest
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.graphics.Typeface
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.Looper
import android.view.ViewGroup
import android.view.ViewGroup.LayoutParams.MATCH_PARENT
import android.widget.Button
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView

class MainActivity : Activity() {

    private companion object {
        const val REQUEST_PERMISSIONS = 7
        const val REFRESH_MS = 1_000L
    }

    private lateinit var status: TextView
    private val main = Handler(Looper.getMainLooper())
    private val onStateChanged: () -> Unit = { main.post { render() } }
    private val ticker = object : Runnable {
        override fun run() {
            render()
            main.postDelayed(this, REFRESH_MS)
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        status = TextView(this).apply {
            typeface = Typeface.MONOSPACE
            textSize = 12f
            setPadding(32, 32, 32, 32)
        }
        val start = Button(this).apply {
            text = "Start advertising"
            setOnClickListener { requestPermissionsThenStart() }
        }
        val stop = Button(this).apply {
            text = "Stop"
            setOnClickListener { stopService(Intent(context, BleSpikeService::class.java)) }
        }
        setContentView(
            LinearLayout(this).apply {
                orientation = LinearLayout.VERTICAL
                // API 35 enforces edge-to-edge; without this the buttons hide under the status bar.
                fitsSystemWindows = true
                addView(start, matchWidth())
                addView(stop, matchWidth())
                addView(ScrollView(context).apply { addView(status) }, matchWidth())
            },
        )
        SpikeState.addListener(onStateChanged)
    }

    private fun matchWidth() =
        LinearLayout.LayoutParams(MATCH_PARENT, ViewGroup.LayoutParams.WRAP_CONTENT)

    override fun onResume() { super.onResume(); main.post(ticker) }

    override fun onPause() { main.removeCallbacks(ticker); super.onPause() }

    override fun onDestroy() { SpikeState.removeListener(onStateChanged); super.onDestroy() }

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
        }
    }

    private fun startSpike() {
        startForegroundService(Intent(this, BleSpikeService::class.java))
        SpikeState.event("start requested")
    }

    private fun render() {
        status.text = SpikeState.render()
    }
}
