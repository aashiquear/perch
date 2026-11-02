package io.perch.android.host

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.pm.ServiceInfo
import android.hardware.display.DisplayManager
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbManager
import android.os.Build
import android.os.IBinder
import android.util.Log
import android.view.Display
import io.perch.android.MainActivity
import io.perch.android.R
import io.perch.android.native.PerchNative
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import java.io.File
import kotlin.concurrent.thread

/**
 * Host mode. Owns:
 *  - the USB connection to the Rokid IMU interface (fd → Rust `ar-drivers`),
 *  - the native tracker (1 kHz on a dedicated thread),
 *  - the [GlassesPresentation] on the external (glasses) display.
 *
 * It is a foreground service so Android keeps it alive with the screen off.
 */
class GlassesService : Service() {

    companion object {
        private const val TAG = "GlassesService"
        private const val ACTION_USB_PERMISSION = "io.perch.android.USB_PERMISSION"
        private const val CHANNEL = "perch"
        private const val NOTIF_ID = 1
        const val ROKID_VID = 0x04d2
        const val ROKID_PID = 0x162f

        // Rokid Max: ar-drivers measured 13 ms display delay.
        private const val DISPLAY_DELAY_US = 13_000L

        /** Latest pose for UI & renderer: qw,qx,qy,qz, wx,wy,wz, conf. */
        val pose: MutableStateFlow<FloatArray> = MutableStateFlow(floatArrayOf(1f, 0f, 0f, 0f, 0f, 0f, 0f, 0f))
        val status: MutableStateFlow<String> = MutableStateFlow("idle")
        val calibrationState: MutableStateFlow<Int> = MutableStateFlow(0)
        val imuHz: MutableStateFlow<Int> = MutableStateFlow(0)

        @Volatile var tracker: Long = 0L
            private set

        fun start(ctx: Context, useMock: Boolean = false) {
            val i = Intent(ctx, GlassesService::class.java).putExtra("mock", useMock)
            ctx.startForegroundService(i)
        }
        /**
         * Hand a granted screen-capture result to the service. Android 14+ only
         * lets a service *become* a mediaProjection foreground service after the
         * user accepted the prompt, and `getMediaProjection` must run after that
         * promotion — so the whole mirror path goes through the service.
         */
        fun startMirror(ctx: Context, resultCode: Int, data: Intent) {
            val i = Intent(ctx, GlassesService::class.java)
                .putExtra("projection_code", resultCode)
                .putExtra("projection_data", data)
            ctx.startForegroundService(i)
        }
        /** Names of all displays the phone sees, for the panel ("is DeX holding the glasses?"). */
        val displayNames: MutableStateFlow<String> = MutableStateFlow("")
        fun stop(ctx: Context) = ctx.stopService(Intent(ctx, GlassesService::class.java))
        fun recenter() { if (tracker != 0L) PerchNative.trackerRecenter(tracker) }
        @Volatile private var instance: GlassesService? = null
        /** Application context for callers without one (AppWindows.relaunch). */
        val appContext: Context? get() = instance?.applicationContext
        /** The glasses presentation, once the external display is up. */
        val presentation: GlassesPresentation? get() = instance?.presentation
        /** True while the glasses display is attached. */
        val glassesAttached: MutableStateFlow<Boolean> = MutableStateFlow(false)
        fun calibrate(seconds: Float = 20f) { if (tracker != 0L) PerchNative.trackerCalibrate(tracker, seconds) }
    }

    private var glasses: Long = 0L
    private var imuThread: Thread? = null
    @Volatile private var running = false
    private var presentation: GlassesPresentation? = null
    private var displayListener: DisplayManager.DisplayListener? = null

