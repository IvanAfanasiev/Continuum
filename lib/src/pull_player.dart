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
    while (_running) {
      late final Float32List pcm;
      try {
        pcm = await playbackPullRender(
          frames: BigInt.from(chunkFrames),
          sampleRate: sampleRate,
        );
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
