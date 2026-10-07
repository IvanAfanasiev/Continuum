import 'dart:async';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

import 'package:cont/src/rust/api/playback.dart';

/// Вывод на Android: Rust рендерит блоки (pull), играет AudioTrack.
/// Цикл живет в главном изоляте (там работает FRB) и шагает сам:
/// блокирующая запись в трек держит темп без таймеров.
/// Фон держит foreground-служба audio_service.
class PullPlayer {
  /// Частота трека (универсальна для AudioTrack, ресемплит сам).
  static const double sampleRate = 48000;

  /// Полсекунды звука за вызов: 24000 фреймов стерео (~192 КБ).
  static const int chunkFrames = 24000;

  static const _channel = MethodChannel('cont/audio');

  bool _running = false;

  bool get running => _running;

  /// Скормлено треку за этот старт (фреймы): вместе с иглой трека
  /// задает темп цикла (см. _loop). Сбрасывается на старте.
  int _fedFrames = 0;

  /// Замеры темпа рендера для диагностики: сколько мс занимает блок.
  /// Рендер медленнее realtime (блок 0.5с дольше 500мс) = заикания:
  /// трек голодает. Пишем в лог только медленные и сводку каждых 20.
  int _chunks = 0;
  int _slowChunks = 0;
  int _renderMsTotal = 0;

  /// Старт: подготовить трек и гнать цикл. false — трек не создался.
  Future<bool> start() async {
    if (_running) return true;
    try {
      final ok = await _channel.invokeMethod<bool>('prepare');
      if (ok != true) {
        debugPrint('[pull] track not prepared');
        return false;
      }
      _running = true;
      _fedFrames = 0;
      _chunks = 0;
      _slowChunks = 0;
      _renderMsTotal = 0;
      await _channel.invokeMethod<void>('playTrack');
      unawaited(_loop());
      return true;
    } catch (e) {
      debugPrint('[pull] start failed: $e');
      _running = false;
      return false;
    }
  }

  Future<void> _loop() async {
    final sw = Stopwatch();
    while (_running) {
      // Темп ровно realtime: держим не больше чанка впереди иглы.
      // Без этого цикл рендерит вперед на всю мощь CPU (старт —
      // пачкой, дальше впритык) — телефон висит целиком, а не только
      // приложение. Игла — часы трека, дрейфа нет. Ошибка иглы —
      // работаем как раньше (fail-open).
      try {
        for (;;) {
          if (!_running) break;
          final head = await _channel.invokeMethod<int>('head');
          if (head == null) break;
          // Игла 32-битная (оборот ~за 24 часа): отрицательное = старшие.
          final h = head < 0 ? head + 4294967296 : head;
          if (_fedFrames - h <= chunkFrames) break;
          await Future<void>.delayed(const Duration(milliseconds: 50));
        }
      } catch (_) {
        // Нет иглы — без pacing, как раньше.
      }
      if (!_running) break;
      late final Float32List pcm;
      try {
        sw.reset();
        sw.start();
        pcm = await playbackPullRender(
          frames: BigInt.from(chunkFrames),
          sampleRate: sampleRate,
        );
        sw.stop();
        _chunks++;
        _renderMsTotal += sw.elapsedMilliseconds;
        if (sw.elapsedMilliseconds > 250) {
          _slowChunks++;
          debugPrint('[pull] SLOW render #$_chunks: ${sw.elapsedMilliseconds}ms '
              '(budget 500ms, slow=$_slowChunks)');
        } else if (_chunks % 20 == 0) {
          debugPrint('[pull] render avg ${_renderMsTotal ~/ _chunks}ms '
              '(slow=$_slowChunks/$_chunks)');
        }
      } catch (e) {
        debugPrint('[pull] render failed: $e');
        await Future<void>.delayed(const Duration(milliseconds: 200));
        continue;
      }
      if (!_running) break;
      if (pcm.isEmpty) {
        await Future<void>.delayed(const Duration(milliseconds: 200));
        continue;
      }
      try {
        final written = await _channel.invokeMethod<int>('feed', {'pcm': pcm});
        if (written != null && written < 0) {
          debugPrint('[pull] feed error, retrying');
          await Future<void>.delayed(const Duration(milliseconds: 200));
        } else if (written != null) {
          _fedFrames += written;
        }
      } catch (e) {
        debugPrint('[pull] feed failed: $e');
        await Future<void>.delayed(const Duration(milliseconds: 200));
      }
    }
  }

  Future<void> stop() async {
    _running = false;
    try {
      await _channel.invokeMethod<void>('pauseTrack');
    } catch (e) {
      debugPrint('[pull] stop failed: $e');
    }
  }
}
