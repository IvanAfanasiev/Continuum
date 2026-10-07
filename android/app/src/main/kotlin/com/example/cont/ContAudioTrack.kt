package com.example.cont

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack

/**
 * PCM-фидер: Rust только рендерит блоки (pull), играет AudioTrack.
 * Маршрут (наушники/динамик), фокус и микширование — штатно от ОС:
 * ломаться тут нечему, самолечение потока не нужно.
 * minSdk=24, поэтому только современные API без веток совместимости.
 */
class ContAudioTrack {
    companion object {
        const val SAMPLE_RATE = 48000
        const val CHANNEL_MASK = AudioFormat.CHANNEL_OUT_STEREO
        const val ENCODING = AudioFormat.ENCODING_PCM_FLOAT
        // Буфер ~1с: сглаживает джиттер рендера. Рендер впрок ограничен
        // им же (неблокирующая запись вернет 0, когда полон — см. feed).
        // ВАЖНО: запись только неблокирующая — блокирующая вешает
        // UI-поток Android на время ожидания места (системные лаги).
        const val BUFFER_FRAMES = SAMPLE_RATE
    }

    private var track: AudioTrack? = null
    private val lock = Any()

    /** Создать трек. false — устройство не дало поток. */
    fun prepare(): Boolean {
        synchronized(lock) {
            releaseLocked()
            val minBuf = AudioTrack.getMinBufferSize(SAMPLE_RATE, CHANNEL_MASK, ENCODING)
            if (minBuf <= 0) return false
            val bufBytes = maxOf(minBuf * 4, BUFFER_FRAMES * 2 * 4)
            track = AudioTrack.Builder()
                .setAudioAttributes(
                    AudioAttributes.Builder()
                        .setUsage(AudioAttributes.USAGE_MEDIA)
                        .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                        .build()
                )
                // Низкая задержка вместо offload/глубокого буфера:
                // файловый плеер идёт тем же трактом ровно, а наш стрим
                // без флага получал порционную выдачу (скачки при
                // идеальных метриках цикла).
                .setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY)
                .setAudioFormat(
                    AudioFormat.Builder()
                        .setEncoding(ENCODING)
                        .setSampleRate(SAMPLE_RATE)
                        .setChannelMask(CHANNEL_MASK)
                        .build()
                )
                .setBufferSizeInBytes(bufBytes)
                .setTransferMode(AudioTrack.MODE_STREAM)
                .build()
            return track?.state == AudioTrack.STATE_INITIALIZED
        }
    }

    fun play() {
        synchronized(lock) {
            val t = track ?: return
            if (t.playState != AudioTrack.PLAYSTATE_PLAYING) {
                runCatching { t.play() }
            }
        }
    }

    /**
     * Неблокирующая запись кадра: возвращает реально принятые фреймы
     * (0..N). Ноль/частичность — буфер полон: Dart подождет и повторит
     * остаток, а не будет спиновать. Никогда не виснет на UI-потоке.
     */
    fun feed(pcm: FloatArray): Int {
        val t = synchronized(lock) { track } ?: return -1
        return try {
            t.write(pcm, 0, pcm.size, AudioTrack.WRITE_NON_BLOCKING) / 2
        } catch (e: Exception) {
            -1
        }
    }

    fun pauseAndFlush() {
        synchronized(lock) {
            val t = track ?: return
            runCatching { t.pause() }
            runCatching { t.flush() }
        }
    }

    fun isPlaying(): Boolean = synchronized(lock) {
        track?.playState == AudioTrack.PLAYSTATE_PLAYING
    }

    /** Позиция иглы во фреймах: Dart держит ≤1 чанка впереди нее. */
    fun headPosition(): Int = synchronized(lock) {
        runCatching { track?.playbackHeadPosition ?: 0 }.getOrDefault(0)
    }

    fun release() {
        synchronized(lock) { releaseLocked() }
    }

    private fun releaseLocked() {
        val t = track
        track = null
        if (t == null) return
        runCatching { t.pause() }
        runCatching { t.flush() }
        runCatching { t.release() }
    }
}
