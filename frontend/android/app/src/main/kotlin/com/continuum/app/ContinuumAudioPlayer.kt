package com.continuum.app

import android.content.Context
import android.media.AudioFocusRequest
import android.media.AudioAttributes
import android.media.AudioManager
import android.media.AudioFormat
import android.media.AudioTrack
import android.media.MediaMetadata
import android.media.session.MediaSession
import android.media.session.PlaybackState
import android.os.Build
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.concurrent.thread
import kotlin.math.abs
import kotlin.math.max

private const val SampleRate = 48_000
private const val ChannelCount = 2
private const val FramesPerBuffer = 512
private const val BytesPerFloat = 4
private const val MobileOutputGain = 3.2f
private const val DuckingGain = 0.22f

class ContinuumAudioPlayer(context: Context) {
    private val lock = Any()
    private val running = AtomicBoolean(false)
    private var renderThread: Thread? = null
    private var runtimeHandle = 0L
    private var nativeBridge: ContinuumNative? = null
    private var focusRequest: AudioFocusRequest? = null
    private var ducked = false
    private var currentPresetId = 0
    private var tempo = 1.0
    private var swing = 0.5
    private val volumes = mutableMapOf<String, Double>()

    private val appContext = context.applicationContext
    private val audioAttributes = AudioAttributes.Builder()
        .setUsage(AudioAttributes.USAGE_MEDIA)
        .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
        .build()
    private val audioManager =
        appContext.getSystemService(Context.AUDIO_SERVICE) as AudioManager
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
                        this@ContinuumAudioPlayer.play(currentPresetId)
                    }
                }

                override fun onPause() {
                    this@ContinuumAudioPlayer.pause()
                }

                override fun onStop() {
                    this@ContinuumAudioPlayer.pause()
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

    fun play(presetId: Int) {
        pause()
        currentPresetId = presetId

        if (!requestMusicFocus()) {
            throw IllegalStateException("Audio focus was not granted")
        }

        val native = native()
        val handle = native.createPreset(presetId, SampleRate.toFloat(), ChannelCount)
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
            applyControlsLocked(native, handle)
        }

        running.set(true)
        updateMediaSession(isPlaying = true)
        renderThread = thread(name = "ContinuumAudio", isDaemon = true) {
            try {
                val buffer = FloatArray(FramesPerBuffer * ChannelCount)
                audioTrack.setVolume(1.0f)
                audioTrack.play()

                while (running.get()) {
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
            } finally {
                runCatching { audioTrack.pause() }
                runCatching { audioTrack.flush() }
                runCatching { audioTrack.release() }

                synchronized(lock) {
                    if (runtimeHandle == handle) {
                        runtimeHandle = 0L
                    }
                }
                updateMediaSession(isPlaying = false)
                abandonMusicFocus()
                native.destroy(handle)
            }
        }
    }

    fun pause() {
        running.set(false)
        val thread = renderThread
        if (thread != null && thread != Thread.currentThread()) {
            thread.join(900)
        }
        renderThread = null
        updateMediaSession(isPlaying = false)
        abandonMusicFocus()
    }

    fun release() {
        pause()
        mediaSession.release()
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
        val actions = PlaybackState.ACTION_PLAY_PAUSE or
            PlaybackState.ACTION_PAUSE or
            PlaybackState.ACTION_STOP

        mediaSession.setPlaybackState(
            PlaybackState.Builder()
                .setActions(actions)
                .setState(state, PlaybackState.PLAYBACK_POSITION_UNKNOWN, 1.0f)
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
        val limited = sample / (1.0f + abs(sample) * 0.48f)
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
    external fun setTempo(runtime: Long, value: Float)
    external fun setSwing(runtime: Long, value: Float)
    external fun setInstrumentVolume(runtime: Long, instrumentId: Int, value: Float): Boolean
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
