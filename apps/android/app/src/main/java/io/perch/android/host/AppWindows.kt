package io.perch.android.host

import android.content.Context
import android.content.pm.PackageManager
import android.graphics.drawable.Drawable
import android.os.SystemClock
import android.util.Log
import android.view.InputDevice
import android.view.MotionEvent
import android.view.Surface
import io.perch.android.native.PerchNative
import io.perch.android.shell.ShellBridge
import kotlinx.coroutines.flow.MutableStateFlow

/**
 * The phone-as-host window list: one entry per app shown on the glasses,
 * each backed by its own trusted virtual display (through [ShellBridge]).
 * Also owns the cursor: a position inside the focused window that the
 * pop-up touchpad moves and that taps/scrolls are injected at.
 */
object AppWindows {
    private const val TAG = "AppWindows"

    /** Portrait display size (phone-like) and its landscape counterpart. */
    const val PORTRAIT_W = 1080
    const val PORTRAIT_H = 2340
    const val DPI = 420

    data class Win(
        val id: Int,
        val pkg: String,
        val label: String,
        val icon: Drawable?,
        val displayId: Int,
        /** Renderer layer index. */
        val layer: Int,
        val portrait: Boolean,
    ) {
        val width: Int get() = if (portrait) PORTRAIT_W else PORTRAIT_H
        val height: Int get() = if (portrait) PORTRAIT_H else PORTRAIT_W
    }

    val windows: MutableStateFlow<List<Win>> = MutableStateFlow(emptyList())
    val focused: MutableStateFlow<Int?> = MutableStateFlow(null)
    val lastError: MutableStateFlow<String?> = MutableStateFlow(null)
    /** Cursor in the focused window, normalised 0..1. */
    @Volatile var cursorU = 0.5f
    @Volatile var cursorV = 0.5f
    private var nextId = 1

    /** Metres for a display: keep the Linux grid's row height, width by aspect. */
    private fun metres(w: Int, h: Int): Pair<Float, Float> {
        val rowH = 0.8f
        return (rowH * w / h) to rowH
    }

    /** Launch `pkg` on a new glasses display at the current gaze. */
    fun add(ctx: Context, pkg: String, portrait: Boolean = true) {
        val shell = ShellBridge.shell.value ?: run { lastError.value = "Shizuku helper not connected"; return }
        val svc = GlassesService.presentation ?: run { lastError.value = "glasses display not attached"; return }
        val pm = ctx.packageManager
        val launch = pm.getLaunchIntentForPackage(pkg)?.component ?: run { lastError.value = "$pkg has no launcher activity"; return }
        val (label, icon) = try {
            val ai = pm.getApplicationInfo(pkg, 0)
            pm.getApplicationLabel(ai).toString() to pm.getApplicationIcon(ai)
        } catch (e: PackageManager.NameNotFoundException) { pkg to null }
        val w = if (portrait) PORTRAIT_W else PORTRAIT_H
        val h = if (portrait) PORTRAIT_H else PORTRAIT_W
        val (wm, hm) = metres(w, h)
        // Place beside the last window (simple flow row), like the Linux grid.
        val n = windows.value.size
        val yaw = -(n * (0.75f + 0.08f)) * 0.5f
        svc.addWindow(PerchNative.SOURCE_VIRTUAL, w, h, yaw, 0f, 2f, wm, hm, PerchNative.ANCHOR_WORLD) { layer, surface: Surface ->
            val displayId = try { shell.createDisplay("perch-$pkg", w, h, DPI, surface) } catch (e: Throwable) { -1 }
            if (displayId < 0) {
                val why = runCatching { shell.lastError() }.getOrNull().orEmpty()
                lastError.value = "could not create a glasses display: " + why.ifEmpty { "(is Shizuku running as shell?)" }
                svc.hideWindow(layer)
                return@addWindow
            }
            val err = try { shell.launch(displayId, launch.flattenToString()) } catch (e: Throwable) { e.toString() }
            if (err.isNotEmpty()) {
                Log.w(TAG, "launch: $err")
                lastError.value = "launch: $err"
            }
            val win = Win(nextId++, pkg, label, icon, displayId, layer, portrait)
            windows.value = windows.value + win
            focus(win.id)
        }
    }

    fun focus(id: Int) {
        focused.value = id
        cursorU = 0.5f; cursorV = 0.5f
        GlassesService.presentation?.setCursor(windows.value.firstOrNull { it.id == id }?.layer, cursorU, cursorV)
    }

    fun focusNext(step: Int) {
        val list = windows.value
        if (list.isEmpty()) return
        val i = list.indexOfFirst { it.id == focused.value }
        focus(list[((i + step) % list.size + list.size) % list.size].id)
    }

    /** Portrait ⇄ landscape: resize the display; the app re-lays itself out. */
    fun flip(id: Int) {
        val win = windows.value.firstOrNull { it.id == id } ?: return
        val shell = ShellBridge.shell.value ?: return
        val flipped = win.copy(portrait = !win.portrait)
        val ok = try { shell.resizeDisplay(win.displayId, flipped.width, flipped.height, DPI) } catch (e: Throwable) { false }
        if (!ok) { lastError.value = "resize failed"; return }
        val (wm, hm) = metres(flipped.width, flipped.height)
        GlassesService.presentation?.resizeWindow(win.layer, flipped.width, flipped.height, wm, hm)
        windows.value = windows.value.map { if (it.id == id) flipped else it }
    }

