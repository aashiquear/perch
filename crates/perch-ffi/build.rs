//! Give the Android cdylib a SONAME. Without it, a JNI library linked against
//! the file records its absolute build path as DT_NEEDED and dlopen fails on
//! the device ("library /mnt/.../libperch_ffi.so not found").
fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("android") {
        println!("cargo:rustc-cdylib-link-arg=-Wl,-soname,libperch_ffi.so");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
