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
import kotlin.math.abs
import kotlin.math.hypot
import kotlin.math.ln

/**
 * The phone-as-host window list: one entry per app shown on the glasses,
 * each backed by its own trusted virtual display (through [ShellBridge]).
 * Also owns the cursor: a position inside the focused window that the
 * pop-up touchpad moves and that taps/scrolls are injected at.
 */
object AppWindows : InputTarget {
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
        /** The app set FLAG_SECURE: the display shows black. Nothing a non-system app can do about it. */
        val secure: Boolean = false,
        /** Surface.ROTATION_* the app put its display in (the display rotates with content). */
        val rotation: Int = 0,
    ) {
        /** Buffer size (what the display was created with). */
        val bufW: Int get() = if (portrait) PORTRAIT_W else PORTRAIT_H
        val bufH: Int get() = if (portrait) PORTRAIT_H else PORTRAIT_W
        /** Logical size the app sees; cursor and injected touches use this. */
        val width: Int get() = if (rotation % 2 == 0) bufW else bufH
        val height: Int get() = if (rotation % 2 == 0) bufH else bufW
        val landscape: Boolean get() = width > height
    }

    /** From the service's display listener. */
    fun onDisplayRotation(displayId: Int, rotation: Int) {
        val win = windows.value.firstOrNull { it.displayId == displayId } ?: return
        if (win.rotation == rotation) return
        windows.value = windows.value.map { if (it.id == win.id) it.copy(rotation = rotation) else it }
        GlassesService.presentation?.setRotation(win.layer, rotation)
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
            // Secure (DRM/banking) apps render black on any non-secure display; tell the user.
            android.os.Handler(android.os.Looper.getMainLooper()).postDelayed({ checkSecure(win.id) }, 3000)
        }
    }

    fun focus(id: Int) {
        focused.value = id
        cursorU = 0.5f; cursorV = 0.5f
        GlassesService.presentation?.setCursor(windows.value.firstOrNull { it.id == id }?.layer, cursorU, cursorV)
    }

    override fun focusNext(step: Int) {
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
        val ok = try { shell.resizeDisplay(win.displayId, flipped.bufW, flipped.bufH, DPI) } catch (e: Throwable) { false }
        if (!ok) { lastError.value = "resize failed"; return }
        val (wm, hm) = metres(flipped.bufW, flipped.bufH)
        GlassesService.presentation?.resizeWindow(win.layer, flipped.bufW, flipped.bufH, wm, hm)
        windows.value = windows.value.map { if (it.id == id) flipped else it }
    }

    fun remove(id: Int) {
        val win = windows.value.firstOrNull { it.id == id } ?: return
        // End the app's tasks on this display first so it leaves Recents, then drop the display.
        runCatching { ShellBridge.shell.value?.closeApp(win.displayId, win.pkg) }
            .onSuccess { if (!it.isNullOrEmpty()) Log.i(TAG, "closeApp: $it") }
        runCatching { ShellBridge.shell.value?.releaseDisplay(win.displayId) }
        GlassesService.presentation?.hideWindow(win.layer)
        windows.value = windows.value.filter { it.id != id }
        if (focused.value == id) focused.value = windows.value.lastOrNull()?.id
        GlassesService.presentation?.setCursor(windows.value.firstOrNull { it.id == focused.value }?.layer, 0.5f, 0.5f)
    }

    fun releaseAll() {
        windows.value.map { it.id }.forEach { remove(it) }
        // Undo anything Perch set on the phone's own display.
        runCatching { ShellBridge.shell.value?.resetOrientation(-1) }
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

    override val supportsWindowOps: Boolean get() = true
    override fun pinHere() { focused.value?.let { pinHere(it) } }
    override fun flip() { focused.value?.let { flip(it) } }
    override fun bigger() { focused.value?.let { bigger(it) } }
    override fun smaller() { focused.value?.let { smaller(it) } }
    override fun nearer() { focused.value?.let { nearer(it) } }
    override fun further() { focused.value?.let { further(it) } }
    override fun relaunch() { focused.value?.let { relaunch(it) } }

    fun pinHere(id: Int) {
        windows.value.firstOrNull { it.id == id }?.let { GlassesService.presentation?.pinHere(it.layer) }
    }

    // ---- cursor / injection -------------------------------------------------

    private fun focusedWin(): Win? = windows.value.firstOrNull { it.id == focused.value }

    /** Move the cursor by trackpad pixels (already accelerated by the gesture layer). */
    override fun cursorMove(dx: Float, dy: Float) {
        val w = focusedWin() ?: return
        cursorU = (cursorU + dx / w.width).coerceIn(0f, 1f)
        cursorV = (cursorV + dy / w.height).coerceIn(0f, 1f)
        GlassesService.presentation?.setCursor(w.layer, cursorU, cursorV)
        if (mouseMode.value) {
            injectMouse(mouseEvent(if (mouseButtonDown) MotionEvent.ACTION_MOVE else MotionEvent.ACTION_HOVER_MOVE,
                if (mouseButtonDown) MotionEvent.BUTTON_PRIMARY else 0, dx, dy))
            return
        }
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

    /** Tap = touch down/up at the cursor (or a mouse click in mouse mode). */
    override fun tap() { pressDown(); pressUp() }

    override fun pressDown() {
        if (mouseMode.value) {
            mouseButtonDown = true
            injectMouse(mouseEvent(MotionEvent.ACTION_DOWN, MotionEvent.BUTTON_PRIMARY))
            injectMouse(mouseEvent(MotionEvent.ACTION_BUTTON_PRESS, MotionEvent.BUTTON_PRIMARY))
            return
        }
        dragging = true; touch(MotionEvent.ACTION_DOWN)
    }
    override fun pressUp() {
        if (mouseButtonDown) {
            mouseButtonDown = false
            injectMouse(mouseEvent(MotionEvent.ACTION_BUTTON_RELEASE, 0))
            injectMouse(mouseEvent(MotionEvent.ACTION_UP, 0))
            return
        }
        if (dragging) { dragging = false; touch(MotionEvent.ACTION_UP) }
    }

    // ---- two-finger scroll as a touch drag (what the app expects from a finger) ----
    private var scrollDrag = false
    private var scrollU = 0f; private var scrollV = 0f
    fun scrollStart() {
        val w = focusedWin() ?: return
        if (mouseMode.value) return
        scrollDrag = true; scrollU = cursorU; scrollV = cursorV
        touchAt(MotionEvent.ACTION_DOWN, scrollU, scrollV, w)
    }
    /** dx, dy in trackpad px: content follows the fingers. */
    override fun scrollMove(dx: Float, dy: Float) {
        val w = focusedWin() ?: return
        if (mouseMode.value) { scroll(if (dx > 0) 1f else if (dx < 0) -1f else 0f, if (dy < 0) 1f else if (dy > 0) -1f else 0f); return }
        if (!scrollDrag) scrollStart()
        scrollU = (scrollU + dx / w.width).coerceIn(0f, 1f); scrollV = (scrollV + dy / w.height).coerceIn(0f, 1f)
        touchAt(MotionEvent.ACTION_MOVE, scrollU, scrollV, w)
    }
    override fun scrollEnd() {
        val w = focusedWin() ?: return
        if (!scrollDrag) return
        scrollDrag = false
        touchAt(MotionEvent.ACTION_UP, scrollU, scrollV, w)
    }
    private fun touchAt(action: Int, u: Float, v: Float, w: Win) {
        val shell = ShellBridge.shell.value ?: return
        val now = SystemClock.uptimeMillis()
        if (action == MotionEvent.ACTION_DOWN) downTime = now
        val ev = MotionEvent.obtain(downTime, now, action, u * w.width, v * w.height, 0)
        ev.source = InputDevice.SOURCE_TOUCHSCREEN
        runCatching { shell.injectMotion(w.displayId, ev) }
        ev.recycle()
    }

    // ---- two-finger pinch: a two-finger touch spreading around the cursor (mouse mode: Ctrl+wheel) ----
    private var pinching = false
    /** Distance between the two injected fingers, in window px. */
    private var pinchSpan = 0f
    private var pinchAcc = 0f
    override fun pinch(scale: Float) {
        val w = focusedWin() ?: return
        if (scale <= 0f) return
        if (mouseMode.value) {
            // One wheel notch per ~10 % change in finger span.
            val step = ln(1.1f)
            pinchAcc += ln(scale)
            while (abs(pinchAcc) >= step) {
                val dir = if (pinchAcc > 0) 1f else -1f
                pinchAcc -= dir * step
                wheel(w, 0f, dir, android.view.KeyEvent.META_CTRL_ON or android.view.KeyEvent.META_CTRL_LEFT_ON)
            }
            return
        }
        if (!pinching) {
            if (scrollDrag) scrollEnd()
            pinching = true
            pinchSpan = minOf(w.width, w.height) * 0.25f
            pinchTouch(MotionEvent.ACTION_DOWN, 1, w)
            pinchTouch(MotionEvent.ACTION_POINTER_DOWN or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), 2, w)
        }
        pinchSpan = (pinchSpan * scale).coerceIn(24f, hypot(w.width.toFloat(), w.height.toFloat()))
        pinchTouch(MotionEvent.ACTION_MOVE, 2, w)
    }
    override fun pinchEnd() {
        pinchAcc = 0f
        if (!pinching) return
        pinching = false
        val w = focusedWin() ?: return
        pinchTouch(MotionEvent.ACTION_POINTER_UP or (1 shl MotionEvent.ACTION_POINTER_INDEX_SHIFT), 2, w)
        pinchTouch(MotionEvent.ACTION_UP, 1, w)
    }
    /** Fingers sit left and right of the cursor, [pinchSpan] apart, clamped to the window. */
    private fun pinchTouch(action: Int, pointers: Int, w: Win) {
        val shell = ShellBridge.shell.value ?: return
        val now = SystemClock.uptimeMillis()
        if (action == MotionEvent.ACTION_DOWN) downTime = now
        val cx = cursorU * w.width; val cy = cursorV * w.height; val half = pinchSpan / 2
        val props = Array(pointers) { i -> MotionEvent.PointerProperties().apply { id = i; toolType = MotionEvent.TOOL_TYPE_FINGER } }
        val coords = Array(pointers) { i ->
            MotionEvent.PointerCoords().apply {
                x = (if (i == 0) cx - half else cx + half).coerceIn(0f, w.width - 1f)
                y = cy; pressure = 1f; size = 1f
            }
        }
        val ev = MotionEvent.obtain(downTime, now, action, pointers, props, coords, 0, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_TOUCHSCREEN, 0)
        runCatching { shell.injectMotion(w.displayId, ev) }.onFailure { Log.w(TAG, "inject pinch: $it") }
        ev.recycle()
    }

    /** Two-finger scroll → mouse wheel at the cursor (lines; dy positive = up, dx positive = right). */
    override fun scroll(dx: Float, dy: Float) {
        val w = focusedWin() ?: return
        wheel(w, dx, dy, 0)
    }
    private fun wheel(w: Win, dx: Float, dy: Float, metaState: Int) {
        val shell = ShellBridge.shell.value ?: return
        val now = SystemClock.uptimeMillis()
        val props = arrayOf(MotionEvent.PointerProperties().apply { id = 0; toolType = MotionEvent.TOOL_TYPE_MOUSE })
        val coords = arrayOf(MotionEvent.PointerCoords().apply {
            x = cursorU * w.width; y = cursorV * w.height
            setAxisValue(MotionEvent.AXIS_VSCROLL, dy)
            setAxisValue(MotionEvent.AXIS_HSCROLL, dx)
        })
        val ev = MotionEvent.obtain(now, now, MotionEvent.ACTION_SCROLL, 1, props, coords, metaState, 0, 1f, 1f, 0, 0, InputDevice.SOURCE_MOUSE, 0)
        runCatching { shell.injectMotion(w.displayId, ev) }
        ev.recycle()
    }

    /** When true, physical keyboard / mouse / gamepad events reaching the Perch activity go to the focused window. */
    val routeHardware: MutableStateFlow<Boolean> = MutableStateFlow(true)

    /** A key event from a real device (not the on-screen keyboard). True if consumed. */
    fun hardwareKey(ev: android.view.KeyEvent): Boolean {
        // Our own virtual controller's HID events land here whenever the phone was touched last
        // (the focused display is then the phone); pass them on with their device identity intact.
        val own = gamepadId >= 0 && ev.deviceId == gamepadId
        if (!own && (!routeHardware.value || ev.deviceId <= 0)) return false
        val w = focusedWin() ?: return false
        val shell = ShellBridge.shell.value ?: return false
        val copy = android.view.KeyEvent(ev)
        return runCatching { shell.injectKeyEvent(w.displayId, copy) }.getOrDefault(false)
    }

    /** Mouse / trackpad / joystick motion from a real device. True if consumed. */
    fun hardwareMotion(ev: MotionEvent): Boolean {
        val own = gamepadId >= 0 && ev.deviceId == gamepadId
        if (!own && (!routeHardware.value || ev.deviceId <= 0)) return false
        val w = focusedWin() ?: return false
        val src = ev.source
        if (src and InputDevice.SOURCE_JOYSTICK == InputDevice.SOURCE_JOYSTICK) {
            val shell = ShellBridge.shell.value ?: return false
            val copy = MotionEvent.obtain(ev)
            val ok = runCatching { shell.injectMotion(w.displayId, copy) }.getOrDefault(false)
            copy.recycle(); return ok
        }
        // A mouse is handled through pointer capture (see MainActivity): the phone's
        // pointer is hidden and we get relative motion, so nothing lands on the panel.
        return false
    }

    /** Captured-pointer event (SOURCE_MOUSE_RELATIVE): x/y are deltas. */
    fun capturedMotion(ev: MotionEvent): Boolean {
        if (focusedWin() == null) return false
        when (ev.actionMasked) {
            MotionEvent.ACTION_MOVE -> cursorMove(ev.x * 1.5f, ev.y * 1.5f)
            MotionEvent.ACTION_BUTTON_PRESS -> pressDown()
            MotionEvent.ACTION_BUTTON_RELEASE -> pressUp()
            MotionEvent.ACTION_SCROLL -> scroll(ev.getAxisValue(MotionEvent.AXIS_HSCROLL), ev.getAxisValue(MotionEvent.AXIS_VSCROLL))
            else -> {}
        }
        return true
    }

    /** Ask the helper whether the app on this display refuses projection; label the card if so. */
    fun checkSecure(id: Int) {
        val win = windows.value.firstOrNull { it.id == id } ?: return
        val shell = ShellBridge.shell.value ?: return
        val secure = runCatching { shell.isSecure(win.displayId) }.getOrDefault(false)
        if (secure != win.secure) windows.value = windows.value.map { if (it.id == id) it.copy(secure = secure) else it }
        if (secure) lastError.value = "${win.label} blocks screen projection (FLAG_SECURE) and shows black on the glasses"
    }

    // ---- glasses dimming -----------------------------------------------------
    val brightness: MutableStateFlow<Float> = MutableStateFlow(1f)
    /** True once the hardware path (cmd display set-brightness on the glasses display) has worked. */
    val hardwareBrightness: MutableStateFlow<Boolean> = MutableStateFlow(false)
    fun setBrightness(b: Float) {
        brightness.value = b
        // Software dimming is what visibly works on the Rokid Max (Android accepts a per-display
        // brightness for it, but the panel ignores the stored value). Set both anyway.
        GlassesService.presentation?.setBrightness(b)
        val shell = ShellBridge.shell.value
        val did = GlassesService.glassesDisplayId
        if (shell != null && did >= 0) {
            val out = runCatching { shell.setBrightness(did, b) }.getOrElse { it.toString() }
            hardwareBrightness.value = out.isEmpty()
            if (out.isNotEmpty()) Log.w(TAG, "hardware brightness: $out")
        }
    }

    // ---- mouse mode: the cursor is a real mouse to the app (cloud gaming, remote desktops) ----
    override val mouseMode: MutableStateFlow<Boolean> = MutableStateFlow(false)
    private fun mouseEvent(action: Int, buttons: Int, dx: Float = 0f, dy: Float = 0f, vscroll: Float = 0f, hscroll: Float = 0f): MotionEvent {
        val w = focusedWin()!!
        val now = SystemClock.uptimeMillis()
        if (action == MotionEvent.ACTION_DOWN) downTime = now
        val props = arrayOf(MotionEvent.PointerProperties().apply { id = 0; toolType = MotionEvent.TOOL_TYPE_MOUSE })
        val coords = arrayOf(MotionEvent.PointerCoords().apply {
            x = cursorU * w.width; y = cursorV * w.height
            setAxisValue(MotionEvent.AXIS_RELATIVE_X, dx); setAxisValue(MotionEvent.AXIS_RELATIVE_Y, dy)
            setAxisValue(MotionEvent.AXIS_VSCROLL, vscroll); setAxisValue(MotionEvent.AXIS_HSCROLL, hscroll)
        })
        return MotionEvent.obtain(downTime, now, action, 1, props, coords, 0, buttons, 1f, 1f, -1, 0, InputDevice.SOURCE_MOUSE, 0)
    }
    private fun injectMouse(ev: MotionEvent) {
        val w = focusedWin() ?: return
        runCatching { ShellBridge.shell.value?.injectMotion(w.displayId, ev) }
        ev.recycle()
    }
    private var mouseButtonDown = false

    // ---- virtual gamepad ------------------------------------------------------
    private var gamepadId = -1
    val gamepadError: MutableStateFlow<String?> = MutableStateFlow(null)
    /** Human-readable state for the pop-up: which device id speaks, last inject result. */
    val gamepadInfo: MutableStateFlow<String> = MutableStateFlow("controller: not created yet")
    private fun describeDevice(id: Int): String {
        val d = InputDevice.getDevice(id)
        return if (d == null) "id $id (no InputDevice)" else "${d.name} (id $id${if (d.isVirtual) ", virtual" else ""})"
    }
    /** Axis state: X Y Z RZ in -1..1, BRAKE GAS 0..1, HAT_X HAT_Y in -1/0/1. */
    override val axes = FloatArray(8)
    private var lastAxesSend = 0L

    /** A real controller paired to the phone, if any (it is forwarded as-is, see hardwareMotion).
     *  Cached: enumerating devices is a binder round-trip per device, far too slow per stick move. */
    private var physicalCache: Int? = null
    private var physicalCacheAt = 0L
    private fun physicalGamepad(): Int? {
        val now = SystemClock.uptimeMillis()
        if (now - physicalCacheAt < 3000) return physicalCache
        physicalCacheAt = now
        physicalCache = InputDevice.getDeviceIds().firstOrNull { id ->
            val d = InputDevice.getDevice(id) ?: return@firstOrNull false
            !d.isVirtual && d.name != "Perch Gamepad" &&
                (d.sources and InputDevice.SOURCE_GAMEPAD == InputDevice.SOURCE_GAMEPAD || d.sources and InputDevice.SOURCE_JOYSTICK == InputDevice.SOURCE_JOYSTICK)
        }
        return physicalCache
    }
    /** A real mouse paired to the phone (pointer capture is only worth requesting then). */
    fun physicalMouse(): Boolean = InputDevice.getDeviceIds().any { id ->
        val d = InputDevice.getDevice(id); d != null && !d.isVirtual && d.sources and InputDevice.SOURCE_MOUSE == InputDevice.SOURCE_MOUSE
    }

    // ---- HID transport: real kernel input through the virtual device ----
    /** true = write HID reports (Chrome, cloud gaming and Unity all see a real controller); false = inject per display. */
    val hidTransport: MutableStateFlow<Boolean> = MutableStateFlow(true)
    private var buttonBits = 0
    private val report = ByteArray(13)
    private fun hidButtonBit(keyCode: Int): Int = when (keyCode) {
        android.view.KeyEvent.KEYCODE_BUTTON_A -> 0; android.view.KeyEvent.KEYCODE_BUTTON_B -> 1
        android.view.KeyEvent.KEYCODE_BUTTON_C -> 2; android.view.KeyEvent.KEYCODE_BUTTON_X -> 3
        android.view.KeyEvent.KEYCODE_BUTTON_Y -> 4; android.view.KeyEvent.KEYCODE_BUTTON_Z -> 5
        android.view.KeyEvent.KEYCODE_BUTTON_L1 -> 6; android.view.KeyEvent.KEYCODE_BUTTON_R1 -> 7
        android.view.KeyEvent.KEYCODE_BUTTON_L2 -> 8; android.view.KeyEvent.KEYCODE_BUTTON_R2 -> 9
        android.view.KeyEvent.KEYCODE_BUTTON_SELECT -> 10; android.view.KeyEvent.KEYCODE_BUTTON_START -> 11
        android.view.KeyEvent.KEYCODE_BUTTON_MODE -> 12; android.view.KeyEvent.KEYCODE_BUTTON_THUMBL -> 13
        android.view.KeyEvent.KEYCODE_BUTTON_THUMBR -> 14
        else -> -1
    }
    /** Build and send the HID report from `axes` + `buttonBits`. */
    private fun sendReport(): Boolean {
        val shell = ShellBridge.shell.value ?: return false
        fun s16(v: Float): Int = (v.coerceIn(-1f, 1f) * 32767f).toInt()
        var i = 0
        for (k in 0..3) { val v = s16(axes[k]); report[i++] = (v and 0xff).toByte(); report[i++] = ((v shr 8) and 0xff).toByte() }
        report[i++] = (axes[4].coerceIn(0f, 1f) * 255f).toInt().toByte()
        report[i++] = (axes[5].coerceIn(0f, 1f) * 255f).toInt().toByte()
        val hx = axes[6].toInt(); val hy = axes[7].toInt()
        report[i++] = when {
            hx == 0 && hy < 0 -> 1; hx > 0 && hy < 0 -> 2; hx > 0 && hy == 0 -> 3; hx > 0 && hy > 0 -> 4
            hx == 0 && hy > 0 -> 5; hx < 0 && hy > 0 -> 6; hx < 0 && hy == 0 -> 7; hx < 0 && hy < 0 -> 8
            else -> 0
        }.toByte()
        report[i++] = (buttonBits and 0xff).toByte()
        report[i] = ((buttonBits shr 8) and 0x7f).toByte()
        return runCatching { shell.gamepadReport(report) }.getOrDefault(false)
    }
    /** Whether the HID path is live (device exists and the last report went through). */
    private fun hidReady(): Boolean = hidTransport.value && physicalGamepad() == null && gamepadDevice() >= 0

    private fun gamepadDevice(): Int {
        // Safeguard: with a Bluetooth controller attached, the on-screen pad speaks as that
        // controller instead of adding a second one (games would otherwise see player 2).
        physicalGamepad()?.let { if (gamepadId != it) gamepadInfo.value = "controller: physical ${describeDevice(it)}"; gamepadId = it; return it }
        // The virtual device can vanish (helper restarted); re-check it is still there.
        if (gamepadId >= 0 && InputDevice.getDevice(gamepadId) == null) gamepadId = -1
        if (gamepadId < 0) {
            val shell = ShellBridge.shell.value ?: return -1
            gamepadId = runCatching { shell.gamepadDeviceId() }.getOrDefault(-1)
            gamepadError.value = if (gamepadId < 0) (runCatching { shell.lastError() }.getOrNull() ?: "no gamepad device") else null
            gamepadInfo.value = if (gamepadId < 0) "controller: none (events carry no device)" else "controller: ${describeDevice(gamepadId)}"
        }
        return gamepadId
    }

    /** Make the controller known again: zero axes + no buttons, sent when a gamepad skin opens;
     *  also moves Android's focused display to the game's so kernel HID events go straight there. */
    override fun gamepadAnnounce() {
        java.util.Arrays.fill(axes, 0f); buttonBits = 0
        focusGameDisplay()
        gamepadAxes(force = true)
    }

    /** Injecting any key into a display makes it the focused display (seen in InputDispatcher logs). */
    fun focusGameDisplay() {
        val w = focusedWin() ?: return
        val shell = ShellBridge.shell.value ?: return
        val now = SystemClock.uptimeMillis()
        val dev = gamepadDevice()
        for (action in intArrayOf(android.view.KeyEvent.ACTION_DOWN, android.view.KeyEvent.ACTION_UP)) {
            val ev = android.view.KeyEvent(now, now, action, android.view.KeyEvent.KEYCODE_UNKNOWN, 0, 0, dev, 0, 0, InputDevice.SOURCE_GAMEPAD)
            runCatching { shell.injectKeyEvent(w.displayId, ev) }
        }
    }

    /** Send the current axes as one joystick move (rate-limited to ~120 Hz). */
    override fun gamepadAxes(force: Boolean) {
        val w = focusedWin() ?: return
        val shell = ShellBridge.shell.value ?: return
        val now = SystemClock.uptimeMillis()
        if (!force && now - lastAxesSend < 12) return
        lastAxesSend = now
        if (hidReady() && sendReport()) return
        val dev = gamepadDevice()
        val props = arrayOf(MotionEvent.PointerProperties().apply { id = 0; toolType = MotionEvent.TOOL_TYPE_UNKNOWN })
        val coords = arrayOf(MotionEvent.PointerCoords().apply {
            setAxisValue(MotionEvent.AXIS_X, axes[0]); setAxisValue(MotionEvent.AXIS_Y, axes[1])
            setAxisValue(MotionEvent.AXIS_Z, axes[2]); setAxisValue(MotionEvent.AXIS_RZ, axes[3])
            setAxisValue(MotionEvent.AXIS_BRAKE, axes[4]); setAxisValue(MotionEvent.AXIS_GAS, axes[5])
            setAxisValue(MotionEvent.AXIS_LTRIGGER, axes[4]); setAxisValue(MotionEvent.AXIS_RTRIGGER, axes[5])
            setAxisValue(MotionEvent.AXIS_HAT_X, axes[6]); setAxisValue(MotionEvent.AXIS_HAT_Y, axes[7])
        })
        val src = InputDevice.SOURCE_JOYSTICK or InputDevice.SOURCE_GAMEPAD
        val ev = MotionEvent.obtain(now, now, MotionEvent.ACTION_MOVE, 1, props, coords, 0, 0, 1f, 1f, dev, 0, src, 0)
        runCatching { shell.injectMotion(w.displayId, ev) }
        ev.recycle()
    }

    /** Gamepad button (KEYCODE_BUTTON_* or DPAD) press/release to the focused window. */
    override fun gamepadButton(keyCode: Int, down: Boolean) {
        val w = focusedWin() ?: return
        val shell = ShellBridge.shell.value ?: return
        if (hidReady()) {
            val bit = hidButtonBit(keyCode)
            if (bit >= 0) buttonBits = if (down) buttonBits or (1 shl bit) else buttonBits and (1 shl bit).inv()
            // Triggers are analog on a real pad (Chrome/Web Gamepad read the axes, not L2/R2): drive both.
            if (keyCode == android.view.KeyEvent.KEYCODE_BUTTON_L2) axes[4] = if (down) 1f else 0f
            if (keyCode == android.view.KeyEvent.KEYCODE_BUTTON_R2) axes[5] = if (down) 1f else 0f
            else when (keyCode) {
                android.view.KeyEvent.KEYCODE_DPAD_UP -> axes[7] = if (down) -1f else 0f
                android.view.KeyEvent.KEYCODE_DPAD_DOWN -> axes[7] = if (down) 1f else 0f
                android.view.KeyEvent.KEYCODE_DPAD_LEFT -> axes[6] = if (down) -1f else 0f
                android.view.KeyEvent.KEYCODE_DPAD_RIGHT -> axes[6] = if (down) 1f else 0f
            }
            val ok = sendReport()
            val base = gamepadInfo.value.substringBefore(" · ")
            gamepadInfo.value = "$base · HID ${android.view.KeyEvent.keyCodeToString(keyCode).removePrefix("KEYCODE_")} ${if (down) "down" else "up"}: ${if (ok) "ok" else "write failed"}"
            if (ok) return
        }
        val now = SystemClock.uptimeMillis()
        val dev = gamepadDevice()
        val ev = android.view.KeyEvent(now, now, if (down) android.view.KeyEvent.ACTION_DOWN else android.view.KeyEvent.ACTION_UP,
            keyCode, 0, 0, dev, 0, 0, InputDevice.SOURCE_GAMEPAD or InputDevice.SOURCE_DPAD)
        val ok = runCatching { shell.injectKeyEvent(w.displayId, ev) }.getOrElse { false }
        val base = gamepadInfo.value.substringBefore(" · ")
        gamepadInfo.value = "$base · last button ${android.view.KeyEvent.keyCodeToString(keyCode).removePrefix("KEYCODE_")} ${if (down) "down" else "up"}: ${if (ok) "ok" else "REFUSED" + (runCatching { shell.lastError() }.getOrNull()?.let { " ($it)" } ?: "")} → display ${w.displayId}"
    }

    /** Arrow keys (suggestion strips, menus) to the focused window; as controller d-pad in gamepad modes
     *  so apps like GeForce NOW stay in gamepad mode instead of switching to keyboard+mouse. */
    override fun arrow(keyCode: Int, asGamepad: Boolean) {
        if (asGamepad) { gamepadButton(keyCode, true); gamepadButton(keyCode, false) } else key(keyCode)
    }

    /** Type text into the focused window as key events (the phone's own keyboard supplies it). */
    override fun typeText(text: String) {
        val w = focusedWin() ?: return
        val shell = ShellBridge.shell.value ?: return
        val kcm = android.view.KeyCharacterMap.load(android.view.KeyCharacterMap.VIRTUAL_KEYBOARD)
        for (ch in text) {
            val events = kcm.getEvents(charArrayOf(ch))
            if (events == null) {
                // Not on the virtual keyboard map (emoji, accents): no key sequence exists for it.
                Log.w(TAG, "typeText: no key events for '$ch'")
                continue
            }
            for (ev in events) runCatching { shell.injectKeyEvent(w.displayId, android.view.KeyEvent(ev)) }
        }
    }

    /** Back key etc. to the focused app. */
    override fun key(keyCode: Int) {
        val w = focusedWin() ?: return
        val shell = ShellBridge.shell.value ?: return
        runCatching { shell.injectKey(w.displayId, keyCode, true, 0); shell.injectKey(w.displayId, keyCode, false, 0) }
    }
}
