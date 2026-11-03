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
import io.perch.android.host.InputTarget
import io.perch.android.host.LocalInputTarget
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
/** The gamepad surface inside the touchpad pop-up: one of the three skins. */
@Composable
fun BoxScope.GamepadSkin(mode: Int) {
    when (mode) { 1 -> XboxSkin(); 2 -> WheelSkin(); else -> FlightSkin() }
}

// ---- axis helpers ------------------------------------------------------------

private fun InputTarget.setAxis(i: Int, v: Float) { axes[i] = v.coerceIn(-1f, 1f); gamepadAxes() }
private fun InputTarget.setAxes(vararg iv: Pair<Int, Float>) { for ((i, v) in iv) axes[i] = v.coerceIn(-1f, 1f); gamepadAxes(force = true) }

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
    val t = LocalInputTarget.current
    Box(
        Modifier.size(size).clip(shape).background(color)
            .pointerInput(keyCode) {
                awaitPointerEventScope {
                    while (true) {
                        val down = awaitPointerEvent()
                        if (down.changes.any { it.pressed && !it.previousPressed }) {
                            t.gamepadButton(keyCode, true)
                            while (true) {
                                val ev = awaitPointerEvent()
                                if (ev.changes.all { !it.pressed }) break
                            }
                            t.gamepadButton(keyCode, false)
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
    val t = LocalInputTarget.current
    Box(Modifier.size(size).clip(CircleShape).background(MaterialTheme.colorScheme.surfaceVariant)
        .pointerInput(Unit) {
            awaitPointerEventScope {
                while (true) {
                    val ev = awaitPointerEvent()
                    val p = ev.changes.firstOrNull { it.pressed }
                    if (p == null) { t.setAxes(6 to 0f, 7 to 0f); continue }
                    val c = Offset(this.size.width / 2f, this.size.height / 2f)
                    val d = p.position - c
                    val ang = atan2(d.y, d.x)
                    val hx = if (kotlin.math.abs(cos(ang)) > 0.5f) kotlin.math.sign(cos(ang)) else 0f
                    val hy = if (kotlin.math.abs(sin(ang)) > 0.5f) kotlin.math.sign(sin(ang)) else 0f
                    t.setAxes(6 to hx, 7 to hy)
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
    val t = LocalInputTarget.current
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        // Row 1: triggers and bumpers at the corners, Xbox button in the middle.
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                PadButton("LT", KeyEvent.KEYCODE_BUTTON_L2, 56.dp, shape = RoundedCornerShape(12.dp))
                PadButton("LB", KeyEvent.KEYCODE_BUTTON_L1, 56.dp, shape = RoundedCornerShape(12.dp))
            }
            PadButton("ⓧ", KeyEvent.KEYCODE_BUTTON_MODE, 40.dp, Color(0xFF9CCC65))
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                PadButton("RB", KeyEvent.KEYCODE_BUTTON_R1, 56.dp, shape = RoundedCornerShape(12.dp))
                PadButton("RT", KeyEvent.KEYCODE_BUTTON_R2, 56.dp, shape = RoundedCornerShape(12.dp))
            }
        }
        // Row 2: the two sticks, stick-click buttons beside them (not below: saves a row).
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceEvenly, verticalAlignment = Alignment.CenterVertically) {
            Row(verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                PadButton("L3", KeyEvent.KEYCODE_BUTTON_THUMBL, 34.dp)
                Stick(118.dp, "L") { x, y -> t.setAxes(0 to x, 1 to y) }
            }
            Row(verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                Stick(118.dp, "R") { x, y -> t.setAxes(2 to x, 3 to y) }
                PadButton("R3", KeyEvent.KEYCODE_BUTTON_THUMBR, 34.dp)
            }
        }
        // Row 3: view / menu and the hat on the left, A B X Y on the right.
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp), verticalAlignment = Alignment.CenterVertically) {
                Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    PadButton("view", KeyEvent.KEYCODE_BUTTON_SELECT, 40.dp, shape = RoundedCornerShape(10.dp))
                    PadButton("menu", KeyEvent.KEYCODE_BUTTON_START, 40.dp, shape = RoundedCornerShape(10.dp))
                }
                Hat(84.dp)
            }
            // Diamond on a tight 3 × 3 grid: buttons touch but never overlap.
            val btn = 54.dp; val cell = 52.dp
            Box(Modifier.size(cell * 2 + btn)) {
                Box(Modifier.offset(x = cell, y = 0.dp)) { PadButton("Y", KeyEvent.KEYCODE_BUTTON_Y, btn, Color(0xFFF9D342)) }
                Box(Modifier.offset(x = 0.dp, y = cell)) { PadButton("X", KeyEvent.KEYCODE_BUTTON_X, btn, Color(0xFF6FA8DC)) }
                Box(Modifier.offset(x = cell * 2, y = cell)) { PadButton("B", KeyEvent.KEYCODE_BUTTON_B, btn, Color(0xFFE57373)) }
                Box(Modifier.offset(x = cell, y = cell * 2)) { PadButton("A", KeyEvent.KEYCODE_BUTTON_A, btn, Color(0xFF81C784)) }
            }
        }
    }
}

