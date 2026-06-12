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

                        "selectPreset" -> {
                            val presetId = call.argument<Int>("presetId") ?: 0
                            audioPlayer.selectPreset(presetId)
                            result.success(null)
                        }

                        "state" -> {
                            result.success(audioPlayer.currentState())
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
        audioPlayer.emitCurrentState()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        handleMediaAction(intent)
    }

    override fun onDestroy() {
        audioPlayer.release()
        super.onDestroy()
    }

    private fun handleMediaAction(intent: Intent?) {
        audioPlayer.handleMediaAction(intent?.action)
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
