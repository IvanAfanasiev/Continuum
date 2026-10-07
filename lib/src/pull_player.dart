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

  /// Десятая секунды за вызов: 4800 фреймов стерео (~38 КБ).
  /// Блок обязан быть не длиннее шага сетки (16-я при 126 BPM = 119мс):
  /// насос разбирает очередь в начале блока, и все ноты блока
  /// стартуют разом. Полсекундные блоки давали залп каждые 0.5с —
  /// «несколько нот выстреливают одновременно».
  static const int chunkFrames = 4800;

  static const _channel = MethodChannel('cont/audio');

  bool _running = false;

  bool get running => _running;

  /// Поколение цикла: stop/start впритык не должны плодить два
  /// параллельных цикла (старый застрял в feed, новый уже гоняет).
  int _runId = 0;

  /// Замеры темпа рендера для диагностики: сколько мс занимает блок.
  /// Рендер медленнее realtime (блок 0.1с дольше 100мс) = заикания:
  /// трек голодает. Пишем в лог только медленные и сводку каждых 100.
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
      _runId++;
      final myId = _runId;
      _chunks = 0;
      _slowChunks = 0;
      _renderMsTotal = 0;
      await _channel.invokeMethod<void>('playTrack');
      unawaited(_loop(myId));
      return true;
    } catch (e) {
      debugPrint('[pull] start failed: $e');
      _running = false;
      return false;
    }
  }

  Future<void> _loop(int myId) async {
    final sw = Stopwatch();
    // Самолечение трека: серия ошибок подачи — пересоздаем трек,
    // а не крутим рендер вхолостую.
    var feedErrors = 0;
    // Несожранный остаток прошлого feed (буфер был полон): отдаем
    // первым, рендерим только когда трек все съел. Потерять хвост
    // нельзя — это заикание, а не джиттер.
    Float32List? pending;
    bool alive() => _running && myId == _runId;
    while (alive()) {
      // Темп задает противодавление трека: пока буфер полон, feed
      // возвращает частичность и цикл ждет — рендера впрок нет,
      // спина нет. Блокирующих вызовов нет вовсе: запись неблокирующая,
      // UI-поток Android не виснет ни на миллисекунду.
      Float32List pcm;
      final rest = pending;
      if (rest != null) {
        pcm = rest;
      } else {
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
          if (sw.elapsedMilliseconds > 50) {
            _slowChunks++;
            debugPrint('[pull] SLOW render #$_chunks: ${sw.elapsedMilliseconds}ms '
                '(budget 100ms, slow=$_slowChunks)');
          } else if (_chunks % 100 == 0) {
            debugPrint('[pull] render avg ${_renderMsTotal ~/ _chunks}ms '
                '(slow=$_slowChunks/$_chunks)');
          }
        } catch (e) {
          debugPrint('[pull] render failed: $e');
          await Future<void>.delayed(const Duration(milliseconds: 200));
          continue;
        }
      }
      if (!alive()) break;
      if (pcm.isEmpty) {
        pending = null;
        await Future<void>.delayed(const Duration(milliseconds: 200));
        continue;
      }
      try {
        final written = await _channel.invokeMethod<int>('feed', {'pcm': pcm});
        if (written == null || written < 0) {
          feedErrors++;
          debugPrint('[pull] feed error #$feedErrors, retrying');
          if (feedErrors >= 10) {
            // Трек мертв (маршрут умер без события): пересоздаем
            // и продолжаем с пустого буфера, а не крутим вхолостую.
            feedErrors = 0;
            pending = null;
            try {
              final ok = await _channel.invokeMethod<bool>('prepare');
              if (ok == true) {
                await _channel.invokeMethod<void>('playTrack');
              }
            } catch (e) {
              debugPrint('[pull] heal failed: $e');
            }
          }
          await Future<void>.delayed(const Duration(milliseconds: 200));
        } else if (written * 2 < pcm.length) {
          // Буфер почти полон: съедено частично — остаток следующим,
          // рендерить нечего, ждем место.
          feedErrors = 0;
          pending = Float32List.sublistView(pcm, written * 2);
          await Future<void>.delayed(const Duration(milliseconds: 30));
        } else {
          feedErrors = 0;
          pending = null;
        }
      } catch (e) {
        debugPrint('[pull] feed failed: $e');
        await Future<void>.delayed(const Duration(milliseconds: 200));
      }
    }
  }

  Future<void> stop() async {
    _running = false;
    _runId++;
    try {
      await _channel.invokeMethod<void>('pauseTrack');
    } catch (e) {
      debugPrint('[pull] stop failed: $e');
    }
  }
}
