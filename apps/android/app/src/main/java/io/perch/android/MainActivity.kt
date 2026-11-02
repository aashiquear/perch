package io.perch.android

import android.app.Application
import android.os.Build
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Computer
import androidx.compose.material.icons.filled.Visibility
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.lifecycleScope
import kotlinx.coroutines.flow.combine
import kotlinx.coroutines.launch
import io.perch.android.host.AppWindows
import io.perch.android.host.GlassesService
import io.perch.android.host.MirrorSource
import io.perch.android.ui.CrashLog
import io.perch.android.touchpad.PerchClient
import io.perch.android.touchpad.TouchpadSurface
import io.perch.android.shell.ShellBridge
import io.perch.android.ui.PanelScreen
import io.perch.android.ui.PerchTheme

class PerchApp : Application() {
    override fun onCreate() {
        super.onCreate()
        CrashLog.install(this)
    }
}

/**
 * Two jobs, one app:
 *
 *  **Host** — the phone drives the glasses itself (USB-C: DP video + IMU).
 *  Your phone screen (or DeX) becomes a pinned 3DOF window. Pocket the phone,
 *  use a Bluetooth keyboard/mouse, recenter with the temple button.
 *
 *  **Touchpad** — the phone is a controller for a laptop running `perchd`:
 *  trackpad, remote keyboard, and window management (pin, focus, presets).
 */
class MainActivity : ComponentActivity() {

    private val client by lazy { PerchClient(this, Build.MODEL) }

    private val projectionLauncher = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { r ->
        val data = r.data ?: return@registerForActivityResult
        // The service promotes itself to a mediaProjection foreground service, then
        // creates the projection (Android 14+ requires exactly that order).
        GlassesService.startMirror(this, r.resultCode, data)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (Build.VERSION.SDK_INT >= 33) requestPermissions(arrayOf(android.Manifest.permission.POST_NOTIFICATIONS), 1)
        ShellBridge.install()
        window.decorView.setOnCapturedPointerListener { _, ev -> AppWindows.capturedMotion(ev) }
        lifecycleScope.launch {
            combine(AppWindows.routeHardware, AppWindows.focused) { r, f -> r to f }.collect { updatePointerCapture() }
        }
        setContent {
            PerchTheme {
                var tab by rememberSaveable { mutableIntStateOf(0) }
                Scaffold(
                    bottomBar = {
                        NavigationBar {
                            NavigationBarItem(tab == 0, { tab = 0 }, icon = { Icon(Icons.Filled.Visibility, null) }, label = { Text("Glasses") })
                            NavigationBarItem(tab == 1, { tab = 1 }, icon = { Icon(Icons.Filled.Computer, null) }, label = { Text("Laptop remote") })
                        }
                    },
                ) { pad ->
                    Box(Modifier.padding(pad)) {
                        if (tab == 0) HostScreen() else TouchpadScreen(client)
                    }
                }
            }
        }
    }

    // Bluetooth keyboard / mouse / gamepad: while Perch is in front and a glasses
    // window is focused, send them there instead of to the panel.
    override fun dispatchKeyEvent(event: android.view.KeyEvent): Boolean =
        AppWindows.hardwareKey(event) || super.dispatchKeyEvent(event)

    override fun dispatchGenericMotionEvent(ev: android.view.MotionEvent): Boolean =
        AppWindows.hardwareMotion(ev) || super.dispatchGenericMotionEvent(ev)

    // Mouse: pointer capture hides the phone's pointer and hands us relative motion,
    // so a Bluetooth mouse drives only the glasses cursor and never clicks the panel.
    private fun updatePointerCapture() {
        val want = AppWindows.routeHardware.value && AppWindows.focused.value != null && hasWindowFocus()
        val v = window.decorView
        if (want && !v.hasPointerCapture()) v.requestPointerCapture()
        else if (!want && v.hasPointerCapture()) v.releasePointerCapture()
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        updatePointerCapture()
    }

    @Composable
    private fun HostScreen() {
        PanelScreen(this) { projectionLauncher.launch(MirrorSource.requestIntent(this@MainActivity)) }
    }

