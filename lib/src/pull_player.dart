import 'dart:async';
import 'dart:io';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:path_provider/path_provider.dart';

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
  /// Разрывы цикла: паузы между рендерами дольше 150мс (Dart-джанк,
  /// GC, подвисший канал) — главный кандидат в «скачки ритма».
  /// Частичные feed'ы (буфер трека полон) — противодавление в норме.
  int _gaps = 0;
  int _partials = 0;
  DateTime? _lastRenderEnd;

  /// Бортовой самописец: последние строки для экрана диагностики.
  /// Кап 60 — память не течет на длинных сессиях.
  final List<String> diagLog = [];
  static const int _diagCap = 60;

  /// Захват потока в файл для экспертизы: пишет сырые f32-блоки
  /// (потом склею в WAV на десктопе). null — не пишем.
  RandomAccessFile? _captureFile;
  int _captureLeft = 0;
  static const int _captureChunks = 300; // 30 секунд по 0.1с.

  /// Начать захват следующих 30 секунд. Путь файла — в логе.
  /// Работает только во время игры.
  Future<String?> startCapture() async {
    if (!_running || _captureFile != null) return null;
    try {
      final dir = await getTemporaryDirectory();
      final path =
          '${dir.path}/cont_capture_${DateTime.now().millisecondsSinceEpoch}.f32';
      _captureFile = await File(path).open(mode: FileMode.write);
      _captureLeft = _captureChunks;
      _log('capture -> $path');
      return path;
    } catch (e) {
      _log('capture failed: $e');
      return null;
    }
  }

  Future<void> _captureWrite(Float32List pcm) async {
    final f = _captureFile;
    if (f == null) return;
    try {
      await f.writeFrom(pcm.buffer.asUint8List());
      _captureLeft--;
      if (_captureLeft <= 0) {
        await f.close();
        _captureFile = null;
        _log('capture done (30s)');
      }
    } catch (e) {
      _log('capture write failed: $e');
      try {
        await f.close();
      } catch (_) {}
      _captureFile = null;
    }
  }

  void _log(String s) {
    final t = DateTime.now();
    final hh = t.hour.toString().padLeft(2, '0');
    final mm = t.minute.toString().padLeft(2, '0');
    final ss = t.second.toString().padLeft(2, '0');
    final line = '[$hh:$mm:$ss] $s';
    debugPrint('[pull] $s');
    diagLog.add(line);
    if (diagLog.length > _diagCap) {
      diagLog.removeRange(0, diagLog.length - _diagCap);
    }
  }

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
      _gaps = 0;
      _partials = 0;
      _lastRenderEnd = null;
      diagLog.clear();
      _log('start: track prepared, loop running');
      await _channel.invokeMethod<void>('playTrack');
      unawaited(_loop(myId));
      return true;
    } catch (e) {
      _log('start failed: $e');
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
          final now = DateTime.now();
          final prev = _lastRenderEnd;
          if (prev != null) {
            final gapMs = now.difference(prev).inMilliseconds;
            if (gapMs > 150) {
              _gaps++;
              if (gapMs > 1000) {
                _log('loop stall ${gapMs}ms (dart jank?)');
              }
            }
          }
          _lastRenderEnd = now;
          sw.reset();
          sw.start();
          pcm = await playbackPullRender(
            frames: BigInt.from(chunkFrames),
            sampleRate: sampleRate,
          );
          sw.stop();
          _chunks++;
          _renderMsTotal += sw.elapsedMilliseconds;
          await _captureWrite(pcm);
          if (sw.elapsedMilliseconds > 50) {
            _slowChunks++;
            _log('SLOW render #$_chunks: ${sw.elapsedMilliseconds}ms '
                '(budget 100ms, slow=$_slowChunks)');
          } else if (_chunks % 100 == 0) {
            // Глубина очереди из бэкенда: растет — насос отстает
            // от композера (ноты опаздывают и дропаются).
            var queued = -1;
            try {
              queued = (await playbackEngineStats()).queued.toInt();
            } catch (_) {}
            _log('render avg ${_renderMsTotal ~/ _chunks}ms '
                '(slow=$_slowChunks/$_chunks gaps=$_gaps partials=$_partials queued=$queued)');
          }
        } catch (e) {
          _log('render failed: $e');
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
          if (feedErrors == 1 || feedErrors % 10 == 0) {
            _log('feed error #$feedErrors, retrying');
          }
          if (feedErrors >= 10) {
            // Трек мертв (маршрут умер без события): пересоздаем
            // и продолжаем с пустого буфера, а не крутим вхолостую.
            feedErrors = 0;
            pending = null;
            try {
              final ok = await _channel.invokeMethod<bool>('prepare');
              if (ok == true) {
                await _channel.invokeMethod<void>('playTrack');
                _log('track recreated after feed errors');
              }
            } catch (e) {
              _log('heal failed: $e');
            }
          }
          await Future<void>.delayed(const Duration(milliseconds: 200));
        } else if (written * 2 < pcm.length) {
          // Буфер почти полон: съедено частично — остаток следующим,
          // рендерить нечего, ждем место.
          feedErrors = 0;
          _partials++;
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
    final cap = _captureFile;
    _captureFile = null;
    if (cap != null) {
      try {
        await cap.close();
      } catch (_) {}
      _log('capture stopped early');
    }
    try {
      await _channel.invokeMethod<void>('pauseTrack');
    } catch (e) {
      debugPrint('[pull] stop failed: $e');
    }
  }
}
