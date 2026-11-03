package io.perch.android.ui

import android.content.Context
import android.content.Intent
import android.graphics.drawable.Drawable
import android.os.SystemClock
import android.view.KeyEvent
import android.view.MotionEvent
import androidx.compose.foundation.Image
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.verticalScroll
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.rememberScrollState
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.*
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInteropFilter
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.core.graphics.drawable.toBitmap
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.perch.android.R
import io.perch.android.host.AppWindows
import io.perch.android.host.GlassesService
import io.perch.android.host.MirrorSource
import io.perch.android.shell.ShellBridge
import kotlin.math.abs

/**
 * The phone-side dashboard for phone-as-host: the Android twin of the Linux
 * control panel. Same information (glasses, stats, layout map), same
 * square icon buttons with the name beside them, plus what only the phone
 * needs: the app picker, portrait/landscape per window, the pop-up
 * touchpad and Shizuku status.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun PanelScreen(ctx: Context, onMirror: () -> Unit) {
    val status by GlassesService.status.collectAsStateWithLifecycle()
    val hz by GlassesService.imuHz.collectAsStateWithLifecycle()
    val cal by GlassesService.calibrationState.collectAsStateWithLifecycle()
    val attached by GlassesService.glassesAttached.collectAsStateWithLifecycle()
    val shellState by ShellBridge.state.collectAsStateWithLifecycle()
    val windows by AppWindows.windows.collectAsStateWithLifecycle()
    val focused by AppWindows.focused.collectAsStateWithLifecycle()
    val err by AppWindows.lastError.collectAsStateWithLifecycle()
    val crash by CrashLog.last.collectAsStateWithLifecycle()
    val routeHw by AppWindows.routeHardware.collectAsStateWithLifecycle()
    val displayNames by GlassesService.displayNames.collectAsStateWithLifecycle()
    var picker by remember { mutableStateOf(false) }
    var touchpad by remember { mutableStateOf(false) }
    var padMode by rememberSaveable { mutableIntStateOf(0) }   // 0 touchpad, 1 xbox, 2 wheel, 3 flight
    val bright by AppWindows.brightness.collectAsStateWithLifecycle()
    var arrange by remember { mutableStateOf(false) }

    LaunchedEffect(Unit) { ShellBridge.refresh() }

    Box(Modifier.fillMaxSize()) {
    LazyColumn(Modifier.fillMaxSize().padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        item { Spacer(Modifier.height(8.dp)) }
        if (crash != null) item {
            Card(colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.errorContainer)) {
                Column(Modifier.padding(12.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Text("Perch crashed last time", fontWeight = FontWeight.Bold)
                    Text(crash!!.lines().take(12).joinToString("\n"), style = MaterialTheme.typography.bodySmall, fontFamily = FontFamily.Monospace)
                    TextButton({ CrashLog.clear(ctx) }) { Text("Dismiss") }
                }
            }
        }
        // ---- header ----
        item {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                if (status.contains("Rokid", ignoreCase = true) || attached) {
                    Image(painterResource(R.drawable.rokid_max), contentDescription = "Rokid Max", modifier = Modifier.height(72.dp))
                }
                Column(Modifier.weight(1f)) {
                    Text("Perch", style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.Bold)
                    Text(status, style = MaterialTheme.typography.bodyMedium)
                    Text(
                        if (attached) "glasses display attached · IMU $hz Hz · ${calLabel(cal)}" else "no glasses display · IMU $hz Hz · ${calLabel(cal)}",
                        style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                    if (displayNames.isNotEmpty()) Text("displays: $displayNames", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                    Text(
                        if (touchpad) "● TOUCHPAD — moving the cursor on the glasses" else "○ PANEL",
                        color = if (touchpad) Color(0xFF5ADC8C) else Color(0xFFE6BE50), fontWeight = FontWeight.SemiBold,
                        style = MaterialTheme.typography.bodySmall,
                    )
                }
            }
        }
        // ---- stat tiles ----
        item {
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Stat(Modifier.weight(1f), windows.size.toString(), "windows")
                Stat(Modifier.weight(1f), hz.toString(), "IMU Hz")
                Stat(Modifier.weight(1f), when (shellState) {
                    ShellBridge.State.Ready -> "on"; ShellBridge.State.Binding -> "…"; else -> "off"
                }, "Shizuku")
            }
        }
        item { LayoutMap(windows.map { it.id to it.portrait }, focused) }
        // ---- actions: always two per row, no holes ----
        item {
            val themeMode by ThemeMode.mode.collectAsStateWithLifecycle()
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Action(Modifier.weight(1f), Icons.Filled.AddToQueue, "Add app", "to the glasses") { picker = true }
                    Action(Modifier.weight(1f), Icons.Filled.TouchApp, "Touchpad", "cursor in focused window", selected = touchpad) { padMode = 0; touchpad = true }
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Action(Modifier.weight(1f), Icons.Filled.SportsEsports, "Gamepad", "Xbox · wheel · flight") { padMode = if (padMode == 0) 1 else padMode; touchpad = true }
                    Action(Modifier.weight(1f), Icons.Filled.CenterFocusStrong, "Recenter", "temple button") { GlassesService.recenter() }
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Action(Modifier.weight(1f), Icons.Filled.Tune, "Calibrate", "20 s, glasses still") { GlassesService.calibrate(20f) }
                    Action(
                        Modifier.weight(1f),
                        when (themeMode) { 1 -> Icons.Filled.LightMode; 2 -> Icons.Filled.DarkMode; else -> Icons.Filled.BrightnessAuto },
                        "Theme", "${ThemeMode.label(themeMode)} · tap to change",
                    ) { ThemeMode.cycle(ctx) }
                }
                Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    Action(Modifier.weight(1f), Icons.Filled.GridView, if (arrange) "Arrange ▴" else "Arrange ▾", "presets", selected = arrange) { arrange = !arrange }
                    Action(Modifier.weight(1f), Icons.Filled.PhoneAndroid, "All back", "release every window") { AppWindows.releaseAll() }
                }
            }
        }
        if (arrange) item {
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                PresetPicture("1 × 1", listOf(0.46f)) { applyPreset("1 × 1") }
                PresetPicture("2 side by side", listOf(0.46f, 0.46f)) { applyPreset("2 side by side") }
                PresetPicture("wide centre", listOf(0.46f, 1.0f, 0.46f)) { applyPreset("wide centre") }
            }
        }
        item {
            // The entry point: larger, coloured, so it reads as "begin here".
            FlowRow(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(14.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
                if (GlassesService.tracker == 0L) {
                    Big(Icons.Filled.PlayArrow, "Start", "glasses on USB", Color(0xFF2E7D32), Color.White) { GlassesService.start(ctx) }
                    Big(Icons.Filled.Science, "Mock", "no glasses", Color(0xFF455A64), Color.White) { GlassesService.start(ctx, useMock = true) }
                } else {
                    Big(Icons.Filled.Stop, "Stop", "release everything", Color(0xFFC62828), Color.White) { GlassesService.stop(ctx) }
                }
                Big(Icons.Filled.ScreenShare, "Mirror", "phone screen as a window", Color(0xFF1565C0), Color.White, onMirror)
            }
        }
        item { BrightnessRow(bright) }
        item {
            Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                Switch(routeHw, { AppWindows.routeHardware.value = it })
                Column {
                    Text("Bluetooth keyboard · mouse · gamepad → glasses", fontWeight = FontWeight.SemiBold)
                    Text("while Perch is open, hardware input goes to the focused window; the mouse pointer is captured (hidden on the phone). Turn off to use the mouse here.", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
                }
            }
        }
        // ---- shizuku ----
        item {
            val (txt, action) = when (shellState) {
                ShellBridge.State.Ready -> "Shizuku helper connected (multi-app windows available)" to null
                ShellBridge.State.NeedPermission -> "Shizuku found — tap to allow Perch" to { ShellBridge.requestPermission() }
                ShellBridge.State.Binding -> "Connecting to Shizuku…" to null
                ShellBridge.State.Failed -> "Shizuku helper failed; see logcat" to { ShellBridge.refresh() }
                ShellBridge.State.NoShizuku -> "Shizuku not running: install it and start via Wireless debugging, then tap here" to { ShellBridge.refresh() }
            }
            Card(Modifier.fillMaxWidth().then(if (action != null) Modifier.clickable { action() } else Modifier)) {
                Text(txt, Modifier.padding(12.dp), style = MaterialTheme.typography.bodySmall)
            }
        }
        err?.let { item { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall) } }
        // ---- windows ----
        item { Text("Windows (${windows.size})", style = MaterialTheme.typography.titleMedium) }
        if (windows.isEmpty()) item { Text("None yet. Add app → it opens on its own glasses display.", style = MaterialTheme.typography.bodySmall) }
        items(windows, key = { it.id }) { w ->
            val isFocused = focused == w.id
            Card(
                Modifier.fillMaxWidth().clickable { AppWindows.focus(w.id) },
                colors = CardDefaults.cardColors(containerColor = if (isFocused) MaterialTheme.colorScheme.primaryContainer else MaterialTheme.colorScheme.surfaceVariant),
            ) {
                Row(Modifier.padding(10.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    AppIcon(w.icon)
                    Column(Modifier.weight(1f)) {
                        Text((if (isFocused) "● " else "○ ") + w.label, fontWeight = FontWeight.SemiBold)
                        Text(if (w.portrait) "portrait ${w.width}×${w.height}" else "landscape ${w.width}×${w.height}", style = MaterialTheme.typography.bodySmall)
                        if (w.secure) Text("blank on the glasses: this app blocks screen projection (FLAG_SECURE)", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error)
                    }
                    IconButton({ AppWindows.flip(w.id) }) { Icon(Icons.Filled.ScreenRotation, "portrait / landscape") }
                    IconButton({ AppWindows.pinHere(w.id) }) { Icon(Icons.Filled.PushPin, "pin where I look") }
                    IconButton({ AppWindows.remove(w.id) }) { Icon(Icons.Filled.Close, "close") }
                }
                if (isFocused) FlowRow(Modifier.fillMaxWidth().padding(start = 6.dp, end = 6.dp, bottom = 4.dp), horizontalArrangement = Arrangement.SpaceEvenly, verticalArrangement = Arrangement.spacedBy(6.dp)) {
                    Small(Icons.Filled.ZoomIn, "bigger") { AppWindows.bigger(w.id) }
                    Small(Icons.Filled.ZoomOut, "smaller") { AppWindows.smaller(w.id) }
                    Small(Icons.Filled.ArrowDownward, "nearer") { AppWindows.nearer(w.id) }
                    Small(Icons.Filled.ArrowUpward, "further") { AppWindows.further(w.id) }
                    Small(Icons.Filled.Refresh, "re-open") { AppWindows.relaunch(w.id) }
                }
            }
        }
        item { Spacer(Modifier.height(24.dp)) }
    }

    if (touchpad) TouchpadOverlay(padMode, { padMode = it }, onDismiss = { touchpad = false })
    }
    if (picker) AppPicker(ctx, onPick = { pkg, portrait -> AppWindows.add(ctx, pkg, portrait); picker = false }, onDismiss = { picker = false })
}

/** Large coloured entry-point button with caption and hint under it. */
@Composable
private fun Big(icon: ImageVector, label: String, hint: String, bg: Color, fg: Color, onClick: () -> Unit) {
    Column(Modifier.width(84.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        FilledIconButton(onClick, Modifier.size(72.dp), shape = RoundedCornerShape(18.dp),
            colors = IconButtonDefaults.filledIconButtonColors(containerColor = bg, contentColor = fg)) {
            Icon(icon, label, Modifier.size(40.dp))
        }
        Text(label, style = MaterialTheme.typography.labelLarge, fontWeight = FontWeight.SemiBold)
        Text(hint, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant, maxLines = 1)
    }
}

/** A preset as a picture: boxes with the given width/height ratios (portrait ≈ 0.46, landscape 1.0..). */
@Composable
private fun PresetPicture(name: String, ratios: List<Float>, onClick: () -> Unit) {
    Column(horizontalAlignment = Alignment.CenterHorizontally) {
        Box(Modifier.size(width = 84.dp, height = 48.dp).clip(RoundedCornerShape(10.dp)).background(MaterialTheme.colorScheme.surfaceVariant).clickable(onClick = onClick)) {
            Canvas(Modifier.fillMaxSize().padding(6.dp)) {
                val h = size.height
                val total = ratios.sumOf { (it * h).toDouble() }.toFloat() + 4f * (ratios.size - 1)
                var x = (size.width - total) / 2f
                for (r in ratios) {
                    val w = r * h
                    drawRoundRect(Color(0xFF7FA7D9), topLeft = androidx.compose.ui.geometry.Offset(x, 0f), size = androidx.compose.ui.geometry.Size(w, h), cornerRadius = androidx.compose.ui.geometry.CornerRadius(3f, 3f))
                    x += w + 4f
                }
            }
        }
        Text(name, style = MaterialTheme.typography.labelSmall)
    }
}

/** Software dimming of the glasses image; hardware brightness is the temple rocker. */
@Composable
private fun BrightnessRow(value: Float) {
    Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        Icon(Icons.Filled.BrightnessLow, "dim", Modifier.size(18.dp))
        Slider(value, { AppWindows.setBrightness(it) }, Modifier.weight(1f), valueRange = 0.15f..1f)
        Icon(Icons.Filled.BrightnessHigh, "bright", Modifier.size(18.dp))
        Text("${(value * 100).toInt()}%", style = MaterialTheme.typography.labelSmall, modifier = Modifier.width(36.dp))
    }
}

