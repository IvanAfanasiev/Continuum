//! Android JNI-мост: отдает JavaVM в `ndk-context`.
//!
//! cpal (перечисление устройств через `AudioTrack.getMinBufferSize`) и Oboe
//! требуют инициализированный android context, иначе паника
//! "android context was not initialized". Контекст приходит только через
//! JVM-загрузчик: Kotlin `System.loadLibrary` в `Application.onCreate`
//! вызывает этот `JNI_OnLoad` (dlopen из Dart-FFI его НЕ вызывает).
//!
//! Файл целиком только для Android: на десктопе и в тестах его нет.

use std::ffi::c_void;
use std::os::raw::c_int;

/// JNI_VERSION_1_6. Отдельный jni-крейт ради одной константы не тянем.
const JNI_VERSION_1_6: c_int = 0x0001_0006;

/// Вызывается JVM при `System.loadLibrary`. `reserved` — null (не Activity),
/// нам нужен только JavaVM: аудио-запросы идут статическими методами,
/// объект контекста не используется.
#[no_mangle]
pub unsafe extern "system" fn JNI_OnLoad(vm: *mut c_void, _reserved: *mut c_void) -> c_int {
    ndk_context::initialize_android_context(vm, std::ptr::null_mut());
    JNI_VERSION_1_6
}
