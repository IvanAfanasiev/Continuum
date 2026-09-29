package com.example.cont

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.media.AudioManager
import com.ryanheise.audioservice.AudioServiceActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.EventChannel
import io.flutter.plugin.common.MethodChannel

/**
 * Точка входа: audio_service требует наследника AudioServiceActivity,
 * сюда же вешаем наш PCM-канал (Rust рендерит, играет AudioTrack).
 */
class MainActivity : AudioServiceActivity() {
    private val track = ContAudioTrack()
    private var events: EventChannel.EventSink? = null

    private val noisyReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (intent.action == AudioManager.ACTION_AUDIO_BECOMING_NOISY) {
                // Наушники выдернули: душим трек сразу, Dart довершит
                // паузу (состояние, уведомление) через событие.
                track.pauseAndFlush()
                events?.success("noisy")
            }
        }
    }

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)

        MethodChannel(
            flutterEngine.dartExecutor.binaryMessenger, "cont/audio"
        ).setMethodCallHandler { call, result ->
            try {
                when (call.method) {
                    "prepare" -> result.success(track.prepare())
                    "playTrack" -> {
                        track.play()
                        result.success(null)
                    }
                    "feed" -> {
                        val pcm = call.argument<FloatArray>("pcm")
                        if (pcm == null) {
                            result.error("cont_audio", "missing pcm", null)
                        } else {
                            result.success(track.feed(pcm))
                        }
                    }
                    "pauseTrack" -> {
                        track.pauseAndFlush()
                        result.success(null)
                    }
                    "isPlaying" -> result.success(track.isPlaying())
                    "release" -> {
                        track.release()
                        result.success(null)
                    }
                    else -> result.notImplemented()
                }
            } catch (e: Exception) {
                result.error("cont_audio", "${e.javaClass.simpleName}: ${e.message}", null)
            }
        }

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
        track.release()
        super.onDestroy()
    }
}
