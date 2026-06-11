package com.continuum.app

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.graphics.Canvas
import android.graphics.Color
import android.graphics.Paint
import android.graphics.Path
import android.media.AudioAttributes
import android.media.AudioFocusRequest
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioTrack
import android.media.MediaMetadata
import android.media.session.MediaSession
import android.media.session.PlaybackState
import android.os.Build
import android.os.SystemClock
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.concurrent.thread
import kotlin.math.abs
import kotlin.math.max

private const val SampleRate = 48_000
private const val ChannelCount = 2
private const val FramesPerBuffer = 512
private const val BytesPerFloat = 4
private const val MobileOutputGain = 6.0f
private const val DuckingGain = 0.22f
private const val PlaybackNotificationId = 4201
private const val PlaybackChannelId = "continuum_playback"
private const val PlaybackChannelName = "Continuum playback"
private const val PresetCount = 2
private const val NotificationTickMs = 1000L

internal const val ContinuumActionPrevious = "com.continuum.app.action.PREVIOUS"
internal const val ContinuumActionPlay = "com.continuum.app.action.PLAY"
internal const val ContinuumActionPause = "com.continuum.app.action.PAUSE"
internal const val ContinuumActionNext = "com.continuum.app.action.NEXT"

class ContinuumAudioPlayer(
    context: Context,
    private val eventSink: (Map<String, Any>) -> Unit = {},
) {
    private val lock = Any()
    private val running = AtomicBoolean(false)
    private val renderLoopRunning = AtomicBoolean(false)
    private var renderThread: Thread? = null
    private var runtimeHandle = 0L
    private var nativeBridge: ContinuumNative? = null
    private var focusRequest: AudioFocusRequest? = null
    private var ducked = false
    private var currentPresetId = 0
    private var accumulatedPlayMs = 0L
    private var playStartElapsedMs = 0L
    private val notificationTickerRunning = AtomicBoolean(false)
    private var expectedStopHandle = 0L
    private var notificationTickerThread: Thread? = null
    private var notificationVisible = false
    private var tempo = 1.0
    private var swing = 0.5
    private val volumes = mutableMapOf<String, Double>()
    private val artworkCache = mutableMapOf<Int, Bitmap>()

    private val appContext = context.applicationContext
    private val audioAttributes = AudioAttributes.Builder()
        .setUsage(AudioAttributes.USAGE_MEDIA)
        .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
        .build()
    private val audioManager =
        appContext.getSystemService(Context.AUDIO_SERVICE) as AudioManager
    private val notificationManager =
        appContext.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
    private val mediaSession = MediaSession(appContext, "Continuum").apply {
        setMetadata(
            MediaMetadata.Builder()
                .putString(MediaMetadata.METADATA_KEY_TITLE, "Continuum")
                .putString(MediaMetadata.METADATA_KEY_ARTIST, "Continuum")
                .build(),
        )
        setCallback(
            object : MediaSession.Callback() {
                override fun onPlay() {
                    if (!running.get()) {
                        this@ContinuumAudioPlayer.playLast()
                    }
                }

                override fun onPause() {
                    this@ContinuumAudioPlayer.pause()
                }

                override fun onStop() {
                    this@ContinuumAudioPlayer.pause()
                }

                override fun onSkipToPrevious() {
                    this@ContinuumAudioPlayer.previousPreset()
                }

                override fun onSkipToNext() {
                    this@ContinuumAudioPlayer.nextPreset()
                }
            },
        )
    }
    private val focusChangeListener = AudioManager.OnAudioFocusChangeListener { change ->
        when (change) {
            AudioManager.AUDIOFOCUS_GAIN -> synchronized(lock) {
                ducked = false
            }

            AudioManager.AUDIOFOCUS_LOSS_TRANSIENT_CAN_DUCK -> synchronized(lock) {
                ducked = true
            }

            AudioManager.AUDIOFOCUS_LOSS,
            AudioManager.AUDIOFOCUS_LOSS_TRANSIENT -> pause()
        }
    }

    init {
        ContinuumAudioSession.player = this
        createPlaybackNotificationChannel()
    }

    fun play(presetId: Int) {
        val nextPresetId = presetId.floorPreset()
        if (renderThread != null && runtimeHandle != 0L && currentPresetId == nextPresetId) {
            resumePlayback()
            return
        }

        if (running.get() || renderThread != null || runtimeHandle != 0L || notificationVisible) {
            stopPlayback(showPausedNotification = true, emitEvent = false)
        }
        currentPresetId = nextPresetId

        if (!requestMusicFocus()) {
            throw IllegalStateException("Audio focus was not granted")
        }

        val native = native()
        val handle = native.createPreset(currentPresetId, SampleRate.toFloat(), ChannelCount)
        if (handle == 0L) {
            abandonMusicFocus()
            throw IllegalStateException("Rust runtime was not created")
        }

        val audioTrack =
            try {
                createAudioTrack()
            } catch (error: Throwable) {
                native.destroy(handle)
                abandonMusicFocus()
                throw error
            }

        if (audioTrack.state != AudioTrack.STATE_INITIALIZED) {
            audioTrack.release()
            native.destroy(handle)
            abandonMusicFocus()
            throw IllegalStateException("AudioTrack was not initialized")
        }

        synchronized(lock) {
            runtimeHandle = handle
            ducked = false
            native.setPaused(handle, false)
            applyControlsLocked(native, handle)
        }

        renderLoopRunning.set(true)
        running.set(true)
        playStartElapsedMs = SystemClock.elapsedRealtime()
        updateMediaMetadata(currentPresetId)
        updateMediaSession(isPlaying = true)
        showPlaybackNotification(isPlaying = true)
        startNotificationTicker()
        emitPlaybackEvent(isPlaying = true)
        renderThread = thread(name = "ContinuumAudio", isDaemon = true) {
            try {
                val buffer = FloatArray(FramesPerBuffer * ChannelCount)
                var audioTrackPlaying = false
                audioTrack.setVolume(1.0f)

                while (renderLoopRunning.get()) {
                    if (!running.get()) {
                        if (audioTrackPlaying) {
                            runCatching { audioTrack.pause() }
                            runCatching { audioTrack.flush() }
                            audioTrackPlaying = false
                        }
                        Thread.sleep(16)
                        continue
                    }

                    if (!audioTrackPlaying) {
                        audioTrack.play()
                        audioTrackPlaying = true
                    }

                    val frames = native.render(handle, buffer, FramesPerBuffer)
                    if (frames <= 0) {
                        buffer.fill(0.0f)
                    }
                    val framesToWrite = if (frames > 0) frames else FramesPerBuffer
                    val samplesToWrite = framesToWrite * ChannelCount
                    applyOutputGain(buffer, samplesToWrite)
                    audioTrack.write(
                        buffer,
                        0,
                        samplesToWrite,
                        AudioTrack.WRITE_BLOCKING,
                    )
                }
            } catch (_: Throwable) {
                running.set(false)
                renderLoopRunning.set(false)
            } finally {
                runCatching { audioTrack.pause() }
                runCatching { audioTrack.flush() }
                runCatching { audioTrack.release() }

                var wasExpected = false
                val wasCurrent = synchronized(lock) {
                    wasExpected = expectedStopHandle == handle
                    if (wasExpected) {
                        expectedStopHandle = 0L
                    }
                    if (runtimeHandle == handle) {
                        runtimeHandle = 0L
                        true
                    } else {
                        false
                    }
                }

                if (wasCurrent && !wasExpected) {
                    captureElapsedPlayback()
                    updateMediaSession(isPlaying = false)
                    stopNotificationTicker()
                    showPlaybackNotification(isPlaying = false)
                    abandonMusicFocus()
                    emitPlaybackEvent(isPlaying = false)
                }
                native.destroy(handle)
            }
        }
    }

    private fun resumePlayback() {
        if (running.get()) {
            emitPlaybackEvent(isPlaying = true)
            return
        }

        if (!requestMusicFocus()) {
            throw IllegalStateException("Audio focus was not granted")
        }

        synchronized(lock) {
            ducked = false
            val handle = runtimeHandle
            val native = nativeBridge
            if (handle != 0L && native != null) {
                native.setPaused(handle, false)
                applyControlsLocked(native, handle)
            }
        }

        running.set(true)
        if (playStartElapsedMs == 0L) {
            playStartElapsedMs = SystemClock.elapsedRealtime()
        }
        updateMediaMetadata(currentPresetId)
        updateMediaSession(isPlaying = true)
        showPlaybackNotification(isPlaying = true)
        startNotificationTicker()
        emitPlaybackEvent(isPlaying = true)
    }

    fun selectPreset(presetId: Int) {
        val nextPresetId = presetId.floorPreset()
        if (nextPresetId == currentPresetId) {
            updateMediaMetadata(currentPresetId)
            updateMediaSession(isPlaying = running.get())
            if (notificationVisible) {
                showPlaybackNotification(isPlaying = running.get())
            }
            emitPlaybackEvent(isPlaying = running.get())
            return
        }

        if (running.get()) {
            play(nextPresetId)
            return
        }

        if (renderThread != null || runtimeHandle != 0L) {
            stopPlayback(showPausedNotification = notificationVisible, emitEvent = false)
        }

        currentPresetId = nextPresetId
        updateMediaMetadata(currentPresetId)
        updateMediaSession(isPlaying = false)
        if (notificationVisible) {
            showPlaybackNotification(isPlaying = false)
        }
        emitPlaybackEvent(isPlaying = false)
    }

    fun playLast() {
        play(currentPresetId)
    }

    fun previousPreset() {
        switchPreset(-1)
    }

    fun nextPreset() {
        switchPreset(1)
    }

    fun handleMediaAction(action: String?) {
        when (action) {
            ContinuumActionPrevious -> previousPreset()
            ContinuumActionPlay -> playLast()
            ContinuumActionPause -> pause()
            ContinuumActionNext -> nextPreset()
        }
    }

    fun emitCurrentState() {
        emitPlaybackEvent(isPlaying = running.get())
    }

    fun currentState(): Map<String, Any> {
        return playbackEvent(isPlaying = running.get())
    }

    private fun switchPreset(direction: Int) {
        val next = Math.floorMod(currentPresetId + direction, PresetCount)
        selectPreset(next)
    }

    fun pause() {
        if (!running.get()) {
            updateMediaSession(isPlaying = false)
            if (notificationVisible) {
                showPlaybackNotification(isPlaying = false)
            }
            emitPlaybackEvent(isPlaying = false)
            return
        }

        synchronized(lock) {
            val handle = runtimeHandle
            val native = nativeBridge
            if (handle != 0L && native != null) {
                native.setPaused(handle, true)
            }
        }

        running.set(false)
        captureElapsedPlayback()
        updateMediaSession(isPlaying = false)
        stopNotificationTicker()
        showPlaybackNotification(isPlaying = false)
        abandonMusicFocus()
        emitPlaybackEvent(isPlaying = false)
    }

    private fun stopPlayback(showPausedNotification: Boolean, emitEvent: Boolean) {
        if (renderThread != null) {
            synchronized(lock) {
                expectedStopHandle = runtimeHandle
            }
        }
        running.set(false)
        renderLoopRunning.set(false)
        val thread = renderThread
        if (thread != null && thread != Thread.currentThread()) {
            thread.join(900)
        }
        renderThread = null
        captureElapsedPlayback()
        updateMediaSession(isPlaying = false)
        stopNotificationTicker()
        if (showPausedNotification) {
            showPlaybackNotification(isPlaying = false)
        } else {
            hidePlaybackNotification()
        }
        abandonMusicFocus()
        if (emitEvent) {
            emitPlaybackEvent(isPlaying = false)
        }
    }

    fun release() {
        stopPlayback(showPausedNotification = false, emitEvent = true)
        hidePlaybackNotification()
        mediaSession.release()
        if (ContinuumAudioSession.player === this) {
            ContinuumAudioSession.player = null
        }
    }

    private fun captureElapsedPlayback() {
        if (playStartElapsedMs == 0L) {
            return
        }

        accumulatedPlayMs += (SystemClock.elapsedRealtime() - playStartElapsedMs).coerceAtLeast(0L)
        playStartElapsedMs = 0L
    }

    private fun elapsedPlaybackMs(): Long {
        val activePart = if (playStartElapsedMs == 0L) {
            0L
        } else {
            (SystemClock.elapsedRealtime() - playStartElapsedMs).coerceAtLeast(0L)
        }
        return accumulatedPlayMs + activePart
    }

    private fun emitPlaybackEvent(isPlaying: Boolean) {
        eventSink(playbackEvent(isPlaying))
    }

    private fun playbackEvent(isPlaying: Boolean): Map<String, Any> {
        return mapOf(
            "isPlaying" to isPlaying,
            "presetId" to currentPresetId,
            "presetName" to presetName(currentPresetId),
            "elapsedMs" to elapsedPlaybackMs(),
        )
    }

    fun updateControls(
        tempo: Double?,
        swing: Double?,
        volumes: Map<*, *>?,
    ) {
        synchronized(lock) {
            if (tempo != null) {
                this.tempo = tempo
            }
            if (swing != null) {
                this.swing = swing
            }
            if (volumes != null) {
                for ((name, value) in volumes) {
                    val key = name as? String ?: continue
                    val asNumber = value as? Number ?: continue
                    this.volumes[key] = asNumber.toDouble()
                }
            }

            val handle = runtimeHandle
            val native = nativeBridge
            if (handle != 0L && native != null) {
                applyControlsLocked(native, handle)
            }
        }
    }

    private fun native(): ContinuumNative {
        return nativeBridge ?: ContinuumNative().also { nativeBridge = it }
    }

    private fun applyControlsLocked(native: ContinuumNative, handle: Long) {
        native.setTempo(handle, tempo.toFloat())
        native.setSwing(handle, swing.toFloat())
        for ((name, value) in volumes) {
            val id = instrumentId(name) ?: continue
            native.setInstrumentVolume(handle, id, value.toFloat())
        }
    }

    private fun createAudioTrack(): AudioTrack {
        val minBufferBytes = AudioTrack.getMinBufferSize(
            SampleRate,
            AudioFormat.CHANNEL_OUT_STEREO,
            AudioFormat.ENCODING_PCM_FLOAT,
        )
        val bufferBytes = max(
            minBufferBytes,
            FramesPerBuffer * ChannelCount * BytesPerFloat * 4,
        )

        return AudioTrack.Builder()
            .setAudioAttributes(audioAttributes)
            .setAudioFormat(
                AudioFormat.Builder()
                    .setSampleRate(SampleRate)
                    .setChannelMask(AudioFormat.CHANNEL_OUT_STEREO)
                    .setEncoding(AudioFormat.ENCODING_PCM_FLOAT)
                    .build(),
            )
            .setTransferMode(AudioTrack.MODE_STREAM)
            .setBufferSizeInBytes(bufferBytes)
            .build()
    }

    private fun createPlaybackNotificationChannel() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
            return
        }

        val channel = NotificationChannel(
            PlaybackChannelId,
            PlaybackChannelName,
            NotificationManager.IMPORTANCE_LOW,
        ).apply {
            description = "Continuum music playback controls"
            setShowBadge(false)
        }
        notificationManager.createNotificationChannel(channel)
    }

    private fun updateMediaMetadata(presetId: Int) {
        val preset = presetName(presetId)
        val artwork = artworkForPreset(presetId)
        mediaSession.setMetadata(
            MediaMetadata.Builder()
                .putString(MediaMetadata.METADATA_KEY_TITLE, preset)
                .putString(MediaMetadata.METADATA_KEY_ARTIST, "Continuum")
                .putString(MediaMetadata.METADATA_KEY_ALBUM, "Generative music")
                .putBitmap(MediaMetadata.METADATA_KEY_ART, artwork)
                .putBitmap(MediaMetadata.METADATA_KEY_ALBUM_ART, artwork)
                .build(),
        )
    }

    private fun showPlaybackNotification(isPlaying: Boolean) {
        val preset = presetName(currentPresetId)
        val elapsedMs = elapsedPlaybackMs()
        val elapsedLabel = formatElapsed(elapsedMs)
        val notificationWhen = System.currentTimeMillis() - elapsedMs
        val builder = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            Notification.Builder(appContext, PlaybackChannelId)
        } else {
            @Suppress("DEPRECATION")
            Notification.Builder(appContext)
        }

        val previousAction = Notification.Action.Builder(
            android.R.drawable.ic_media_previous,
            "Previous",
            mediaActionIntent(ContinuumActionPrevious, 1),
        ).build()
        val playPauseAction = if (isPlaying) {
            Notification.Action.Builder(
                android.R.drawable.ic_media_pause,
                "Pause",
                mediaActionIntent(ContinuumActionPause, 2),
            ).build()
        } else {
            Notification.Action.Builder(
                android.R.drawable.ic_media_play,
                "Play",
                mediaActionIntent(ContinuumActionPlay, 3),
            ).build()
        }
        val nextAction = Notification.Action.Builder(
            android.R.drawable.ic_media_next,
            "Next",
            mediaActionIntent(ContinuumActionNext, 4),
        ).build()

        builder
            .setSmallIcon(R.drawable.ic_stat_continuum)
            .setLargeIcon(artworkForPreset(currentPresetId))
            .setContentTitle(preset)
            .setContentText(
                if (isPlaying) {
                    "Playing for $elapsedLabel"
                } else {
                    "Paused at $elapsedLabel"
                },
            )
            .setSubText("Continuum")
            .setContentInfo(elapsedLabel)
            .setCategory(Notification.CATEGORY_TRANSPORT)
            .setVisibility(Notification.VISIBILITY_PUBLIC)
            .setOngoing(isPlaying)
            .setWhen(notificationWhen)
            .setShowWhen(true)
            .setUsesChronometer(isPlaying)
            .setColor(Color.rgb(110, 214, 197))
            .setContentIntent(openAppIntent())
            .addAction(previousAction)
            .addAction(playPauseAction)
            .addAction(nextAction)
            .setStyle(
                Notification.MediaStyle()
                    .setMediaSession(mediaSession.sessionToken)
                    .setShowActionsInCompactView(0, 1, 2),
            )

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            builder.setColorized(true)
        }

        runCatching {
            notificationManager.notify(PlaybackNotificationId, builder.build())
            notificationVisible = true
        }
    }

    private fun hidePlaybackNotification() {
        notificationManager.cancel(PlaybackNotificationId)
        notificationVisible = false
    }

    private fun startNotificationTicker() {
        if (notificationTickerRunning.getAndSet(true)) {
            return
        }

        notificationTickerThread = thread(name = "ContinuumNotification", isDaemon = true) {
            while (notificationTickerRunning.get()) {
                Thread.sleep(NotificationTickMs)
                if (notificationTickerRunning.get() && running.get()) {
                    updateMediaSession(isPlaying = true)
                    showPlaybackNotification(isPlaying = true)
                }
            }
        }
    }

    private fun stopNotificationTicker() {
        notificationTickerRunning.set(false)
        val thread = notificationTickerThread
        if (thread != null && thread != Thread.currentThread()) {
            thread.join(250)
        }
        notificationTickerThread = null
    }

    private fun openAppIntent(): PendingIntent? {
        val intent = appContext.packageManager.getLaunchIntentForPackage(appContext.packageName)
            ?: return null
        intent.addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
        return PendingIntent.getActivity(
            appContext,
            0,
            intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
    }

    private fun mediaActionIntent(action: String, requestCode: Int): PendingIntent {
        val intent = Intent(appContext, ContinuumMediaActionReceiver::class.java).apply {
            this.action = action
        }
        return PendingIntent.getBroadcast(
            appContext,
            requestCode,
            intent,
            PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
    }

    private fun presetName(presetId: Int): String {
        return when (presetId) {
            1 -> "Jazz"
            else -> "Ambient"
        }
    }

    private fun Int.floorPreset(): Int {
        return Math.floorMod(this, PresetCount)
    }

    private fun artworkForPreset(presetId: Int): Bitmap {
        return artworkCache.getOrPut(presetId) {
            val assetPath = when (presetId) {
                1 -> "flutter_assets/assets/backgrounds/Jazz.png"
                else -> "flutter_assets/assets/backgrounds/Ambient.jfif"
            }
            runCatching {
                appContext.assets.open(assetPath).use { stream ->
                    BitmapFactory.decodeStream(stream)
                }
            }.getOrNull() ?: generatedArtwork(presetId)
        }
    }

    private fun generatedArtwork(presetId: Int): Bitmap {
        val bitmap = Bitmap.createBitmap(512, 512, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(bitmap)
        val paint = Paint(Paint.ANTI_ALIAS_FLAG)
        val accent = if (presetId == 1) {
            Color.rgb(255, 200, 107)
        } else {
            Color.rgb(125, 211, 252)
        }

        canvas.drawColor(Color.rgb(13, 15, 18))
        paint.color = Color.rgb(23, 25, 29)
        val diamond = Path().apply {
            moveTo(256f, 52f)
            lineTo(460f, 256f)
            lineTo(256f, 460f)
            lineTo(52f, 256f)
            close()
        }
        canvas.drawPath(diamond, paint)

        paint.color = accent
        val bars = listOf(
            floatArrayOf(150f, 294f, 176f, 380f),
            floatArrayOf(204f, 232f, 230f, 380f),
            floatArrayOf(258f, 270f, 284f, 380f),
            floatArrayOf(312f, 188f, 338f, 380f),
            floatArrayOf(366f, 246f, 392f, 380f),
        )
        for (bar in bars) {
            canvas.drawRoundRect(bar[0], bar[1], bar[2], bar[3], 10f, 10f, paint)
        }
        return bitmap
    }

    private fun formatElapsed(elapsedMs: Long): String {
        val totalSeconds = elapsedMs / 1000L
        val minutes = totalSeconds / 60L
        val seconds = totalSeconds % 60L
        return "%d:%02d".format(minutes, seconds)
    }

    private fun requestMusicFocus(): Boolean {
        val result = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val request = focusRequest ?: AudioFocusRequest.Builder(AudioManager.AUDIOFOCUS_GAIN)
                .setAudioAttributes(audioAttributes)
                .setOnAudioFocusChangeListener(focusChangeListener)
                .setWillPauseWhenDucked(false)
                .build()
                .also { focusRequest = it }
            audioManager.requestAudioFocus(request)
        } else {
            @Suppress("DEPRECATION")
            audioManager.requestAudioFocus(
                focusChangeListener,
                AudioManager.STREAM_MUSIC,
                AudioManager.AUDIOFOCUS_GAIN,
            )
        }

        return result == AudioManager.AUDIOFOCUS_REQUEST_GRANTED
    }

    private fun abandonMusicFocus() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            focusRequest?.let { audioManager.abandonAudioFocusRequest(it) }
        } else {
            @Suppress("DEPRECATION")
            audioManager.abandonAudioFocus(focusChangeListener)
        }

        synchronized(lock) {
            ducked = false
        }
    }

    private fun updateMediaSession(isPlaying: Boolean) {
        val state = if (isPlaying) {
            PlaybackState.STATE_PLAYING
        } else {
            PlaybackState.STATE_PAUSED
        }
        val speed = if (isPlaying) 1.0f else 0.0f
        val actions = PlaybackState.ACTION_PLAY_PAUSE or
            PlaybackState.ACTION_PLAY or
            PlaybackState.ACTION_PAUSE or
            PlaybackState.ACTION_STOP or
            PlaybackState.ACTION_SKIP_TO_PREVIOUS or
            PlaybackState.ACTION_SKIP_TO_NEXT

        mediaSession.setPlaybackState(
            PlaybackState.Builder()
                .setActions(actions)
                .setState(state, elapsedPlaybackMs(), speed)
                .build(),
        )
        mediaSession.isActive = isPlaying
    }

    private fun applyOutputGain(buffer: FloatArray, sampleCount: Int) {
        val focusGain = synchronized(lock) {
            if (ducked) DuckingGain else 1.0f
        }
        val gain = MobileOutputGain * focusGain
        for (index in 0 until sampleCount.coerceAtMost(buffer.size)) {
            buffer[index] = softLimit(buffer[index] * gain)
        }
    }

    private fun softLimit(sample: Float): Float {
        val limited = sample / (1.0f + abs(sample) * 0.55f)
        return limited.coerceIn(-0.98f, 0.98f)
    }

    private fun instrumentId(name: String): Int? {
        return when (name.lowercase()) {
            "piano" -> 0
            "pad" -> 1
            "bass" -> 2
            "triangle" -> 3
            "kick" -> 4
            "ride" -> 5
            "hihat" -> 6
            else -> null
        }
    }
}

class ContinuumNative {
    init {
        ContinuumNativeLoader.load()
    }

    external fun createPreset(presetId: Int, sampleRate: Float, channels: Int): Long
    external fun destroy(runtime: Long)
    external fun render(runtime: Long, output: FloatArray, frames: Int): Int
    external fun setPaused(runtime: Long, paused: Boolean)
    external fun setTempo(runtime: Long, value: Float)
    external fun setSwing(runtime: Long, value: Float)
    external fun setInstrumentVolume(runtime: Long, instrumentId: Int, value: Float): Boolean
}

internal object ContinuumAudioSession {
    var player: ContinuumAudioPlayer? = null
}

class ContinuumMediaActionReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent?) {
        ContinuumAudioSession.player?.handleMediaAction(intent?.action)
    }
}

private object ContinuumNativeLoader {
    private var loaded = false

    @Synchronized
    fun load() {
        if (loaded) {
            return
        }

        try {
            System.loadLibrary("continuum_jni")
            loaded = true
        } catch (error: UnsatisfiedLinkError) {
            throw IllegalStateException("Native load failed: ${error.message}", error)
        }
    }
}
