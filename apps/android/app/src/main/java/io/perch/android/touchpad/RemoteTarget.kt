package io.perch.android.touchpad

import android.view.KeyEvent
import io.perch.android.host.InputTarget
import kotlinx.coroutines.flow.MutableStateFlow
import kotlin.math.abs
import kotlin.math.ln

/**
 * The pop-up's input sent to a laptop running perchd: mouse, keys, text and
 * the whole controller state over the UDP stream (protocol 1.6).
 */
class RemoteTarget(private val client: PerchClient) : InputTarget {
    private companion object {
        /** HID usage of left Ctrl. */
        const val CTRL = 0xe0
        /** ln(1.1): one wheel notch per 10 % pinch. */
        val ZOOM_STEP = ln(1.1f)
    }
    private fun send(vararg ev: Postcard.Event) = client.sendInput(Postcard.input(ev.toList()))

    override fun cursorMove(dx: Float, dy: Float) = send(Postcard.Event.MouseMove(dx, dy))
    override fun tap() = send(Postcard.Event.MouseButton(0, true), Postcard.Event.MouseButton(0, false))
    override fun pressDown() = send(Postcard.Event.MouseButton(0, true))
    override fun pressUp() = send(Postcard.Event.MouseButton(0, false))
    override fun scroll(dx: Float, dy: Float) = send(Postcard.Event.Scroll(dx, dy))
    /** A laptop has no finger: content-follows-fingers becomes wheel lines (≈ 30 px per line). */
    override fun scrollMove(dx: Float, dy: Float) {
        accX += dx / 30f; accY += dy / 30f
        val lx = accX.toInt(); val ly = accY.toInt()
        if (lx != 0 || ly != 0) { accX -= lx; accY -= ly; send(Postcard.Event.Scroll(-lx.toFloat(), ly.toFloat())) }
    }
    private var accX = 0f; private var accY = 0f
    override fun scrollEnd() { accX = 0f; accY = 0f }
    /** A laptop zooms with Ctrl+wheel: one notch per ~10 % change in finger span. */
    override fun pinch(scale: Float) {
        if (scale <= 0f) return
        zoomAcc += ln(scale)
        while (abs(zoomAcc) >= ZOOM_STEP) {
            val dir = if (zoomAcc > 0) 1f else -1f
            zoomAcc -= dir * ZOOM_STEP
            send(Postcard.Event.Key(CTRL, true, 1), Postcard.Event.Scroll(0f, dir), Postcard.Event.Key(CTRL, false, 0))
        }
    }
    override fun pinchEnd() { zoomAcc = 0f }
    private var zoomAcc = 0f

    override fun key(keyCode: Int) {
        val u = HidMap.usage(keyCode) ?: return
        send(Postcard.Event.Key(u, true, 0), Postcard.Event.Key(u, false, 0))
    }
    override fun arrow(keyCode: Int, asGamepad: Boolean) {
        if (asGamepad) { gamepadButton(keyCode, true); gamepadButton(keyCode, false) } else key(keyCode)
    }
    override fun typeText(text: String) = send(*text.map { Postcard.Event.Text(it.toString()) }.toTypedArray())

    /** A hardware key that reached the phone while the remote is active. */
    fun hardwareKey(ev: KeyEvent): Boolean {
        val u = HidMap.usage(ev.keyCode) ?: return false
        send(Postcard.Event.Key(u, ev.action == KeyEvent.ACTION_DOWN, HidMap.mods(ev.metaState)))
        return true
    }

    // ---- controller: same axes/buttons layout as the local HID report ----
    override val axes = FloatArray(8)
    private var buttons = 0
    private var lastSend = 0L
    private fun bit(keyCode: Int): Int = when (keyCode) {
        KeyEvent.KEYCODE_BUTTON_A -> 0; KeyEvent.KEYCODE_BUTTON_B -> 1; KeyEvent.KEYCODE_BUTTON_C -> 2
        KeyEvent.KEYCODE_BUTTON_X -> 3; KeyEvent.KEYCODE_BUTTON_Y -> 4; KeyEvent.KEYCODE_BUTTON_Z -> 5
        KeyEvent.KEYCODE_BUTTON_L1 -> 6; KeyEvent.KEYCODE_BUTTON_R1 -> 7; KeyEvent.KEYCODE_BUTTON_L2 -> 8
        KeyEvent.KEYCODE_BUTTON_R2 -> 9; KeyEvent.KEYCODE_BUTTON_SELECT -> 10; KeyEvent.KEYCODE_BUTTON_START -> 11
        KeyEvent.KEYCODE_BUTTON_MODE -> 12; KeyEvent.KEYCODE_BUTTON_THUMBL -> 13; KeyEvent.KEYCODE_BUTTON_THUMBR -> 14
        else -> -1
    }
    private fun report() = send(Postcard.Event.Gamepad(axes.copyOf(), buttons))
    override fun gamepadAxes(force: Boolean) {
        val now = android.os.SystemClock.uptimeMillis()
        if (!force && now - lastSend < 12) return
        lastSend = now; report()
    }
    override fun gamepadButton(keyCode: Int, down: Boolean) {
        val b = bit(keyCode)
        if (b >= 0) buttons = if (down) buttons or (1 shl b) else buttons and (1 shl b).inv()
        else when (keyCode) {
            KeyEvent.KEYCODE_DPAD_UP -> axes[7] = if (down) -1f else 0f
            KeyEvent.KEYCODE_DPAD_DOWN -> axes[7] = if (down) 1f else 0f
            KeyEvent.KEYCODE_DPAD_LEFT -> axes[6] = if (down) -1f else 0f
            KeyEvent.KEYCODE_DPAD_RIGHT -> axes[6] = if (down) 1f else 0f
        }
        if (keyCode == KeyEvent.KEYCODE_BUTTON_L2) axes[4] = if (down) 1f else 0f
        if (keyCode == KeyEvent.KEYCODE_BUTTON_R2) axes[5] = if (down) 1f else 0f
        report()
    }
    override fun gamepadAnnounce() { java.util.Arrays.fill(axes, 0f); buttons = 0; report() }

    override fun focusNext(step: Int) = client.focusNext()
    override val supportsWindowOps: Boolean get() = false
    override fun pinHere() { client.windows.value.firstOrNull { it.focused }?.let { client.pinHere(it.id) } }
    override fun flip() {}
    override fun bigger() {}
    override fun smaller() {}
    override fun nearer() {}
    override fun further() {}
    override fun relaunch() {}
    override val mouseMode = MutableStateFlow(true)
}