@Composable
private fun BoxScope.WheelSkin() {
    val t = LocalInputTarget.current
    var angle by remember { mutableFloatStateOf(0f) }   // degrees, ±180 = full lock
    Column(Modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
            Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                PadButton("L", KeyEvent.KEYCODE_BUTTON_L1, 48.dp, shape = RoundedCornerShape(10.dp))
                Pedal("brake", 56.dp, 140.dp, { t.setAxis(4, it) })
            }
            Box(
                Modifier.size(190.dp).clip(CircleShape).background(MaterialTheme.colorScheme.surfaceVariant)
                    .pointerInput(Unit) {
                        val c = Offset(this.size.width / 2f, this.size.height / 2f)
                        var last = 0f
                        detectDragGestures(
                            onDragStart = { last = Math.toDegrees(atan2((it - c).y, (it - c).x).toDouble()).toFloat() },
                            onDragEnd = { angle = 0f; t.setAxis(0, 0f) },
                            onDragCancel = { angle = 0f; t.setAxis(0, 0f) },
                        ) { change, _ ->
                            change.consume()
                            val a = Math.toDegrees(atan2((change.position - c).y, (change.position - c).x).toDouble()).toFloat()
                            var d = a - last
                            if (d > 180f) d -= 360f; if (d < -180f) d += 360f
                            last = a
                            angle = (angle + d).coerceIn(-180f, 180f)
                            t.setAxis(0, angle / 180f)
                        }
                    },
                contentAlignment = Alignment.Center,
            ) {
                val r = 76f
                val rad = Math.toRadians((angle - 90f).toDouble())
                Box(Modifier.offset { androidx.compose.ui.unit.IntOffset((r * cos(rad)).toInt(), (r * sin(rad)).toInt()) }.size(28.dp).clip(CircleShape).background(MaterialTheme.colorScheme.primary))
                Text("${angle.toInt()}°", style = MaterialTheme.typography.labelSmall)
            }
            Column(verticalArrangement = Arrangement.spacedBy(6.dp), horizontalAlignment = Alignment.End) {
                PadButton("R", KeyEvent.KEYCODE_BUTTON_R1, 48.dp, shape = RoundedCornerShape(10.dp))
                Pedal("gas", 56.dp, 140.dp, { t.setAxis(5, it) })
            }
        }
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceEvenly) {
            PadButton("A", KeyEvent.KEYCODE_BUTTON_A, 54.dp, Color(0xFF81C784))
            PadButton("B", KeyEvent.KEYCODE_BUTTON_B, 54.dp, Color(0xFFE57373))
            PadButton("X", KeyEvent.KEYCODE_BUTTON_X, 54.dp, Color(0xFF6FA8DC))
            PadButton("Y", KeyEvent.KEYCODE_BUTTON_Y, 54.dp, Color(0xFFF9D342))
            PadButton("view", KeyEvent.KEYCODE_BUTTON_SELECT, 46.dp, shape = RoundedCornerShape(10.dp))
            PadButton("menu", KeyEvent.KEYCODE_BUTTON_START, 46.dp, shape = RoundedCornerShape(10.dp))
        }
    }
}

@Composable
private fun BoxScope.FlightSkin() {
    val t = LocalInputTarget.current
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween, verticalAlignment = Alignment.CenterVertically) {
        // Throttle holds its position (no spring-back), like a real quadrant.
        Pedal("throttle", 64.dp, 200.dp, { t.setAxis(3, -(it * 2f - 1f)) }, springBack = false)
        Column(horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(2.dp)) {
            Stick(170.dp, "stick") { x, y -> t.setAxes(0 to x, 1 to y) }
            var rud by remember { mutableFloatStateOf(0f) }
            Slider(rud, { rud = it; t.setAxis(2, it) }, Modifier.width(170.dp), valueRange = -1f..1f, onValueChangeFinished = { rud = 0f; t.setAxis(2, 0f) })
            Text("rudder", style = MaterialTheme.typography.labelSmall)
        }
        Column(verticalArrangement = Arrangement.spacedBy(6.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            Hat(76.dp)
            PadButton("FIRE", KeyEvent.KEYCODE_BUTTON_A, 68.dp, Color(0xFFE57373))
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                PadButton("B", KeyEvent.KEYCODE_BUTTON_B, 48.dp)
                PadButton("X", KeyEvent.KEYCODE_BUTTON_X, 48.dp)
            }
            Row(horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                PadButton("L1", KeyEvent.KEYCODE_BUTTON_L1, 48.dp, shape = RoundedCornerShape(10.dp))
                PadButton("R1", KeyEvent.KEYCODE_BUTTON_R1, 48.dp, shape = RoundedCornerShape(10.dp))
            }
        }
    }
}
