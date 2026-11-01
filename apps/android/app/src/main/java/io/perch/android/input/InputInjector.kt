package io.perch.android.input

import android.accessibilityservice.AccessibilityService
import android.accessibilityservice.GestureDescription
import android.graphics.Path
import android.inputmethodservice.InputMethodService
import android.util.Log
import android.view.accessibility.AccessibilityEvent
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.float
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import java.util.concurrent.atomic.AtomicReference

/**
 * When a window that lives on *this phone* has focus, perchd forwards the
 * routed input here (`{"msg":"input","events":[...]}` over the control
 * WebSocket). Android has no uinput for unprivileged apps, so:
 *
 *  - pointer motion/clicks/scroll → [PerchAccessibilityService.dispatchGesture]
 *    (taps, drags, flings). We keep our own cursor position and draw it with
 *    an overlay in a later milestone.
 *  - text → [PerchInputMethodService] commits it to the focused text field
 *    (the user selects "Perch remote keyboard" as the active IME when using
 *    the phone from the glasses; the app offers a one-tap switcher).
 *  - hardware keys (arrows, enter, shortcuts) → IME `sendDownUpKeyEvents`.
 *
 * For true keyboard/mouse injection into *any* app (games, DeX), the
 * privileged route is Shizuku/ADB `INJECT_EVENTS`; see docs/PLATFORMS.md.
 */
object InputInjector {
    private const val TAG = "InputInjector"
    val a11y = AtomicReference<PerchAccessibilityService?>(null)
    val ime = AtomicReference<PerchInputMethodService?>(null)

    // Cursor in screen pixels; MouseMoveAbs is window-normalised so we scale.
    @Volatile var cursorX = 540f
    @Volatile var cursorY = 1170f
    @Volatile var screenW = 1080f
    @Volatile var screenH = 2340f
    private var leftDown = false
    private var dragPath: Path? = null

    fun inject(events: JsonArray) {
        val svc = a11y.get()
        for (e in events) {
            val o = e.jsonObject
            // externally tagged: {"mouse_move":{...}}
            val (kind, body) = o.entries.first().let { it.key to it.value.jsonObject }
            when (kind) {
                "mouse_move" -> { cursorX = (cursorX + body["dx"]!!.jsonPrimitive.float).coerceIn(0f, screenW); cursorY = (cursorY + body["dy"]!!.jsonPrimitive.float).coerceIn(0f, screenH); if (leftDown) dragPath?.lineTo(cursorX, cursorY) }
                "mouse_move_abs" -> { cursorX = body["x"]!!.jsonPrimitive.float * screenW; cursorY = body["y"]!!.jsonPrimitive.float * screenH; if (leftDown) dragPath?.lineTo(cursorX, cursorY) }
                "mouse_button" -> {
                    val down = body["down"]!!.jsonPrimitive.boolean
                    val button = body["button"]!!.jsonPrimitive.content
                    if (button == "left") {
                        if (down) { leftDown = true; dragPath = Path().apply { moveTo(cursorX, cursorY) } }
                        else {
                            leftDown = false
                            val p = dragPath ?: Path().apply { moveTo(cursorX, cursorY) }
                            svc?.gesture(p, if (p.isEmpty) 60 else 250)
                            dragPath = null
                        }
                    } else if (button == "right" && !down) {
                        svc?.gesture(Path().apply { moveTo(cursorX, cursorY) }, 600) // long press ≈ context menu
                    } else if (button == "back" && !down) {
                        svc?.performGlobalAction(AccessibilityService.GLOBAL_ACTION_BACK)
                    }
                }
                "scroll" -> {
                    val dy = body["dy"]!!.jsonPrimitive.float
                    val p = Path().apply { moveTo(cursorX, cursorY); lineTo(cursorX, cursorY + dy * 80f) }
                    svc?.gesture(p, 120)
                }
                "text" -> ime.get()?.commit(body["ch"]!!.jsonPrimitive.content)
                "key" -> {
                    val down = body["down"]!!.jsonPrimitive.boolean
                    if (down) ime.get()?.key(body["usage"]!!.jsonPrimitive.content.toInt(), body["mods"]!!.jsonPrimitive.content.toInt())
                }
            }
        }
    }
}

class PerchAccessibilityService : AccessibilityService() {
    override fun onServiceConnected() { InputInjector.a11y.set(this); Log.i("Perch", "accessibility connected") }
    override fun onAccessibilityEvent(event: AccessibilityEvent?) {}
    override fun onInterrupt() {}
    override fun onDestroy() { InputInjector.a11y.compareAndSet(this, null); super.onDestroy() }

    fun gesture(path: Path, durationMs: Long) {
        val stroke = GestureDescription.StrokeDescription(path, 0, durationMs)
        dispatchGesture(GestureDescription.Builder().addStroke(stroke).build(), null, null)
    }
}

class PerchInputMethodService : InputMethodService() {
    override fun onCreate() { super.onCreate(); InputInjector.ime.set(this) }
    override fun onDestroy() { InputInjector.ime.compareAndSet(this, null); super.onDestroy() }
    override fun onEvaluateInputViewShown(): Boolean = false   // no on-screen keyboard; it's remote

    fun commit(text: String) { currentInputConnection?.commitText(text, 1) }

    /** HID usage → Android KeyEvent for the non-printing keys we care about. */
    fun key(usage: Int, mods: Int) {
        val code = when (usage) {
            0x28 -> android.view.KeyEvent.KEYCODE_ENTER
            0x29 -> android.view.KeyEvent.KEYCODE_ESCAPE
            0x2a -> android.view.KeyEvent.KEYCODE_DEL
            0x2b -> android.view.KeyEvent.KEYCODE_TAB
            0x4f -> android.view.KeyEvent.KEYCODE_DPAD_RIGHT
            0x50 -> android.view.KeyEvent.KEYCODE_DPAD_LEFT
            0x51 -> android.view.KeyEvent.KEYCODE_DPAD_DOWN
            0x52 -> android.view.KeyEvent.KEYCODE_DPAD_UP
            0x4a -> android.view.KeyEvent.KEYCODE_MOVE_HOME
            0x4d -> android.view.KeyEvent.KEYCODE_MOVE_END
            0x4c -> android.view.KeyEvent.KEYCODE_FORWARD_DEL
            else -> return
        }
        val meta = (if (mods and 1 != 0) android.view.KeyEvent.META_CTRL_ON else 0) or
            (if (mods and 2 != 0) android.view.KeyEvent.META_SHIFT_ON else 0) or
            (if (mods and 4 != 0) android.view.KeyEvent.META_ALT_ON else 0)
        val ic = currentInputConnection ?: return
        val now = android.os.SystemClock.uptimeMillis()
        ic.sendKeyEvent(android.view.KeyEvent(now, now, android.view.KeyEvent.ACTION_DOWN, code, 0, meta))
        ic.sendKeyEvent(android.view.KeyEvent(now, now, android.view.KeyEvent.ACTION_UP, code, 0, meta))
    }
}
