package com.continuum.app

import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel

class MainActivity : FlutterActivity() {
    private val audioPlayer by lazy { ContinuumAudioPlayer(this) }

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)

        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "continuum/audio")
            .setMethodCallHandler { call, result ->
                try {
                    when (call.method) {
                        "play" -> {
                            val presetId = call.argument<Int>("presetId") ?: 0
                            audioPlayer.play(presetId)
                            result.success(null)
                        }

                        "pause" -> {
                            audioPlayer.pause()
                            result.success(null)
                        }

                        "controls" -> {
                            audioPlayer.updateControls(
                                tempo = call.argument<Double>("tempo"),
                                swing = call.argument<Double>("swing"),
                                volumes = call.argument<Map<*, *>>("volumes"),
                            )
                            result.success(null)
                        }

                        else -> result.notImplemented()
                    }
                } catch (error: Throwable) {
                    result.error(
                        "continuum_audio",
                        "${error.javaClass.simpleName}: ${error.message}",
                        null,
                    )
                }
            }
    }

    override fun onDestroy() {
        audioPlayer.release()
        super.onDestroy()
    }
}
