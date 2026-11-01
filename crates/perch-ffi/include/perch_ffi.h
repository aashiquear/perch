/* perch_ffi.h — C ABI of crates/perch-ffi. Hand-maintained to match lib.rs;
 * `scripts/gen-header.sh` regenerates it with cbindgen for verification. */
#ifndef PERCH_FFI_H
#define PERCH_FFI_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef struct PerchTracker PerchTracker;
typedef struct PerchSpace PerchSpace;
typedef struct PerchGlasses PerchGlasses;

/* ---- tracker ---------------------------------------------------------- */
PerchTracker *perch_tracker_new(uint64_t display_delay_us, const char *calibration_json);
void          perch_tracker_free(PerchTracker *t);
/* out8 = [qw,qx,qy,qz, wx,wy,wz, confidence]; mag may be NULL */
int  perch_tracker_feed(PerchTracker *t, uint64_t t_us, const float *accel3, const float *gyro3, const float *mag3, float *out8);
int  perch_tracker_predict(const PerchTracker *t, float extra_s, float *out8);
void perch_tracker_recenter(PerchTracker *t);
void perch_tracker_calibrate(PerchTracker *t, float seconds);
/* 0 uncalibrated, 1..100 calibrating %, 200 ready */
int  perch_tracker_calibration_state(const PerchTracker *t);
int  perch_tracker_calibration_json(const PerchTracker *t, char *buf, size_t cap);

/* ---- space ------------------------------------------------------------ */
PerchSpace *perch_space_new(uint32_t width_px, uint32_t height_px, float fov_h_rad, float tilt_rad);
void perch_space_free(PerchSpace *s);
int  perch_space_set_pose(PerchSpace *s, const float *pose8);
/* source_kind: 0 phone screen, 1 DeX, 2 virtual; anchor: 0 world, 1 head, 2 smooth-follow. Returns index. */
int  perch_space_upsert(PerchSpace *s, int index, int source_kind, uint32_t src_w, uint32_t src_h,
                        float yaw, float pitch, float distance, float width_m, float height_m, int anchor);
int  perch_space_pin_here(PerchSpace *s, int index);
int  perch_space_count(const PerchSpace *s);
/* out8 = TL,TR,BR,BL (x,y) in panel px; h9 = row-major 3x3 homography (may be NULL). 1 on screen, 0 off, -1 behind. */
int  perch_space_project(PerchSpace *s, int index, float dt, float *out8, float *h9);
int  perch_space_json(const PerchSpace *s, char *buf, size_t cap);
int  perch_space_load_json(PerchSpace *s, const char *json);

/* ---- glasses (Android USB fd) ----------------------------------------- */
PerchGlasses *perch_glasses_open_fd(intptr_t fd);
PerchGlasses *perch_glasses_open_mock(void);
void perch_glasses_free(PerchGlasses *g);
/* 0 = IMU sample (t2 = [lo,hi] µs, out9 = accel,gyro,mag), 1 = button (t2[0] = id), -1 = disconnected */
int  perch_glasses_next(PerchGlasses *g, uint32_t *t2, float *out9);
/* 0 mono, 1 stereo, 2 mono120, 3 stereo120 */
int  perch_glasses_set_mode(PerchGlasses *g, int mode);

#ifdef __cplusplus
}
#endif
#endif /* PERCH_FFI_H */
