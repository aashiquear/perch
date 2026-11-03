package io.perch.android.ui

import android.view.KeyEvent
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectDragGestures
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Close
import androidx.compose.material3.*
import androidx.compose.runtime.*
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalConfiguration
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import io.perch.android.host.AppWindows
import kotlin.math.atan2
import kotlin.math.cos
import kotlin.math.hypot
import kotlin.math.sin

/**
 * Pop-up controller. Three skins over one virtual gamepad (the game sees an
 * Xbox-like controller): Xbox layout, racing wheel (wheel → left stick X,
 * pedals → triggers, paddles → bumpers), flight stick (stick → left stick,
 * twist → right stick X, throttle → right stick Y).
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun BoxScope.GamepadOverlay(onDismiss: () -> Unit) {
    var skin by rememberSaveable { mutableIntStateOf(0) }
    val err by AppWindows.gamepadError.collectAsStateWithLifecycle()
    val bright by AppWindows.brightness.collectAsStateWithLifecycle()
    Box(Modifier.fillMaxSize().background(Color.Black.copy(alpha = 0.35f)).clickable(onClick = onDismiss))
    Surface(
        Modifier.align(Alignment.BottomCenter).fillMaxWidth(),
        shape = RoundedCornerShape(topStart = 24.dp, topEnd = 24.dp), tonalElevation = 3.dp, shadowElevation = 8.dp,
    ) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 12.dp).padding(top = 10.dp, bottom = 20.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Row(Modifier.weight(1f), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                    listOf("Xbox", "Racing wheel", "Flight stick").forEachIndexed { i, n ->
                        FilterChip(selected = skin == i, onClick = { skin = i }, label = { Text(n) })
                    }
                }
                IconButton(onDismiss) { Icon(Icons.Filled.Close, "close gamepad") }
            }
            err?.let { Text("Controller identity unavailable ($it); games that insist on a real gamepad may ignore input.", style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.error) }
            val h = (LocalConfiguration.current.screenHeightDp * 0.42f).dp.coerceIn(260.dp, 420.dp)
            Box(Modifier.fillMaxWidth().height(h)) {
                when (skin) { 0 -> XboxSkin(); 1 -> WheelSkin(); else -> FlightSkin() }
            }
            Row(Modifier.fillMaxWidth(), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Text("Glasses dim", style = MaterialTheme.typography.labelSmall)
                Slider(bright, { AppWindows.setBrightness(it) }, Modifier.weight(1f), valueRange = 0.15f..1f)
                Text("${(bright * 100).toInt()}%", style = MaterialTheme.typography.labelSmall)
            }
        }
    }
}

// ---- axis helpers ------------------------------------------------------------

private fun setAxis(i: Int, v: Float) { AppWindows.axes[i] = v.coerceIn(-1f, 1f); AppWindows.gamepadAxes() }
private fun setAxes(vararg iv: Pair<Int, Float>) { for ((i, v) in iv) AppWindows.axes[i] = v.coerceIn(-1f, 1f); AppWindows.gamepadAxes(force = true) }

/** Round stick; emits normalised (x, y) with y positive = down (Android convention). */
@Composable
private fun Stick(size: Dp, label: String, onMove: (Float, Float) -> Unit) {
    var knob by remember { mutableStateOf(Offset.Zero) }
    Box(
        Modifier.size(size).clip(CircleShape).background(MaterialTheme.colorScheme.surfaceVariant)
            .pointerInput(Unit) {
                val r = this.size.width / 2f
                detectDragGestures(
                    onDragStart = { knob = Offset.Zero },
                    onDragEnd = { knob = Offset.Zero; onMove(0f, 0f) },
                    onDragCancel = { knob = Offset.Zero; onMove(0f, 0f) },
                ) { change, drag ->
                    change.consume()
                    var k = knob + drag
                    val d = hypot(k.x, k.y)
                    if (d > r) k = Offset(k.x / d * r, k.y / d * r)
                    knob = k
                    onMove(k.x / r, k.y / r)
                }
            },
        contentAlignment = Alignment.Center,
    ) {
        Text(label, style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.align(Alignment.TopCenter).padding(4.dp))
        Box(Modifier.offset { androidx.compose.ui.unit.IntOffset(knob.x.toInt(), knob.y.toInt()) }.size(size / 2.6f).clip(CircleShape).background(MaterialTheme.colorScheme.primary))
    }
}