/** Round four-way pad sending DPAD key codes. */
@Composable
private fun DPad(modifier: Modifier, onKey: (Int) -> Unit) {
    Box(modifier.clip(androidx.compose.foundation.shape.CircleShape).background(MaterialTheme.colorScheme.surfaceVariant)) {
        @Composable
        fun Arm(align: Alignment, icon: ImageVector, code: Int, label: String) {
            IconButton({ onKey(code) }, Modifier.align(align).size(36.dp)) { Icon(icon, label) }
        }
        Arm(Alignment.TopCenter, Icons.Filled.KeyboardArrowUp, KeyEvent.KEYCODE_DPAD_UP, "up")
        Arm(Alignment.BottomCenter, Icons.Filled.KeyboardArrowDown, KeyEvent.KEYCODE_DPAD_DOWN, "down")
        Arm(Alignment.CenterStart, Icons.Filled.KeyboardArrowLeft, KeyEvent.KEYCODE_DPAD_LEFT, "left")
        Arm(Alignment.CenterEnd, Icons.Filled.KeyboardArrowRight, KeyEvent.KEYCODE_DPAD_RIGHT, "right")
        IconButton({ onKey(KeyEvent.KEYCODE_DPAD_CENTER) }, Modifier.align(Alignment.Center).size(30.dp)) { Icon(Icons.Filled.RadioButtonChecked, "enter", Modifier.size(16.dp)) }
    }
}

