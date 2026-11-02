package io.perch.android.shell

import android.content.AttributionSource
import android.content.Context
import android.content.ContextWrapper
import android.hardware.display.DisplayManager
import android.hardware.display.VirtualDisplay
import android.os.Build
import android.os.Process
import android.util.Log
import android.view.InputEvent
import android.view.KeyEvent
import android.view.MotionEvent
import android.view.Surface
import org.lsposed.hiddenapibypass.HiddenApiBypass
import java.io.BufferedReader
import java.io.InputStreamReader

/**
 * Runs inside the Shizuku user-service process (uid 2000 = shell), not in
 * the app. This is the same privilege scrcpy's server has, and the same two
 * tricks: trusted virtual displays any app can be launched onto, and
 * `InputManager.injectInputEvent` for input into a chosen display.
 *
 * Keep it small and boring: no state beyond the displays it created.
 */
class PerchShellService(private val context: Context) : IPerchShell.Stub() {

    companion object {
        private const val TAG = "PerchShell"

        // DisplayManager hidden flags (frameworks/base DisplayManager.java).
        const val FLAG_TRUSTED = 1 shl 10
        const val FLAG_OWN_DISPLAY_GROUP = 1 shl 11
        const val FLAG_ALWAYS_UNLOCKED = 1 shl 12
        const val FLAG_TOUCH_FEEDBACK_DISABLED = 1 shl 13
        const val FLAG_OWN_FOCUS = 1 shl 14
        const val FLAG_DEVICE_DISPLAY_GROUP = 1 shl 15
        const val FLAG_SUPPORTS_TOUCH = 1 shl 6
        const val FLAG_ROTATES_WITH_CONTENT = 1 shl 7
        const val FLAG_DESTROY_CONTENT_ON_REMOVAL = 1 shl 8

        // InputManager.injectInputEvent mode: async, no wait.
        private const val INJECT_MODE_ASYNC = 0
    }

    private val displays = HashMap<Int, VirtualDisplay>()
    @Volatile private var lastError = ""

    /**
     * System services check that the package name matches the calling uid.
     * Shizuku hands us a system context whose package is "android", which uid
     * 2000 does not own, so DisplayManager would throw "packageName must match
     * the calling uid". Same trick as scrcpy's FakeContext: claim com.android.shell.
     */
    private class ShellContext(base: Context) : ContextWrapper(base) {
        override fun getPackageName() = "com.android.shell"
        override fun getOpPackageName() = "com.android.shell"
        override fun getAttributionSource(): AttributionSource =
            AttributionSource.Builder(Process.myUid()).setPackageName("com.android.shell").build()
    }

    private val shellContext by lazy { ShellContext(context) }
    private val dm: DisplayManager by lazy {
        // Hidden ctor DisplayManager(Context) binds the manager to our shell context;
        // getSystemService on a wrapper would use the base context's package instead.
        try {
            HiddenApiBypass.newInstance(DisplayManager::class.java, shellContext) as DisplayManager
        } catch (e: Throwable) {
            Log.w(TAG, "DisplayManager(Context) ctor unavailable: $e")
            shellContext.getSystemService(Context.DISPLAY_SERVICE) as DisplayManager
        }
    }

    init {
        if (Build.VERSION.SDK_INT >= 28) HiddenApiBypass.addHiddenApiExemptions("")
        Log.i(TAG, "started as uid ${Process.myUid()}")
    }

    override fun uid(): Int = Process.myUid()
    override fun lastError(): String = lastError

    override fun createDisplay(name: String, width: Int, height: Int, dpi: Int, surface: Surface): Int {
        val flags = DisplayManager.VIRTUAL_DISPLAY_FLAG_PUBLIC or
            DisplayManager.VIRTUAL_DISPLAY_FLAG_OWN_CONTENT_ONLY or
            FLAG_SUPPORTS_TOUCH or FLAG_ROTATES_WITH_CONTENT or FLAG_DESTROY_CONTENT_ON_REMOVAL or
            FLAG_TRUSTED or FLAG_OWN_DISPLAY_GROUP or FLAG_ALWAYS_UNLOCKED or
            FLAG_TOUCH_FEEDBACK_DISABLED or FLAG_OWN_FOCUS
        return try {
            val vd = dm.createVirtualDisplay(name, width, height, dpi, surface, flags)
            val id = vd.display.displayId
            displays[id] = vd
            Log.i(TAG, "display $id: $name ${width}x$height@$dpi flags=0x${flags.toString(16)}")
            id
        } catch (e: Throwable) {
            Log.e(TAG, "createVirtualDisplay failed: $e")
            lastError = "createVirtualDisplay: $e"
            -1
        }
    }

    override fun resizeDisplay(displayId: Int, width: Int, height: Int, dpi: Int): Boolean {
        val vd = displays[displayId] ?: return false
        return try {
            vd.resize(width, height, dpi); true
        } catch (e: Throwable) {
            Log.e(TAG, "resize: $e"); lastError = "resize: $e"; false
        }
    }

    override fun releaseDisplay(displayId: Int) {
        displays.remove(displayId)?.release()
    }

    override fun launch(displayId: Int, component: String): String {
        // `am` is what adb shell uses; as uid shell it may target any display.
        val cmd = listOf(
            "am", "start", "--display", displayId.toString(),
            "--activity-multiple-task", "-n", component,
        )
        return try {
            val p = ProcessBuilder(cmd).redirectErrorStream(true).start()
            val out = BufferedReader(InputStreamReader(p.inputStream)).readText()
            p.waitFor()
            if (p.exitValue() == 0 && !out.contains("Error")) "" else out.trim().also { lastError = "am start: $it" }
        } catch (e: Throwable) {
            "am start failed: $e"
        }
    }

    private val inputManager: Any? by lazy {
        try {
            val cls = Class.forName("android.hardware.input.InputManager")
            if (Build.VERSION.SDK_INT >= 34) shellContext.getSystemService(Context.INPUT_SERVICE)
            else cls.getMethod("getInstance").invoke(null)
        } catch (e: Throwable) {
            Log.e(TAG, "InputManager: $e"); null
        }
    }

    private fun inject(event: InputEvent, displayId: Int): Boolean {
        val im = inputManager ?: return false
        return try {
            HiddenApiBypass.invoke(InputEvent::class.java, event, "setDisplayId", displayId)
            HiddenApiBypass.invoke(im.javaClass, im, "injectInputEvent", event, INJECT_MODE_ASYNC) as Boolean
        } catch (e: Throwable) {
            Log.e(TAG, "inject: $e"); lastError = "inject: $e"; false
        }
    }

    override fun injectMotion(displayId: Int, event: MotionEvent): Boolean = inject(event, displayId)

    override fun injectKey(displayId: Int, keyCode: Int, down: Boolean, metaState: Int): Boolean {
        val now = android.os.SystemClock.uptimeMillis()
        val ev = KeyEvent(now, now, if (down) KeyEvent.ACTION_DOWN else KeyEvent.ACTION_UP, keyCode, 0, metaState)
        return inject(ev, displayId)
    }

    override fun destroy() {
        displays.values.forEach { runCatching { it.release() } }
        displays.clear()
        Log.i(TAG, "destroyed")
        System.exit(0)
    }
}
