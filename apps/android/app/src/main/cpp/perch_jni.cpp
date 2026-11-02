// JNI glue between io.perch.android.native.PerchNative and libperch_ffi.so.
// Deliberately thin: no logic here, just marshalling.
#include <jni.h>
#include <android/log.h>
#include <string>
#include <vector>
#include "perch_ffi.h"

#define LOGI(...) __android_log_print(ANDROID_LOG_INFO, "PerchJNI", __VA_ARGS__)

extern "C" {

// ---- tracker -------------------------------------------------------------
JNIEXPORT jlong JNICALL
Java_io_perch_android_native_PerchNative_trackerNew(JNIEnv *env, jobject, jlong delayUs, jstring calJson) {
    const char *c = calJson ? env->GetStringUTFChars(calJson, nullptr) : nullptr;
    auto *t = perch_tracker_new((uint64_t) delayUs, c);
    if (c) env->ReleaseStringUTFChars(calJson, c);
    return reinterpret_cast<jlong>(t);
}

JNIEXPORT void JNICALL
Java_io_perch_android_native_PerchNative_trackerFree(JNIEnv *, jobject, jlong t) {
    perch_tracker_free(reinterpret_cast<PerchTracker *>(t));
}

JNIEXPORT jint JNICALL
Java_io_perch_android_native_PerchNative_trackerFeed(JNIEnv *env, jobject, jlong t, jlong tUs,
                                                     jfloatArray imu9, jboolean hasMag, jfloatArray out8) {
    float imu[9];
    env->GetFloatArrayRegion(imu9, 0, 9, imu);
    float out[8];
    int r = perch_tracker_feed(reinterpret_cast<PerchTracker *>(t), (uint64_t) tUs, imu, imu + 3,
                               hasMag ? imu + 6 : nullptr, out);
    env->SetFloatArrayRegion(out8, 0, 8, out);
    return r;
}

JNIEXPORT jint JNICALL
Java_io_perch_android_native_PerchNative_trackerPredict(JNIEnv *env, jobject, jlong t, jfloat extraS, jfloatArray out8) {
    float out[8];
    int r = perch_tracker_predict(reinterpret_cast<PerchTracker *>(t), extraS, out);
    env->SetFloatArrayRegion(out8, 0, 8, out);
    return r;
}

JNIEXPORT void JNICALL
Java_io_perch_android_native_PerchNative_trackerRecenter(JNIEnv *, jobject, jlong t) {
    perch_tracker_recenter(reinterpret_cast<PerchTracker *>(t));
}

JNIEXPORT void JNICALL
Java_io_perch_android_native_PerchNative_trackerCalibrate(JNIEnv *, jobject, jlong t, jfloat seconds) {
    perch_tracker_calibrate(reinterpret_cast<PerchTracker *>(t), seconds);
}

JNIEXPORT jint JNICALL
Java_io_perch_android_native_PerchNative_trackerCalibrationState(JNIEnv *, jobject, jlong t) {
    return perch_tracker_calibration_state(reinterpret_cast<PerchTracker *>(t));
}

JNIEXPORT jstring JNICALL
Java_io_perch_android_native_PerchNative_trackerCalibrationJson(JNIEnv *env, jobject, jlong t) {
    std::vector<char> buf(1024);
    int n = perch_tracker_calibration_json(reinterpret_cast<PerchTracker *>(t), buf.data(), buf.size());
    if (n < 0) return nullptr;
    if ((size_t) n + 1 > buf.size()) {
        buf.resize(n + 1);
        perch_tracker_calibration_json(reinterpret_cast<PerchTracker *>(t), buf.data(), buf.size());
    }
    return env->NewStringUTF(buf.data());
}

// ---- space ---------------------------------------------------------------
JNIEXPORT jlong JNICALL
Java_io_perch_android_native_PerchNative_spaceNew(JNIEnv *, jobject, jint w, jint h, jfloat fov, jfloat tilt) {
    return reinterpret_cast<jlong>(perch_space_new((uint32_t) w, (uint32_t) h, fov, tilt));
}

JNIEXPORT void JNICALL
Java_io_perch_android_native_PerchNative_spaceFree(JNIEnv *, jobject, jlong s) {
    perch_space_free(reinterpret_cast<PerchSpace *>(s));
}

JNIEXPORT jint JNICALL
Java_io_perch_android_native_PerchNative_spaceSetPose(JNIEnv *env, jobject, jlong s, jfloatArray pose8) {
    float p[8];
    env->GetFloatArrayRegion(pose8, 0, 8, p);
    return perch_space_set_pose(reinterpret_cast<PerchSpace *>(s), p);
}

JNIEXPORT jint JNICALL
Java_io_perch_android_native_PerchNative_spaceUpsert(JNIEnv *, jobject, jlong s, jint index, jint kind, jint sw, jint sh,
                                                     jfloat yaw, jfloat pitch, jfloat dist, jfloat wm, jfloat hm, jint anchor) {
    return perch_space_upsert(reinterpret_cast<PerchSpace *>(s), index, kind, (uint32_t) sw, (uint32_t) sh,
                              yaw, pitch, dist, wm, hm, anchor);
}

JNIEXPORT jint JNICALL
Java_io_perch_android_native_PerchNative_spacePlacement(JNIEnv *env, jobject, jlong s, jint index, jfloatArray out5) {
    jfloat *o = env->GetFloatArrayElements(out5, nullptr);
    int r = perch_space_placement(reinterpret_cast<const PerchSpace *>(s), index, o);
    env->ReleaseFloatArrayElements(out5, o, 0);
    return r;
}

JNIEXPORT jint JNICALL
Java_io_perch_android_native_PerchNative_spacePinHere(JNIEnv *, jobject, jlong s, jint index) {
    return perch_space_pin_here(reinterpret_cast<PerchSpace *>(s), index);
}

JNIEXPORT jint JNICALL
Java_io_perch_android_native_PerchNative_spaceCount(JNIEnv *, jobject, jlong s) {
    return perch_space_count(reinterpret_cast<PerchSpace *>(s));
}

JNIEXPORT jint JNICALL
Java_io_perch_android_native_PerchNative_spaceProject(JNIEnv *env, jobject, jlong s, jint index, jfloat dt,
                                                      jfloatArray out8, jfloatArray h9) {
    float o[8], h[9];
    int r = perch_space_project(reinterpret_cast<PerchSpace *>(s), index, dt, o, h9 ? h : nullptr);
    env->SetFloatArrayRegion(out8, 0, 8, o);
    if (h9) env->SetFloatArrayRegion(h9, 0, 9, h);
    return r;
}

// ---- glasses -------------------------------------------------------------
JNIEXPORT jlong JNICALL
Java_io_perch_android_native_PerchNative_glassesOpenFd(JNIEnv *, jobject, jint fd) {
    auto *g = perch_glasses_open_fd((intptr_t) fd);
    LOGI("glassesOpenFd(%d) -> %p", fd, (void *) g);
    return reinterpret_cast<jlong>(g);
}

JNIEXPORT jstring JNICALL
Java_io_perch_android_native_PerchNative_lastError(JNIEnv *env, jobject) {
    std::vector<char> buf(512);
    int n = perch_last_error(buf.data(), buf.size());
    if (n <= 0) return nullptr;
    if ((size_t) n + 1 > buf.size()) { buf.resize(n + 1); perch_last_error(buf.data(), buf.size()); }
    return env->NewStringUTF(buf.data());
}

JNIEXPORT jlong JNICALL
Java_io_perch_android_native_PerchNative_glassesOpenMock(JNIEnv *, jobject) {
    return reinterpret_cast<jlong>(perch_glasses_open_mock());
}

JNIEXPORT void JNICALL
Java_io_perch_android_native_PerchNative_glassesFree(JNIEnv *, jobject, jlong g) {
    perch_glasses_free(reinterpret_cast<PerchGlasses *>(g));
}

JNIEXPORT jint JNICALL
Java_io_perch_android_native_PerchNative_glassesNext(JNIEnv *env, jobject, jlong g, jintArray t2, jfloatArray out9) {
    uint32_t t[2];
    float o[9];
    int r = perch_glasses_next(reinterpret_cast<PerchGlasses *>(g), t, o);
    env->SetIntArrayRegion(t2, 0, 2, reinterpret_cast<jint *>(t));
    env->SetFloatArrayRegion(out9, 0, 9, o);
    return r;
}

JNIEXPORT jint JNICALL
Java_io_perch_android_native_PerchNative_glassesSetMode(JNIEnv *, jobject, jlong g, jint mode) {
    return perch_glasses_set_mode(reinterpret_cast<PerchGlasses *>(g), mode);
}

} // extern "C"
