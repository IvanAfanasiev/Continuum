package com.continuum.app

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.media.AudioManager
import com.ryanheise.audioservice.AudioServiceActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.EventChannel

/**
 * Entry point: audio_service requires an AudioServiceActivity descendant.
 * Sound comes from the native Rust thread (its own AudioTrack via JNI);
 * this class only handles route events (headphones unplugged -> pause).
 */
class MainActivity : AudioServiceActivity() {
    private var events: EventChannel.EventSink? = null

    private val noisyReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (intent.action == AudioManager.ACTION_AUDIO_BECOMING_NOISY) {
                // Headphones unplugged: Dart completes the pause (state,
                // notification, stopping the native stream) via this event.
                events?.success("noisy")
            }
        }
    }

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)

        EventChannel(
            flutterEngine.dartExecutor.binaryMessenger, "continuum/audio_events"
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