/** Press-and-hold button sending a gamepad key code. */
@Composable
private fun PadButton(label: String, keyCode: Int, size: Dp = 52.dp, color: Color = MaterialTheme.colorScheme.secondaryContainer, shape: androidx.compose.ui.graphics.Shape = CircleShape) {
    Box(
        Modifier.size(size).clip(shape).background(color)
            .pointerInput(keyCode) {
                awaitPointerEventScope {
                    while (true) {
                        val down = awaitPointerEvent()
                        if (down.changes.any { it.pressed && !it.previousPressed }) {
                            AppWindows.gamepadButton(keyCode, true)
                            while (true) {
                                val ev = awaitPointerEvent()
                                if (ev.changes.all { !it.pressed }) break
                            }
                            AppWindows.gamepadButton(keyCode, false)
                        }
                    }
                }
            },
        contentAlignment = Alignment.Center,
    ) { Text(label, fontWeight = FontWeight.Bold, style = MaterialTheme.typography.labelLarge) }
}

/** Four-way hat sending AXIS_HAT_X/Y. */
@Composable
private fun Hat(size: Dp) {
    Box(Modifier.size(size).clip(CircleShape).background(MaterialTheme.colorScheme.surfaceVariant)
        .pointerInput(Unit) {
            awaitPointerEventScope {
                while (true) {
                    val ev = awaitPointerEvent()
                    val p = ev.changes.firstOrNull { it.pressed }
                    if (p == null) { setAxes(6 to 0f, 7 to 0f); continue }
                    val c = Offset(this.size.width / 2f, this.size.height / 2f)
                    val d = p.position - c
                    val ang = atan2(d.y, d.x)
                    val hx = if (kotlin.math.abs(cos(ang)) > 0.5f) kotlin.math.sign(cos(ang)) else 0f
                    val hy = if (kotlin.math.abs(sin(ang)) > 0.5f) kotlin.math.sign(sin(ang)) else 0f
                    setAxes(6 to hx, 7 to hy)
                }
            }
        }, contentAlignment = Alignment.Center) { Text("✚", style = MaterialTheme.typography.titleLarge) }
}

/** Vertical pedal/throttle; returns 0..1 from bottom to top. */
@Composable
private fun Pedal(label: String, width: Dp, height: Dp, onValue: (Float) -> Unit, springBack: Boolean = true) {
    var v by remember { mutableFloatStateOf(0f) }
    Box(Modifier.size(width, height).clip(RoundedCornerShape(12.dp)).background(MaterialTheme.colorScheme.surfaceVariant)
        .pointerInput(springBack) {
            awaitPointerEventScope {
                while (true) {
                    val ev = awaitPointerEvent()
                    val p = ev.changes.firstOrNull { it.pressed }
                    if (p == null) { if (springBack) { v = 0f; onValue(0f) }; continue }
                    v = (1f - p.position.y / this.size.height).coerceIn(0f, 1f); onValue(v)
                }
            }
        }) {
        Box(Modifier.align(Alignment.BottomCenter).fillMaxWidth().fillMaxHeight(v.coerceAtLeast(0.02f)).background(MaterialTheme.colorScheme.primary.copy(alpha = 0.6f)))
        Text(label, Modifier.align(Alignment.TopCenter).padding(4.dp), style = MaterialTheme.typography.labelSmall)
    }
}

// ---- skins ----------------------------------------------------------------------

@Composable
private fun BoxScope.XboxSkin() {
    Row(Modifier.align(Alignment.TopCenter).fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
        Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            PadButton("LT", KeyEvent.KEYCODE_BUTTON_L2, 44.dp, shape = RoundedCornerShape(10.dp))
            PadButton("LB", KeyEvent.KEYCODE_BUTTON_L1, 44.dp, shape = RoundedCornerShape(10.dp))
        }
        Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            PadButton("☰", KeyEvent.KEYCODE_BUTTON_SELECT, 36.dp)
            PadButton("≡", KeyEvent.KEYCODE_BUTTON_START, 36.dp)
        }
        Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
            PadButton("RB", KeyEvent.KEYCODE_BUTTON_R1, 44.dp, shape = RoundedCornerShape(10.dp))
            PadButton("RT", KeyEvent.KEYCODE_BUTTON_R2, 44.dp, shape = RoundedCornerShape(10.dp))
        }
    }
    Row(Modifier.align(Alignment.Center).fillMaxWidth().padding(top = 44.dp), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
        Stick(120.dp, "L") { x, y -> setAxes(0 to x, 1 to y) }
        Hat(84.dp)
        Box(Modifier.size(140.dp)) {
            Box(Modifier.align(Alignment.TopCenter)) { PadButton("Y", KeyEvent.KEYCODE_BUTTON_Y, 46.dp, Color(0xFFF9D342)) }
            Box(Modifier.align(Alignment.CenterStart)) { PadButton("X", KeyEvent.KEYCODE_BUTTON_X, 46.dp, Color(0xFF6FA8DC)) }
            Box(Modifier.align(Alignment.CenterEnd)) { PadButton("B", KeyEvent.KEYCODE_BUTTON_B, 46.dp, Color(0xFFE57373)) }
            Box(Modifier.align(Alignment.BottomCenter)) { PadButton("A", KeyEvent.KEYCODE_BUTTON_A, 46.dp, Color(0xFF81C784)) }
        }
        Stick(120.dp, "R") { x, y -> setAxes(2 to x, 3 to y) }
    }
}

