//! Android JNI bridge: hands the JavaVM to `ndk-context`.
//!
//! cpal (device enumeration via `AudioTrack.getMinBufferSize`) and Oboe
//! require an initialized Android context, otherwise panic
//! "android context was not initialized". The context arrives only through the
//! JVM loader: Kotlin `System.loadLibrary` in `Application.onCreate`
//! calls this `JNI_OnLoad` (dlopen from Dart-FFI does NOT call it).
//!
//! The whole file is Android-only: it is absent on desktop and in tests.

use std::ffi::c_void;
use std::os::raw::c_int;

/// JNI_VERSION_1_6. We don't pull in a separate jni crate for a single constant.
const JNI_VERSION_1_6: c_int = 0x0001_0006;

/// Called by the JVM on `System.loadLibrary`. `reserved` is null (not an Activity);
/// we only need the JavaVM: audio requests go through static methods,
/// the context object is not used.
#[no_mangle]
pub unsafe extern "system" fn JNI_OnLoad(vm: *mut c_void, _reserved: *mut c_void) -> c_int {
    ndk_context::initialize_android_context(vm, std::ptr::null_mut());
    JNI_VERSION_1_6
}