    private val usbReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            when (intent.action) {
                ACTION_USB_PERMISSION -> {
                    val dev: UsbDevice? = intent.getParcelableExtra(UsbManager.EXTRA_DEVICE)
                    if (intent.getBooleanExtra(UsbManager.EXTRA_PERMISSION_GRANTED, false) && dev != null) openDevice(dev)
                    else status.value = "USB permission denied"
                }
                UsbManager.ACTION_USB_DEVICE_DETACHED -> stopImu("glasses unplugged")
            }
        }
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onCreate() {
        super.onCreate()
        instance = this
        createChannel()
        val filter = IntentFilter().apply {
            addAction(ACTION_USB_PERMISSION)
            addAction(UsbManager.ACTION_USB_DEVICE_DETACHED)
        }
        if (Build.VERSION.SDK_INT >= 33) registerReceiver(usbReceiver, filter, RECEIVER_NOT_EXPORTED)
        else registerReceiver(usbReceiver, filter)
    }

    private var projecting = false

    /**
     * Foreground with the `connectedDevice` type only; `mediaProjection` is
     * added by [promoteToProjection] once the user granted a capture. Declaring
     * it up front throws SecurityException on Android 14+ (the old crash on Start).
     */
    private fun foreground(withProjection: Boolean): Boolean {
        var type = ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE
        if (withProjection) type = type or ServiceInfo.FOREGROUND_SERVICE_TYPE_MEDIA_PROJECTION
        return try {
            startForeground(NOTIF_ID, notification(), type)
            true
        } catch (e: Exception) {
            Log.e(TAG, "startForeground: $e")
            status.value = "foreground service refused: ${e.message?.take(120)}"
            false
        }
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        if (!projecting && !foreground(false)) { stopSelf(); return START_NOT_STICKY }

        val code = intent?.getIntExtra("projection_code", Int.MIN_VALUE) ?: Int.MIN_VALUE
        if (code != Int.MIN_VALUE) {
            val data: Intent? = intent?.getParcelableExtra("projection_data")
            if (data != null) promoteToProjection(code, data)
            if (glasses != 0L) return START_STICKY   // already tracking: nothing else to start
        }

        if (glasses == 0L) {
            if (intent?.getBooleanExtra("mock", false) == true) startImu(PerchNative.glassesOpenMock(), "mock glasses")
            else findAndOpen()
        }
        if (displayListener == null) watchDisplays()
        return START_STICKY
    }

    private fun promoteToProjection(resultCode: Int, data: Intent) {
        if (!projecting) {
            if (!foreground(true)) return
            projecting = true
        }
        val mp = try { MirrorSource.fromResult(this, resultCode, data) } catch (e: Exception) {
            status.value = "screen capture refused: ${e.message?.take(120)}"; null
        } ?: return
        // Mirror at the phone's real pixel size so text is not resampled, and give
        // the window the same aspect so nothing stretches (1080×2340 was a guess).
        val b = (getSystemService(WINDOW_SERVICE) as android.view.WindowManager).maximumWindowMetrics.bounds
        val (w, h) = b.width() to b.height()
        val rowH = 0.8f
        addWindow(MirrorSource(this, mp), PerchNative.SOURCE_PHONE, w, h, rowH * w / h, rowH) {}
    }

    private fun findAndOpen() {
        val usb = getSystemService(USB_SERVICE) as UsbManager
        val dev = usb.deviceList.values.firstOrNull { it.vendorId == ROKID_VID && it.productId == ROKID_PID }
        if (dev == null) {
            val seen = usb.deviceList.values.joinToString { "%04x:%04x %s".format(it.vendorId, it.productId, it.productName ?: "") }
            status.value = if (seen.isEmpty()) "waiting for Rokid glasses: no USB devices visible to the phone"
            else "waiting for Rokid glasses (04d2:162f); USB sees: $seen"
            return
        }
        if (usb.hasPermission(dev)) openDevice(dev)
        else {
            val pi = PendingIntent.getBroadcast(
                this, 0, Intent(ACTION_USB_PERMISSION).setPackage(packageName),
                PendingIntent.FLAG_MUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
            )
            usb.requestPermission(dev, pi)
            status.value = "requesting USB permission"
        }
    }

    private fun openDevice(dev: UsbDevice) {
        val usb = getSystemService(USB_SERVICE) as UsbManager
        val conn = usb.openDevice(dev) ?: run { status.value = "openDevice failed"; return }
        // libusb needs the raw fd; we keep `conn` alive for the service lifetime.
        val fd = conn.fileDescriptor
        val handle = PerchNative.glassesOpenFd(fd)
        if (handle == 0L) {
            status.value = "USB open failed: ${PerchNative.lastError() ?: "unknown native error"}"
            conn.close()
            return
        }
        // 120 Hz mono: lowest latency for a 3DOF desktop.
        PerchNative.glassesSetMode(handle, 2)
        startImu(handle, dev.productName ?: "Rokid")
    }

    private fun startImu(handle: Long, name: String) {
        stopImu(null)
        glasses = handle
        tracker = PerchNative.trackerNew(DISPLAY_DELAY_US, loadCalibration())
        running = true
        status.value = "tracking $name"
        imuThread = thread(name = "perch-imu", priority = Thread.MAX_PRIORITY) {
            val t2 = IntArray(2)
            val imu = FloatArray(9)
            val out = FloatArray(8)
            var n = 0
            var lastHz = System.nanoTime()
            var lastCal = -1
            while (running) {
                when (PerchNative.glassesNext(glasses, t2, imu)) {
                    0 -> {
                        val tUs = (t2[1].toLong() shl 32) or (t2[0].toLong() and 0xffffffffL)
                        PerchNative.trackerFeed(tracker, tUs, imu, true, out)
                        pose.value = out.copyOf()
                        n++
                        val now = System.nanoTime()
                        if (now - lastHz > 1_000_000_000L) {
                            imuHz.value = n; n = 0; lastHz = now
                            val cs = PerchNative.trackerCalibrationState(tracker)
                            if (cs != lastCal) {
                                calibrationState.value = cs
                                if (cs == 200 && lastCal in 1..100) saveCalibration()
                                lastCal = cs
                            }
                        }
                    }
                    1 -> if (t2[0] == 0) PerchNative.trackerRecenter(tracker)   // temple button → recenter
                    else -> { running = false; status.value = "glasses disconnected" }
                }
            }
        }
    }

    private fun stopImu(reason: String?) {
        running = false
        imuThread?.join(500)
        imuThread = null
        if (glasses != 0L) { PerchNative.glassesFree(glasses); glasses = 0L }
        if (tracker != 0L) { PerchNative.trackerFree(tracker); tracker = 0L }
        reason?.let { status.value = it }
    }

    // --- external display -------------------------------------------------
    private fun watchDisplays() {
        val dm = getSystemService(DISPLAY_SERVICE) as DisplayManager
        fun attach() {
            displayNames.value = dm.displays.joinToString(" · ") { "${it.displayId}:${it.name} ${it.mode.physicalWidth}×${it.mode.physicalHeight}" }
            val ext = dm.displays.firstOrNull { it.displayId != Display.DEFAULT_DISPLAY && it.isValid }
            if (ext != null && presentation == null) {
                Log.i(TAG, "presenting on ${ext.name} ${ext.mode.physicalWidth}x${ext.mode.physicalHeight}")
                try {
                    presentation = GlassesPresentation(this, ext).also { it.show() }
                    glassesAttached.value = true
                } catch (e: Exception) {
                    Log.e(TAG, "presentation: $e")
                    status.value = "cannot present on ${ext.name}: ${e.message?.take(100)}"
                    presentation = null
                }
            } else if (ext == null && presentation != null) {
                presentation?.dismiss(); presentation = null
                glassesAttached.value = false
            }
        }
        displayListener = object : DisplayManager.DisplayListener {
            override fun onDisplayAdded(id: Int) = attach()
            override fun onDisplayRemoved(id: Int) = attach()
            override fun onDisplayChanged(id: Int) {}
        }.also { dm.registerDisplayListener(it, null) }
        attach()
    }

    // --- persistence -------------------------------------------------------
    private fun calFile() = File(filesDir, "calibration.json")
    private fun loadCalibration(): String? = calFile().takeIf { it.exists() }?.readText()
    private fun saveCalibration() {
        PerchNative.trackerCalibrationJson(tracker)?.let { calFile().writeText(it) }
    }

    private val sources = ArrayList<WindowSource>()

    /** Create a spatial window dead ahead and start `src` drawing into it. */
    fun addWindow(src: WindowSource, kind: Int, w: Int, h: Int, widthM: Float, heightM: Float, onIndex: (Int) -> Unit) {
        val p = presentation ?: run { status.value = "no glasses display yet"; return }
        p.addWindow(kind, w, h, 0f, 0f, 2f, widthM, heightM, PerchNative.ANCHOR_WORLD) { index, surface ->
            src.start(surface, w, h); sources += src; onIndex(index)
        }
    }

    override fun onDestroy() {
        instance = null
        AppWindows.releaseAll()
        sources.forEach { it.stop() }
        stopImu("stopped")
        presentation?.dismiss()
        displayListener?.let { (getSystemService(DISPLAY_SERVICE) as DisplayManager).unregisterDisplayListener(it) }
        unregisterReceiver(usbReceiver)
        super.onDestroy()
    }

    private fun createChannel() {
        val nm = getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(NotificationChannel(CHANNEL, getString(R.string.notif_channel), NotificationManager.IMPORTANCE_LOW))
    }

    private fun notification(): Notification {
        val pi = PendingIntent.getActivity(this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE)
        return Notification.Builder(this, CHANNEL)
            .setContentTitle("Perch is driving your glasses")
            .setContentText("Tap to open controls")
            .setSmallIcon(android.R.drawable.ic_menu_compass)
            .setContentIntent(pi)
            .setOngoing(true)
            .build()
    }
}