@Composable
private fun BoxScope.WheelSkin() {
    var angle by remember { mutableFloatStateOf(0f) }   // degrees, ±180 = full lock
    Row(Modifier.fillMaxSize(), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            PadButton("L", KeyEvent.KEYCODE_BUTTON_L1, 48.dp, shape = RoundedCornerShape(10.dp))
            Pedal("brake", 56.dp, 150.dp, { setAxis(4, it) })
        }
        Box(
            Modifier.size(200.dp).clip(CircleShape).background(MaterialTheme.colorScheme.surfaceVariant)
                .pointerInput(Unit) {
                    val c = Offset(this.size.width / 2f, this.size.height / 2f)
                    var last = 0f
                    detectDragGestures(
                        onDragStart = { last = Math.toDegrees(atan2((it - c).y, (it - c).x).toDouble()).toFloat() },
                        onDragEnd = { angle = 0f; setAxis(0, 0f) },
                        onDragCancel = { angle = 0f; setAxis(0, 0f) },
                    ) { change, _ ->
                        change.consume()
                        val a = Math.toDegrees(atan2((change.position - c).y, (change.position - c).x).toDouble()).toFloat()
                        var d = a - last
                        if (d > 180f) d -= 360f; if (d < -180f) d += 360f
                        last = a
                        angle = (angle + d).coerceIn(-180f, 180f)
                        setAxis(0, angle / 180f)
                    }
                },
            contentAlignment = Alignment.Center,
        ) {
            // Spoke shows the wheel angle.
            val r = 80f
            val rad = Math.toRadians((angle - 90f).toDouble())
            Box(Modifier.offset { androidx.compose.ui.unit.IntOffset((r * cos(rad)).toInt(), (r * sin(rad)).toInt()) }.size(28.dp).clip(CircleShape).background(MaterialTheme.colorScheme.primary))
            Text("${angle.toInt()}°", style = MaterialTheme.typography.labelSmall)
        }
        Column(verticalArrangement = Arrangement.spacedBy(8.dp), horizontalAlignment = Alignment.End) {
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                PadButton("R", KeyEvent.KEYCODE_BUTTON_R1, 48.dp, shape = RoundedCornerShape(10.dp))
            }
            Pedal("gas", 56.dp, 150.dp, { setAxis(5, it) })
        }
    }
    Row(Modifier.align(Alignment.BottomCenter), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
        PadButton("A", KeyEvent.KEYCODE_BUTTON_A, 40.dp, Color(0xFF81C784))
        PadButton("B", KeyEvent.KEYCODE_BUTTON_B, 40.dp, Color(0xFFE57373))
        PadButton("X", KeyEvent.KEYCODE_BUTTON_X, 40.dp, Color(0xFF6FA8DC))
        PadButton("Y", KeyEvent.KEYCODE_BUTTON_Y, 40.dp, Color(0xFFF9D342))
        PadButton("≡", KeyEvent.KEYCODE_BUTTON_START, 40.dp)
    }
}

@Composable
private fun BoxScope.FlightSkin() {
    Row(Modifier.fillMaxSize(), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
        // Throttle holds its position (no spring-back), like a real quadrant.
        Pedal("throttle", 64.dp, 220.dp, { setAxis(3, -(it * 2f - 1f)) }, springBack = false)
        Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Stick(190.dp, "stick") { x, y -> setAxes(0 to x, 1 to y) }
            // Twist / rudder: horizontal slider that springs back.
            var rud by remember { mutableFloatStateOf(0f) }
            Slider(rud, { rud = it; setAxis(2, it) }, Modifier.width(190.dp), valueRange = -1f..1f, onValueChangeFinished = { rud = 0f; setAxis(2, 0f) })
            Text("rudder", style = MaterialTheme.typography.labelSmall)
        }
        Column(verticalArrangement = Arrangement.spacedBy(10.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            Hat(84.dp)
            PadButton("FIRE", KeyEvent.KEYCODE_BUTTON_A, 60.dp, Color(0xFFE57373))
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                PadButton("B", KeyEvent.KEYCODE_BUTTON_B, 40.dp)
                PadButton("X", KeyEvent.KEYCODE_BUTTON_X, 40.dp)
            }
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                PadButton("L1", KeyEvent.KEYCODE_BUTTON_L1, 40.dp, shape = RoundedCornerShape(10.dp))
                PadButton("R1", KeyEvent.KEYCODE_BUTTON_R1, 40.dp, shape = RoundedCornerShape(10.dp))
            }
        }
    }
}