    @Composable
    private fun TouchpadScreen(client: PerchClient) {
        val hosts by client.hosts.collectAsStateWithLifecycle()
        val connected by client.connected.collectAsStateWithLifecycle()
        val hostName by client.hostName.collectAsStateWithLifecycle()
        val windows by client.windows.collectAsStateWithLifecycle()
        val err by client.lastError.collectAsStateWithLifecycle()
        var manual by rememberSaveable { mutableStateOf("") }
        DisposableEffect(Unit) { client.startDiscovery(); onDispose { client.stopDiscovery() } }

        if (!connected) {
            Column(Modifier.fillMaxSize().padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
                Text("Laptop remote", style = MaterialTheme.typography.headlineSmall)
                Text("Use the phone as a trackpad and keyboard for perchd on your laptop. For the glasses-on-phone touchpad, use the Glasses tab → Touchpad button.", style = MaterialTheme.typography.bodyMedium)
                if (hosts.isEmpty()) Text("Looking for perchd on this Wi-Fi… (run `perchd` on your laptop)")
                LazyColumn { items(hosts) { h -> ListItem(headlineContent = { Text(h.name) }, supportingContent = { Text("${h.host}:${h.port}") }, modifier = Modifier.clickable { client.connect(h.host, h.port) }) } }
                OutlinedTextField(manual, { manual = it }, label = { Text("or host IP") }, singleLine = true)
                Button({ if (manual.isNotBlank()) client.connect(manual.trim()) }) { Text("Connect") }
                err?.let { Text(it, color = MaterialTheme.colorScheme.error) }
            }
            return
        }
        Column(Modifier.fillMaxSize().padding(12.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.SpaceBetween, modifier = Modifier.fillMaxWidth()) {
                Text(hostName, style = MaterialTheme.typography.titleMedium)
                TextButton({ client.disconnect() }) { Text("Disconnect") }
            }
            // Window chips: tap = focus, long-press = pin here.
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                windows.filter { it.visible }.forEach { w ->
                    FilterChip(selected = w.focused, onClick = { client.focus(w.id) }, label = { Text(w.label) })
                }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                Button({ client.recenter() }) { Text("Recenter") }
                OutlinedButton({ windows.firstOrNull { it.focused }?.let { client.pinHere(it.id) } }) { Text("Pin here") }
                OutlinedButton({ client.focusNext() }) { Text("Next") }
                var presets by remember { mutableStateOf(false) }
                Box {
                    OutlinedButton({ presets = true }) { Text("Layout") }
                    DropdownMenu(presets, { presets = false }) {
                        listOf("single", "coding", "gaming", "couch").forEach { p -> DropdownMenuItem({ Text(p) }, { client.preset(p); presets = false }) }
                    }
                }
            }
            TouchpadSurface(client, Modifier.weight(1f))
            RemoteKeyboardBar(client)
        }
    }
}


@Composable
private fun RemoteKeyboardBar(client: PerchClient) {
    var text by remember { mutableStateOf("") }
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
        OutlinedTextField(
            text, { new ->
                // Send only the delta; the host types it where the focus is.
                if (new.length > text.length) {
                    val added = new.substring(text.length)
                    client.sendInput(io.perch.android.touchpad.Postcard.input(added.map { io.perch.android.touchpad.Postcard.Event.Text(it.toString()) }))
                } else if (new.length < text.length) {
                    client.sendInput(io.perch.android.touchpad.Postcard.input(listOf(
                        io.perch.android.touchpad.Postcard.Event.Key(0x2a, true, 0), io.perch.android.touchpad.Postcard.Event.Key(0x2a, false, 0))))
                }
                text = if (new.length > 200) "" else new
            },
            modifier = Modifier.weight(1f), singleLine = true, placeholder = { Text("type here → focused window") },
        )
        FilledTonalButton({ client.sendInput(io.perch.android.touchpad.Postcard.input(listOf(
            io.perch.android.touchpad.Postcard.Event.Key(0x28, true, 0), io.perch.android.touchpad.Postcard.Event.Key(0x28, false, 0)))) }) { Text("⏎") }
    }
}
