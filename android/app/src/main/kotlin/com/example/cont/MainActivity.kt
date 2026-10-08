package com.example.cont

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.media.AudioManager
import com.ryanheise.audioservice.AudioServiceActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.EventChannel

/**
 * Точка входа: audio_service требует наследника AudioServiceActivity.
 * Звук идет из нативного Rust-потока (свой AudioTrack по JNI),
 * здесь только события маршрута (наушники выдернули -> пауза).
 */
class MainActivity : AudioServiceActivity() {
    private var events: EventChannel.EventSink? = null

    private val noisyReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (intent.action == AudioManager.ACTION_AUDIO_BECOMING_NOISY) {
                // Наушники выдернули: Dart довершит паузу (состояние,
                // уведомление, останов нативного потока) через событие.
                events?.success("noisy")
            }
        }
    }

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)

        EventChannel(
            flutterEngine.dartExecutor.binaryMessenger, "cont/audio_events"
        ).setStreamHandler(object : EventChannel.StreamHandler {
            override fun onListen(args: Any?, sink: EventChannel.EventSink) {
                events = sink
            }

            override fun onCancel(args: Any?) {
                events = null
            }
        })

        registerReceiver(
            noisyReceiver,
            IntentFilter(AudioManager.ACTION_AUDIO_BECOMING_NOISY)
        )
    }

    override fun onDestroy() {
        runCatching { unregisterReceiver(noisyReceiver) }
        super.onDestroy()
    }
}