/** Compact icon button with a one-word caption under it. */
@Composable
private fun Small(icon: ImageVector, label: String, onClick: () -> Unit) {
    // Fixed width so a row of these wraps cleanly on narrow phones instead of
    // squeezing the caption into a column of single characters.
    Column(Modifier.width(64.dp), horizontalAlignment = Alignment.CenterHorizontally) {
        FilledTonalIconButton(onClick, Modifier.size(40.dp)) { Icon(icon, label) }
        Text(label, style = MaterialTheme.typography.labelSmall, maxLines = 1, softWrap = false)
    }
}

private fun calLabel(c: Int) = when { c == 0 -> "not calibrated"; c == 200 -> "calibrated"; else -> "calibrating $c%" }

private fun applyPreset(name: String) {
    // Presets on the phone are about aspect: portrait apps side by side, or a
    // landscape one in the middle. Spatial placement is re-flowed by index.
    val list = AppWindows.windows.value
    when (name) {
        "wide centre" -> list.getOrNull(1)?.let { if (it.portrait) AppWindows.flip(it.id) }
        else -> list.forEach { if (!it.portrait) AppWindows.flip(it.id) }
    }
}

@Composable
private fun Stat(modifier: Modifier, value: String, caption: String) {
    Card(modifier) {
        Column(Modifier.fillMaxWidth().padding(vertical = 8.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            Text(value, style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold)
            Text(caption, style = MaterialTheme.typography.labelSmall)
        }
    }
}

/** Square icon button, name + hint beside it (the Linux panel's layout). */
@Composable
private fun Action(modifier: Modifier, icon: ImageVector, label: String, hint: String, selected: Boolean = false, onClick: () -> Unit) {
    Row(modifier, verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        FilledTonalIconButton(
            onClick, Modifier.size(56.dp), shape = RoundedCornerShape(12.dp),
            colors = if (selected) IconButtonDefaults.filledTonalIconButtonColors(containerColor = MaterialTheme.colorScheme.primary, contentColor = MaterialTheme.colorScheme.onPrimary)
            else IconButtonDefaults.filledTonalIconButtonColors(),
        ) { Icon(icon, label, Modifier.size(28.dp)) }
        Column {
            Text(label, style = MaterialTheme.typography.bodyLarge)
            Text(hint, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

@Composable
private fun AppIcon(d: Drawable?) {
    if (d == null) { Icon(Icons.Filled.Apps, null, Modifier.size(36.dp)); return }
    val bmp = remember(d) { d.toBitmap(96, 96).asImageBitmap() }
    Image(bmp, null, Modifier.size(36.dp).clip(RoundedCornerShape(8.dp)))
}

/** A row of boxes in window order; portrait ones narrow, the focused one bright. */
@Composable
private fun LayoutMap(wins: List<Pair<Int, Boolean>>, focused: Int?) {
    Box(Modifier.fillMaxWidth().height(90.dp).clip(RoundedCornerShape(10.dp)).background(Color(0xFF12141A)), contentAlignment = Alignment.Center) {
        if (wins.isEmpty()) Text("no windows on the glasses", color = Color(0xFF6E7380), style = MaterialTheme.typography.bodySmall)
        Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
            wins.forEachIndexed { i, (id, portrait) ->
                Box(
                    Modifier.size(width = if (portrait) 32.dp else 68.dp, height = if (portrait) 68.dp else 32.dp)
                        .clip(RoundedCornerShape(4.dp))
                        .background(if (id == focused) Color(0xFF468CC8) else Color(0xFF3C465A)),
                    contentAlignment = Alignment.Center,
                ) { Text((i + 1).toString(), color = Color.White, style = MaterialTheme.typography.labelMedium) }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// App picker
// ---------------------------------------------------------------------------

private data class LaunchableApp(val pkg: String, val label: String, val icon: Drawable)

@Composable
private fun AppPicker(ctx: Context, onPick: (String, Boolean) -> Unit, onDismiss: () -> Unit) {
    val pm = ctx.packageManager
    val apps = remember {
        val intent = Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER)
        pm.queryIntentActivities(intent, 0)
            .map { LaunchableApp(it.activityInfo.packageName, it.loadLabel(pm).toString(), it.loadIcon(pm)) }
            .filter { it.pkg != ctx.packageName }
            .distinctBy { it.pkg }
            .sortedBy { it.label.lowercase() }
    }
    var query by remember { mutableStateOf("") }
    var portrait by remember { mutableStateOf(true) }
    AlertDialog(
        onDismissRequest = onDismiss,
        confirmButton = { TextButton(onDismiss) { Text("Cancel") } },
        title = { Text("Open on the glasses") },
        text = {
            Column(Modifier.fillMaxWidth().height(460.dp)) {
                OutlinedTextField(query, { query = it }, Modifier.fillMaxWidth(), singleLine = true, placeholder = { Text("search apps") }, keyboardOptions = KeyboardOptions.Default)
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text("Portrait", style = MaterialTheme.typography.bodySmall)
                    Switch(portrait, { portrait = it }, Modifier.padding(horizontal = 8.dp))
                    Text(if (portrait) "phone-shaped window" else "landscape window", style = MaterialTheme.typography.bodySmall)
                }
                LazyColumn(Modifier.weight(1f)) {
                    items(apps.filter { query.isBlank() || it.label.contains(query, ignoreCase = true) }, key = { it.pkg }) { a ->
                        ListItem(
                            headlineContent = { Text(a.label) },
                            supportingContent = { Text(a.pkg, style = MaterialTheme.typography.labelSmall) },
                            leadingContent = { AppIcon(a.icon) },
                            modifier = Modifier.clickable { onPick(a.pkg, portrait) },
                        )
                    }
                }
            }
        },
    )
}

// ---------------------------------------------------------------------------
// Pop-up touchpad
// ---------------------------------------------------------------------------

/**
 * One finger moves the cursor (with acceleration), tap = tap, double-tap and
 * hold = drag, two fingers = scroll, three-finger swipe = next/previous
 * window. The phone is the trackpad; the cursor is drawn on the glasses.
 */
/**
 * Docked at the bottom of the panel, not a ModalBottomSheet: the sheet's own
 * drag-to-dismiss swallowed vertical finger movement before the pad saw it.
 */
@OptIn(ExperimentalComposeUiApi::class, ExperimentalLayoutApi::class)
@Composable
private fun BoxScope.TouchpadOverlay(mode: Int, onMode: (Int) -> Unit, onDismiss: () -> Unit) {
    val focused by AppWindows.focused.collectAsStateWithLifecycle()
    val windows by AppWindows.windows.collectAsStateWithLifecycle()
    // Scrim: tap outside the pad to close.
    Box(Modifier.fillMaxSize().background(Color.Black.copy(alpha = 0.35f)).clickable(onClick = onDismiss))
    val maxH = (LocalConfiguration.current.screenHeightDp * 0.88f).dp
    Surface(
        Modifier.align(Alignment.BottomCenter).fillMaxWidth().heightIn(max = maxH),
        shape = RoundedCornerShape(topStart = 24.dp, topEnd = 24.dp), tonalElevation = 3.dp, shadowElevation = 8.dp,
    ) {
        // Scrolls when a skin plus the bars is taller than the screen (small phones).
        Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(horizontal = 16.dp).padding(top = 12.dp, bottom = 24.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                Row(Modifier.weight(1f).horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    windows.forEach { w ->
                        FilterChip(selected = focused == w.id, onClick = { AppWindows.focus(w.id) }, label = { Text(w.label) })
                    }
                }
                IconButton(onDismiss) { Icon(Icons.Filled.Close, "close touchpad") }
            }
            // One pop-up, four input surfaces; controls, d-pad and dimming stay the same.
            Row(Modifier.fillMaxWidth().horizontalScroll(rememberScrollState()), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                listOf("Touchpad", "Xbox", "Racing wheel", "Flight stick").forEachIndexed { i, n ->
                    FilterChip(selected = mode == i, onClick = { onMode(i) }, label = { Text(n) })
                }
            }
            if (mode != 0) LaunchedEffect(mode) { AppWindows.gamepadAnnounce() }
            val gpErr by AppWindows.gamepadError.collectAsStateWithLifecycle()
            val gpInfo by AppWindows.gamepadInfo.collectAsStateWithLifecycle()
            if (mode != 0) {
                gpErr?.let { Text("Controller identity unavailable ($it); games that insist on a real gamepad may ignore input.", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error) }
                Text(gpInfo, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            val gestures = remember { PhoneTouchpad() }
            var dragLock by remember { mutableStateOf(false) }
            gestures.dragLock = dragLock
            // Pad height follows the screen: ~34 % of it, so the bars below stay reachable.
            val padH = (LocalConfiguration.current.screenHeightDp * 0.34f).dp.coerceIn(180.dp, 360.dp)
            val edge = with(LocalDensity.current) { 22.dp.toPx() }
            gestures.edgePx = edge
            Box(Modifier.fillMaxWidth().then(if (mode == 0) Modifier.height(padH) else Modifier.heightIn(min = padH))) {
                if (mode != 0) GamepadSkin(mode)
                else Box(
                    Modifier.fillMaxSize().clip(RoundedCornerShape(16.dp))
                        .background(MaterialTheme.colorScheme.surfaceVariant)
                        // Thin rim: a stroke that starts here is a drag (press held while you slide).
                        .border(22.dp, if (dragLock) Color(0x553F51B5) else Color(0x332196F3), RoundedCornerShape(16.dp))
                        .onSizeChanged { gestures.padW = it.width.toFloat(); gestures.padH = it.height.toFloat() }
                        .pointerInteropFilter { gestures.onTouch(it) },
                    contentAlignment = Alignment.Center,
                ) {
                    Text(
                        if (dragLock) "DRAG MODE: slide = drag with the button held\ntap the lock to go back" else
                        "1 finger: move · tap · double-tap+hold: drag\nstart on the rim: drag · 2 fingers: scroll · 3-finger swipe: next window",
                        style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant,
                        textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                    )
                    // Lock in the corner toggles drag mode for every stroke.
                    FilledTonalIconButton({ dragLock = !dragLock }, Modifier.align(Alignment.TopEnd).padding(6.dp).size(36.dp),
                        colors = if (dragLock) IconButtonDefaults.filledTonalIconButtonColors(containerColor = MaterialTheme.colorScheme.primary, contentColor = MaterialTheme.colorScheme.onPrimary) else IconButtonDefaults.filledTonalIconButtonColors()) {
                        Icon(if (dragLock) Icons.Filled.Lock else Icons.Filled.LockOpen, "drag lock", Modifier.size(18.dp))
                    }
                }
            }
            val b by AppWindows.brightness.collectAsStateWithLifecycle()
            BrightnessRow(b)
            // Controls wrap on the left; the d-pad keeps its place on the right at any width.
            val fid = focused
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            FlowRow(Modifier.weight(1f), horizontalArrangement = Arrangement.SpaceEvenly, verticalArrangement = Arrangement.spacedBy(6.dp)) {
                Small(Icons.AutoMirrored.Filled.ArrowBack, "back") { AppWindows.key(KeyEvent.KEYCODE_BACK) }
                Small(Icons.Filled.SkipNext, "next window") { AppWindows.focusNext(1) }
                Small(Icons.Filled.PushPin, "pin here") { fid?.let { AppWindows.pinHere(it) } }
                Small(Icons.Filled.ScreenRotation, "rotate") { fid?.let { AppWindows.flip(it) } }
                Small(Icons.Filled.ZoomIn, "bigger") { fid?.let { AppWindows.bigger(it) } }
                Small(Icons.Filled.ZoomOut, "smaller") { fid?.let { AppWindows.smaller(it) } }
                Small(Icons.Filled.ArrowDownward, "nearer") { fid?.let { AppWindows.nearer(it) } }
                Small(Icons.Filled.ArrowUpward, "further") { fid?.let { AppWindows.further(it) } }
                Small(Icons.Filled.Refresh, "re-open app") { fid?.let { AppWindows.relaunch(it) } }
            }
            // Arrow keys for suggestion strips / menus that need a keyboard cursor.
            DPad(Modifier.size(112.dp)) { AppWindows.arrow(it) }
            }
        }
    }
}

private class PhoneTouchpad {
    /** Every stroke is a drag (corner lock). */
    @Volatile var dragLock = false
    /** Strokes starting within this many px of the pad edge are drags. */
    @Volatile var edgePx = 0f
    private var lastX = 0f; private var lastY = 0f
    private var downT = 0L; private var lastTapT = 0L
    private var maxFingers = 0; private var moved = false; private var dragging = false
    private var scrollAcc = 0f; private var scrollAccX = 0f; private var threeStartX = 0f
    private val gain = 2.2f

    /** Pad size in px, from onSizeChanged. */
    @Volatile var padW = 0f
    @Volatile var padH = 0f

    fun onTouch(e: MotionEvent): Boolean {
        when (e.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                lastX = e.x; lastY = e.y; downT = SystemClock.uptimeMillis(); maxFingers = 1; moved = false; scrollAcc = 0f; scrollAccX = 0f
                val onRim = edgePx > 0f && (e.x < edgePx || e.y < edgePx || e.x > padW - edgePx || e.y > padH - edgePx)
                if (dragLock || onRim || downT - lastTapT < 250) { dragging = true; AppWindows.pressDown() }
            }
            MotionEvent.ACTION_POINTER_DOWN -> {
                maxFingers = maxOf(maxFingers, e.pointerCount)
                if (e.pointerCount == 3) threeStartX = e.getX(0)
                lastX = e.getX(0); lastY = e.getY(0)
            }
            MotionEvent.ACTION_MOVE -> {
                val dx = e.getX(0) - lastX; val dy = e.getY(0) - lastY
                lastX = e.getX(0); lastY = e.getY(0)
                if (abs(dx) + abs(dy) > 2f) moved = true
                when (e.pointerCount) {
                    1 -> { val a = 1f + (abs(dx) + abs(dy)) / 40f; AppWindows.cursorMove(dx * gain * a, dy * gain * a) }
                    2 -> {
                        scrollAcc += dy; scrollAccX += dx
                        if (abs(scrollAcc) > 24f) { AppWindows.scroll(0f, if (scrollAcc > 0) 1f else -1f); scrollAcc = 0f }
                        if (abs(scrollAccX) > 24f) { AppWindows.scroll(if (scrollAccX > 0) 1f else -1f, 0f); scrollAccX = 0f }
                    }
                }
            }
            MotionEvent.ACTION_UP -> {
                val now = SystemClock.uptimeMillis()
                if (dragging) { dragging = false; AppWindows.pressUp() }
                else if (!moved && now - downT < 220) {
                    when (maxFingers) {
                        1 -> { AppWindows.tap(); lastTapT = now }
                        2 -> AppWindows.key(KeyEvent.KEYCODE_BACK)
                    }
                } else if (maxFingers == 3 && abs(e.getX(0) - threeStartX) > 120f) {
                    AppWindows.focusNext(if (e.getX(0) > threeStartX) 1 else -1)
                }
            }
        }
        return true
    }
}
