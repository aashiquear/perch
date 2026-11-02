cmake_minimum_required(VERSION 3.22.1)
project(perch_jni LANGUAGES CXX)

# libperch_ffi.so is produced by `cargo ndk` into src/main/jniLibs/<abi>/ (see app/build.gradle.kts).
set(PERCH_FFI_LIB ${CMAKE_CURRENT_SOURCE_DIR}/../jniLibs/${ANDROID_ABI}/libperch_ffi.so)

add_library(perch_ffi SHARED IMPORTED)
# IMPORTED_NO_SONAME: link as -lperch_ffi so DT_NEEDED is the bare file name, not this path.
set_target_properties(perch_ffi PROPERTIES IMPORTED_LOCATION ${PERCH_FFI_LIB} IMPORTED_NO_SONAME TRUE)

add_library(perch_jni SHARED perch_jni.cpp)
target_include_directories(perch_jni PRIVATE ${CMAKE_CURRENT_SOURCE_DIR}/../../../../../../crates/perch-ffi/include)
target_link_libraries(perch_jni perch_ffi log)
