package com.continuum.app

import android.app.Application

/// Process entry point: load the native library through the JVM loader
/// (not via dlopen from Dart FFI). This is the only way to invoke JNI_OnLoad
/// and give the Rust side a JavaVM for cpal/Oboe.
/// Without it the first audio access failed with
/// "android context was not initialized".
class App : Application() {
    override fun onCreate() {
        super.onCreate()
        System.loadLibrary("rust_lib_continuum")
    }
}
