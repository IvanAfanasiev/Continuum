package com.continuum.app

import android.Manifest
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel

private const val NotificationPermissionRequest = 7001

class MainActivity : FlutterActivity() {
    private var audioChannel: MethodChannel? = null
    private val audioPlayer by lazy {
        ContinuumAudioPlayer(this) { event ->
            runOnUiThread {
                audioChannel?.invokeMethod("playbackEvent", event)
            }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        requestNotificationPermissionIfNeeded()
        handleMediaAction(intent)
    }

    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)

        audioChannel = MethodChannel(flutterEngine.dartExecutor.binaryMessenger, "continuum/audio")
        audioChannel?.setMethodCallHandler { call, result ->
            when (call.method) {
                "play" -> {
                    runAudioCommand(result) {
                        val presetId = call.argument<Int>("presetId") ?: 0
                        audioPlayer.play(presetId)
                        null
                    }
                }

                "pause" -> {
                    runAudioCommand(result) {
                        audioPlayer.pause()
                        null
                    }
                }

                "selectPreset" -> {
                    runAudioCommand(result) {
                        val presetId = call.argument<Int>("presetId") ?: 0
                        audioPlayer.selectPreset(presetId)
                        null
                    }
                }

                "state" -> {
                    runAudioCommand(result) {
                        audioPlayer.currentState()
                    }
                }

                "controls" -> {
                    runAudioCommand(result) {
                        audioPlayer.updateControls(
                            tempo = call.argument<Double>("tempo"),
                            swing = call.argument<Double>("swing"),
                            volumes = call.argument<Map<*, *>>("volumes"),
                        )
                        null
                    }
                }

                else -> result.notImplemented()
            }
        }
        audioPlayer.emitCurrentState()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handleMediaAction(intent)
    }

    override fun onDestroy() {
        ContinuumAudioSession.execute {
            audioPlayer.release()
        }
        super.onDestroy()
    }

    private fun runAudioCommand(result: MethodChannel.Result, command: () -> Any?) {
        ContinuumAudioSession.execute {
            try {
                val value = command()
                runOnUiThread {
                    result.success(value)
                }
            } catch (error: Throwable) {
                runOnUiThread {
                    result.error(
                        "continuum_audio",
                        "${error.javaClass.simpleName}: ${error.message}",
                        null,
                    )
                }
            }
        }
    }

    private fun handleMediaAction(intent: Intent?) {
        ContinuumAudioSession.execute {
            audioPlayer.handleMediaAction(intent?.action)
        }
    }

    private fun requestNotificationPermissionIfNeeded() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) {
            return
        }

        if (checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) ==
            PackageManager.PERMISSION_GRANTED
        ) {
            return
        }

        requestPermissions(
            arrayOf(Manifest.permission.POST_NOTIFICATIONS),
            NotificationPermissionRequest,
        )
    }
}
