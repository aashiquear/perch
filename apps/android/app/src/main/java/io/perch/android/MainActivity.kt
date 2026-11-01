package io.perch.android

import android.app.Application
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.perch.android.host.GlassesService
import io.perch.android.host.MirrorSource
import io.perch.android.touchpad.PerchClient
import io.perch.android.touchpad.TouchpadSurface
import io.perch.android.ui.PerchTheme

class PerchApp : Application()

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
        val mp = MirrorSource.fromResult(this, r.resultCode, data) ?: return@registerForActivityResult
        // Hand the projection to the running service's presentation.
        GlassesService.addMirror(MirrorSource(this, mp))
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (Build.VERSION.SDK_INT >= 33) requestPermissions(arrayOf(android.Manifest.permission.POST_NOTIFICATIONS), 1)
        setContent {
            PerchTheme {
                var tab by rememberSaveable { mutableIntStateOf(0) }
                Scaffold(
                    bottomBar = {
                        NavigationBar {
                            NavigationBarItem(tab == 0, { tab = 0 }, icon = {}, label = { Text("Host") })
                            NavigationBarItem(tab == 1, { tab = 1 }, icon = {}, label = { Text("Touchpad") })
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

    @Composable
    private fun HostScreen() {
        val status by GlassesService.status.collectAsStateWithLifecycle()
        val hz by GlassesService.imuHz.collectAsStateWithLifecycle()
        val cal by GlassesService.calibrationState.collectAsStateWithLifecycle()
        val pose by GlassesService.pose.collectAsStateWithLifecycle()
        Column(Modifier.fillMaxSize().padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text("Glasses host", style = MaterialTheme.typography.headlineSmall)
            Text(status); Text("IMU $hz Hz · calibration ${calLabel(cal)}")
            Text("q = [%.2f %.2f %.2f %.2f]".format(pose[0], pose[1], pose[2], pose[3]), style = MaterialTheme.typography.bodySmall)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Button({ GlassesService.start(this@MainActivity) }) { Text("Start") }
                OutlinedButton({ GlassesService.start(this@MainActivity, useMock = true) }) { Text("Start (mock)") }
                OutlinedButton({ GlassesService.stop(this@MainActivity) }) { Text("Stop") }
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Button({ GlassesService.recenter() }) { Text("Recenter") }
                OutlinedButton({ GlassesService.calibrate(20f) }) { Text("Calibrate (20 s, still)") }
            }
            HorizontalDivider()
            Text("Windows", style = MaterialTheme.typography.titleMedium)
            Button({ projectionLauncher.launch(MirrorSource.requestIntent(this@MainActivity)) }) { Text("Add: mirror this phone's screen") }
            Text("DeX: enable Developer options → \"Force desktop mode\" and Perch will offer each app as its own window. See Settings help.", style = MaterialTheme.typography.bodySmall)
            HorizontalDivider()
            Text("Let Perch control this phone from the glasses", style = MaterialTheme.typography.titleMedium)
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                OutlinedButton({ startActivity(Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS)) }) { Text("Accessibility") }
                OutlinedButton({ startActivity(Intent(Settings.ACTION_INPUT_METHOD_SETTINGS)) }) { Text("Enable keyboard") }
            }
        }
    }

    private fun calLabel(c: Int) = when { c == 0 -> "none"; c == 200 -> "ready"; else -> "$c%" }

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
                Text("Connect to a Perch host", style = MaterialTheme.typography.headlineSmall)
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

private fun Modifier.clickable(onClick: () -> Unit): Modifier = androidx.compose.foundation.clickable(this, onClick = onClick)

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
