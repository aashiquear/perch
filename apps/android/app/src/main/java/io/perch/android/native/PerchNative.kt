package io.perch.android.native

/**
 * Kotlin face of `libperch_jni.so` → `libperch_ffi.so` (crates/perch-ffi).
 *
 * All methods are thin JNI calls; keep logic in Rust so it is shared with
 * the desktop daemon and unit-tested there.
 */
object PerchNative {
    init {
        System.loadLibrary("perch_ffi")
        System.loadLibrary("perch_jni")
    }

    // --- tracker ---------------------------------------------------------
    external fun trackerNew(displayDelayUs: Long, calibrationJson: String?): Long
    external fun trackerFree(t: Long)
    /** imu9 = accel(3) gyro(3) mag(3). out8 = qw,qx,qy,qz, wx,wy,wz, confidence. */
    external fun trackerFeed(t: Long, tUs: Long, imu9: FloatArray, hasMag: Boolean, out8: FloatArray): Int
    external fun trackerPredict(t: Long, extraS: Float, out8: FloatArray): Int
    external fun trackerRecenter(t: Long)
    external fun trackerCalibrate(t: Long, seconds: Float)
    /** 0 uncalibrated, 1..100 calibrating %, 200 ready. */
    external fun trackerCalibrationState(t: Long): Int
    external fun trackerCalibrationJson(t: Long): String?

    // --- space -----------------------------------------------------------
    external fun spaceNew(widthPx: Int, heightPx: Int, fovHRad: Float, tiltRad: Float): Long
    external fun spaceFree(s: Long)
    external fun spaceSetPose(s: Long, pose8: FloatArray): Int
    external fun spaceUpsert(
        s: Long, index: Int, sourceKind: Int, srcW: Int, srcH: Int,
        yaw: Float, pitch: Float, distance: Float, widthM: Float, heightM: Float, anchor: Int,
    ): Int
    external fun spacePinHere(s: Long, index: Int): Int
    external fun spaceCount(s: Long): Int
    /** out8 = TL,TR,BR,BL (x,y) px; h9 row-major homography. 1 on-screen, 0 off, -1 behind. */
    external fun spaceProject(s: Long, index: Int, dt: Float, out8: FloatArray, h9: FloatArray?): Int

    // --- glasses ---------------------------------------------------------
    external fun glassesOpenFd(fd: Int): Long
    external fun glassesOpenMock(): Long
    external fun glassesFree(g: Long)
    /** 0 imu (t2 = lo,hi µs; out9 = accel,gyro,mag), 1 button (t2[0]), -1 disconnected. */
    external fun glassesNext(g: Long, t2: IntArray, out9: FloatArray): Int
    /** 0 mono, 1 stereo, 2 mono120, 3 stereo120 */
    external fun glassesSetMode(g: Long, mode: Int): Int

    const val SOURCE_PHONE = 0
    const val SOURCE_DEX = 1
    const val SOURCE_VIRTUAL = 2
    const val ANCHOR_WORLD = 0
    const val ANCHOR_HEAD = 1
    const val ANCHOR_SMOOTH = 2
}
