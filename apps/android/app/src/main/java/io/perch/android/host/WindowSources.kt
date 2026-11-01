package io.perch.android.host

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.hardware.display.DisplayManager
import android.hardware.display.VirtualDisplay
import android.media.projection.MediaProjection
import android.media.projection.MediaProjectionManager
import android.view.Surface

/**
 * Things that can draw into a spatial window's Surface on Android.
 *
 * 1. [MirrorSource] — MediaProjection mirror of the phone's own screen. Works
 *    on every phone; this is the "phone screen in a floating window" case.
 *    Needs the user to accept the screen-capture prompt once per session.
 *
 * 2. [AppDisplaySource] — a private VirtualDisplay that *apps are launched
 *    into* (`ActivityOptions.setLaunchDisplayId`). This is how you get real
 *    multi-window: each window is its own Android display running a
 *    different app, no mirroring. Caveat: stock Android only lets an app
 *    launch *its own* activities onto a non-trusted virtual display. To
 *    launch arbitrary apps you need one of:
 *      - Samsung DeX "Dual mode"/"force desktop mode" (Developer options →
 *        "Force desktop mode") plus Shizuku to grant
 *        `INTERNAL_SYSTEM_WINDOW`/`ACTIVITY_EMBEDDING`;
 *      - ADB: `adb shell settings put global force_desktop_mode_on_external_displays 1`
 *        (then apps opened on the external display get freeform windows);
 *      - root.
 *    See docs/PLATFORMS.md §Android for the recommended path on the S24 Ultra.
 */
interface WindowSource {
    fun start(surface: Surface, width: Int, height: Int)
    fun stop()
}

class MirrorSource(private val ctx: Context, private val projection: MediaProjection) : WindowSource {
    private var vd: VirtualDisplay? = null
    override fun start(surface: Surface, width: Int, height: Int) {
        val dpi = ctx.resources.displayMetrics.densityDpi
        vd = projection.createVirtualDisplay(
            "perch-mirror", width, height, dpi,
            DisplayManager.VIRTUAL_DISPLAY_FLAG_AUTO_MIRROR,
            surface, null, null,
        )
    }
    override fun stop() { vd?.release(); vd = null }

    companion object {
        /** Call from an Activity; result goes to [fromResult]. */
        fun requestIntent(activity: Activity): Intent =
            (activity.getSystemService(Context.MEDIA_PROJECTION_SERVICE) as MediaProjectionManager).createScreenCaptureIntent()

        fun fromResult(ctx: Context, resultCode: Int, data: Intent): MediaProjection? =
            (ctx.getSystemService(Context.MEDIA_PROJECTION_SERVICE) as MediaProjectionManager).getMediaProjection(resultCode, data)
    }
}

class AppDisplaySource(private val ctx: Context, private val name: String) : WindowSource {
    private var vd: VirtualDisplay? = null
    val displayId: Int get() = vd?.display?.displayId ?: -1

    override fun start(surface: Surface, width: Int, height: Int) {
        val dm = ctx.getSystemService(Context.DISPLAY_SERVICE) as DisplayManager
        val flags = DisplayManager.VIRTUAL_DISPLAY_FLAG_PUBLIC or
            DisplayManager.VIRTUAL_DISPLAY_FLAG_OWN_CONTENT_ONLY or
            DisplayManager.VIRTUAL_DISPLAY_FLAG_PRESENTATION
        vd = dm.createVirtualDisplay(name, width, height, 240, surface, flags)
    }

    /** Launch an activity into this display (own-app activities always work). */
    fun launch(intent: Intent) {
        val opts = android.app.ActivityOptions.makeBasic().setLaunchDisplayId(displayId)
        ctx.startActivity(intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_MULTIPLE_TASK), opts.toBundle())
    }

    override fun stop() { vd?.release(); vd = null }
}