    fun remove(id: Int) {
        val win = windows.value.firstOrNull { it.id == id } ?: return
        runCatching { ShellBridge.shell.value?.releaseDisplay(win.displayId) }
        GlassesService.presentation?.hideWindow(win.layer)
        windows.value = windows.value.filter { it.id != id }
        if (focused.value == id) focused.value = windows.value.lastOrNull()?.id
        GlassesService.presentation?.setCursor(windows.value.firstOrNull { it.id == focused.value }?.layer, 0.5f, 0.5f)
    }

    fun releaseAll() {
        windows.value.map { it.id }.forEach { remove(it) }
    }

    /** Linux bar equivalents: nearer/further step 0.25 m, bigger/smaller ×1.15. */
    fun nearer(id: Int) = nudge(id, -0.25f, 1f)
    fun further(id: Int) = nudge(id, 0.25f, 1f)
    fun bigger(id: Int) = nudge(id, 0f, 1.15f)
    fun smaller(id: Int) = nudge(id, 0f, 1 / 1.15f)
    private fun nudge(id: Int, d: Float, s: Float) {
        windows.value.firstOrNull { it.id == id }?.let { GlassesService.presentation?.nudgeWindow(it.layer, d, s) }
    }

    /** Bring the app back to the front of its display (after Back left it blank, say). */
    fun relaunch(id: Int) {
        val win = windows.value.firstOrNull { it.id == id } ?: return
        val shell = ShellBridge.shell.value ?: return
        val launch = runCatching { GlassesService.appContext?.packageManager?.getLaunchIntentForPackage(win.pkg)?.component }.getOrNull() ?: return
        val err = runCatching { shell.launch(win.displayId, launch.flattenToString()) }.getOrElse { it.toString() }
        if (err.isNotEmpty()) lastError.value = "relaunch: $err"
    }

    fun pinHere(id: Int) {
        windows.value.firstOrNull { it.id == id }?.let { GlassesService.presentation?.pinHere(it.layer) }
    }

    // ---- cursor / injection -------------------------------------------------

    private fun focusedWin(): Win? = windows.value.firstOrNull { it.id == focused.value }

    /** Move the cursor by trackpad pixels (already accelerated by the gesture layer). */
    fun cursorMove(dx: Float, dy: Float) {
        val w = focusedWin() ?: return
        cursorU = (cursorU + dx / w.width).coerceIn(0f, 1f)
        cursorV = (cursorV + dy / w.height).coerceIn(0f, 1f)
        GlassesService.presentation?.setCursor(w.layer, cursorU, cursorV)
        if (dragging) touch(MotionEvent.ACTION_MOVE)
    }

    private var dragging = false
    private var downTime = 0L

    private fun touch(action: Int) {
        val w = focusedWin() ?: return
        val shell = ShellBridge.shell.value ?: return
        val now = SystemClock.uptimeMillis()
        if (action == MotionEvent.ACTION_DOWN) downTime = now
        val ev = MotionEvent.obtain(downTime, now, action, cursorU * w.width, cursorV * w.height, 0)
        ev.source = InputDevice.SOURCE_TOUCHSCREEN
        runCatching { shell.injectMotion(w.displayId, ev) }.onFailure { Log.w(TAG, "inject: $it") }
        ev.recycle()
    }

    /** Tap = touch down/up at the cursor. */
    fun tap() { touch(MotionEvent.ACTION_DOWN); touch(MotionEvent.ACTION_UP) }

    fun pressDown() { dragging = true; touch(MotionEvent.ACTION_DOWN) }
    fun pressUp() { if (dragging) { dragging = false; touch(MotionEvent.ACTION_UP) } }

    /** Two-finger scroll → mouse wheel at the cursor (lines, positive = up). */
    fun scroll(dy: Float) {
        val w = focusedWin() ?: return
        val shell = ShellBridge.shell.value ?: return
        val now = SystemClock.uptimeMillis()
        val props = arrayOf(MotionEvent.PointerProperties().apply { id = 0; toolType = MotionEvent.TOOL_TYPE_MOUSE })
        val coords = arrayOf(MotionEvent.PointerCoords().apply {
            x = cursorU * w.width; y = cursorV * w.height
            setAxisValue(MotionEvent.AXIS_VSCROLL, dy)
        })
        val ev = MotionEvent.obtain(now, now, MotionEvent.ACTION_SCROLL, 1, props, coords, 0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_MOUSE, 0)
        runCatching { shell.injectMotion(w.displayId, ev) }
        ev.recycle()
    }

    /** Back key etc. to the focused app. */
    fun key(keyCode: Int) {
        val w = focusedWin() ?: return
        val shell = ShellBridge.shell.value ?: return
        runCatching { shell.injectKey(w.displayId, keyCode, true, 0); shell.injectKey(w.displayId, keyCode, false, 0) }
    }
}
