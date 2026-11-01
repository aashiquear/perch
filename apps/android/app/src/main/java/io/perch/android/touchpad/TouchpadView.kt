package io.perch.android.touchpad

import android.os.SystemClock
import android.view.MotionEvent
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.input.pointer.pointerInteropFilter
import androidx.compose.ui.ExperimentalComposeUiApi
import kotlin.math.abs

/**
 * Laptop-style trackpad gestures → [Postcard.Event]s, batched every ~8 ms:
 *
 *  - 1 finger move      → pointer (with acceleration)
 *  - 1 finger tap       → left click; two quick taps + hold → drag
 *  - 2 finger tap       → right click
 *  - 2 finger move      → scroll (natural)
 *  - 3 finger swipe ←/→ → focus previous / next window
 *  - 3 finger tap       → middle click
 *
 * Gesture state is tiny on purpose; everything else (edge switching, focus)
 * is decided by perchd so all controllers behave the same.
 */
class TouchpadGestures(private val send: (List<Postcard.Event>) -> Unit, private val onThreeSwipe: (Int) -> Unit) {
    var sensitivity = 1.4f
    var naturalScroll = true

    private var lastX = 0f; private var lastY = 0f
    private var downT = 0L; private var downX = 0f; private var downY = 0f
    private var maxFingers = 0
    private var moved = false
    private var dragging = false
    private var lastTapT = 0L
    private var scrollAcc = 0f
    private var threeStartX = 0f
    private val batch = ArrayList<Postcard.Event>()
    private var lastFlush = 0L

    fun onTouch(e: MotionEvent): Boolean {
        when (e.actionMasked) {
            MotionEvent.ACTION_DOWN -> {
                lastX = e.x; lastY = e.y; downX = e.x; downY = e.y
                downT = SystemClock.uptimeMillis(); maxFingers = 1; moved = false; scrollAcc = 0f
                // double-tap-and-hold → drag
                if (downT - lastTapT < 250) { dragging = true; batch += Postcard.Event.MouseButton(Postcard.BTN_LEFT, true) }
            }
            MotionEvent.ACTION_POINTER_DOWN -> {
                maxFingers = maxOf(maxFingers, e.pointerCount)
                if (e.pointerCount == 3) threeStartX = e.getX(0)
                lastX = e.getX(0); lastY = e.getY(0)
            }
            MotionEvent.ACTION_MOVE -> {
                val x = e.getX(0); val y = e.getY(0)
                val dx = x - lastX; val dy = y - lastY
                lastX = x; lastY = y
                if (abs(x - downX) > 8 || abs(y - downY) > 8) moved = true
                when (e.pointerCount) {
                    1 -> batch += Postcard.Event.MouseMove(accel(dx), accel(dy))
                    2 -> {
                        scrollAcc += dy
                        val lines = (scrollAcc / 24f).toInt()
                        if (lines != 0) {
                            scrollAcc -= lines * 24f
                            batch += Postcard.Event.Scroll(0f, (if (naturalScroll) lines else -lines).toFloat())
                        }
                    }
                }
            }
            MotionEvent.ACTION_POINTER_UP -> {
                if (e.pointerCount == 3 && !moved) batch += click(Postcard.BTN_MIDDLE)
                if (maxFingers == 3 && moved) {
                    val d = e.getX(0) - threeStartX
                    if (abs(d) > 120) onThreeSwipe(if (d > 0) 1 else -1)
                    moved = false
                }
            }
            MotionEvent.ACTION_UP -> {
                val dt = SystemClock.uptimeMillis() - downT
                if (dragging) { dragging = false; batch += Postcard.Event.MouseButton(Postcard.BTN_LEFT, false) }
                else if (!moved && dt < 200) {
                    batch += click(if (maxFingers >= 2) Postcard.BTN_RIGHT else Postcard.BTN_LEFT)
                    lastTapT = SystemClock.uptimeMillis()
                }
                maxFingers = 0
            }
            MotionEvent.ACTION_CANCEL -> { if (dragging) { dragging = false; batch += Postcard.Event.MouseButton(Postcard.BTN_LEFT, false) } }
        }
        flush(force = e.actionMasked == MotionEvent.ACTION_UP)
        return true
    }

    private fun accel(d: Float): Float {
        val a = abs(d)
        val gain = if (a > 20f) 2.2f else if (a > 6f) 1.5f else 1.0f
        return d * sensitivity * gain
    }

    private fun click(b: Int) = listOf(Postcard.Event.MouseButton(b, true), Postcard.Event.MouseButton(b, false))
    private operator fun ArrayList<Postcard.Event>.plusAssign(l: List<Postcard.Event>) { addAll(l) }

    private fun flush(force: Boolean) {
        val now = SystemClock.uptimeMillis()
        if (batch.isEmpty()) return
        if (force || now - lastFlush >= 8) {
            send(batch.toList()); batch.clear(); lastFlush = now
        }
    }
}

@OptIn(ExperimentalComposeUiApi::class)
@Composable
fun TouchpadSurface(client: PerchClient, modifier: Modifier = Modifier) {
    val gestures = remember(client) {
        TouchpadGestures(
            send = { client.sendInput(Postcard.input(it)) },
            onThreeSwipe = { dir -> if (dir > 0) client.focusNext() else client.focusNext() /* prev: add FocusPrev in proto v1.1 */ },
        )
    }
    Box(
        modifier
            .fillMaxSize()
            .clip(RoundedCornerShape(24.dp()))
            .background(MaterialTheme.colorScheme.surfaceVariant)
            .pointerInteropFilter { gestures.onTouch(it) },
        contentAlignment = Alignment.Center,
    ) {
        Text("trackpad · 1 tap click · 2 tap right-click · 2 drag scroll · 3 swipe switch window",
            style = MaterialTheme.typography.labelSmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
    }
}

private fun Int.dp() = androidx.compose.ui.unit.Dp(this.toFloat())
