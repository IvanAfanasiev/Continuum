package com.example.cont

import android.app.Application

/// Точка входа процесса: грузим нативную библиотеку через JVM-загрузчик
/// (а не dlopen из Dart-FFI) — только так вызывается наш JNI_OnLoad,
/// и Rust-сторона получает JavaVM для cpal/Oboe.
/// Без этого первое обращение к аудио падало с
/// "android context was not initialized".
class App : Application() {
    override fun onCreate() {
        super.onCreate()
        System.loadLibrary("rust_lib_cont")
    }
}
