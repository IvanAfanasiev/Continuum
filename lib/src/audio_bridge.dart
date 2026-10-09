import 'dart:async';
import 'dart:io';

import 'package:audio_service/audio_service.dart';
import 'package:audio_session/audio_session.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:image/image.dart' as img;
import 'package:path_provider/path_provider.dart';

import 'package:cont/src/player_controller.dart';
import 'package:cont/src/rust/api/playback.dart';

/// Мост к системному плееру: фоновая служба, уведомление с паузой,
/// аудиофокус («главный звуковой поток»). Весь звук по-прежнему
/// синтезирует Rust — мост только управляет и показывает состояние.
///
/// На Android звук идет из нативного Rust-потока (свой AudioTrack
/// по JNI): Dart шлет только команды play/pause, в тракте его нет.
///
/// Если нативная служба недоступна (десктоп, ошибка init), мост
/// тихо деградирует до прямых вызовов контроллера — UI не замечает.
class AudioBridge {
  AudioBridge(this._controller);

  final PlayerController _controller;
  ContAudioHandler? _handler;
  StreamSubscription? _interruptionSub;
  StreamSubscription? _noisySub;

  bool get available => _handler != null;

  Future<void> init() async {
    try {
      debugPrint('[audio] AudioService.init...');
      final handler = ContAudioHandler(_controller);
      await AudioService.init(
        builder: () => handler,
        config: const AudioServiceConfig(
          androidNotificationChannelId: 'com.example.cont.channel.audio',
          androidNotificationChannelName: 'Continuum playback',
          // Пауза не снимает foreground: рестарт службы из фона на
          // Android 12+ кидал бы исключение без этого.
          androidStopForegroundOnPause: false,
        ),
      );
      _handler = handler;
      debugPrint('[audio] service ready');
    } catch (e) {
      debugPrint('[audio] service unavailable: $e');
    }
    try {
      final session = await AudioSession.instance;
      await session.configure(const AudioSessionConfiguration.music());
      _interruptionSub = session.interruptionEventStream.listen((event) {
        // Звонок/навигатор забрали фокус — встаем на паузу, как плеер.
        if (event.begin) {
          debugPrint('[audio] interruption begin -> pause');
          unawaited(_controller.setPausedExternal(true));
        }
      });
      session.becomingNoisyEventStream.listen((_) {
        debugPrint('[audio] becoming noisy -> pause');
        unawaited(pause());
      });
      // Дублирующий native-сигнал (трек душится сам, событие — для паузы
      // состояния и уведомления, если главный изолят жив).
      if (Platform.isAndroid) {
        _noisySub = const EventChannel('cont/audio_events')
            .receiveBroadcastStream()
            .listen((_) async {
          debugPrint('[pull] becoming noisy -> pause');
          await pause();
        }, onError: (Object e) {
          debugPrint('[pull] events error: $e');
        });
      }
    } catch (e) {
      debugPrint('[audio] session unavailable: $e');
    }
  }

  /// Play на Android: сессия бэкенда + нативный аудио-поток.
  /// Остальные платформы — как раньше, через контроллер напрямую.
  Future<bool> play() async {
    if (Platform.isAndroid) {
      await _controller.ensurePlaying();
      try {
        await playbackPullThreadStart(
          styleName: _controller.styleName,
          seed: _controller.currentSeed,
        );
      } catch (e) {
        debugPrint('[audio] thread start failed: $e');
        return false;
      }
      await sync();
      return true;
    }
    if (_handler != null) {
      await _handler!.play();
    } else {
      await _controller.togglePlay();
    }
    return true;
  }

  Future<void> pause() async {
    if (Platform.isAndroid) {
      await playbackPullThreadStop();
      await _controller.ensurePaused();
      await sync();
      return;
    }
    if (_handler != null) {
      await _handler!.pause();
    } else {
      await _controller.togglePlay();
    }
  }

  /// Нативный поток жив (для экрана диагностики).
  Future<bool> pullActive() => Platform.isAndroid
      ? playbackPullAlive()
      : Future.value(false);

  /// Журнал нативного потока для экрана диагностики.
  Future<List<String>> diagLog() => Platform.isAndroid
      ? playbackPullDiag()
      : Future.value(const <String>[]);

  /// Захват следующих 30 секунд потока в файл (экспертиза скачков).
  /// Путь — публичная папка приложения. null — не играет.
  Future<String?> startCapture() async {
    if (!Platform.isAndroid) return null;
    try {
      final ext = await getExternalStorageDirectory();
      final dir = ext ?? await getTemporaryDirectory();
      final path =
          '${dir.path}/cont_capture_${DateTime.now().millisecondsSinceEpoch}.f32';
      return await playbackPullCapture(path: path) ? path : null;
    } catch (_) {
      return null;
    }
  }

  /// Прогрев кэша артов после загрузки списка стилей: первый тап
  /// play/стиль не будет тормозить на декоде картинки в UI-потоке.
  /// Fire-and-forget: ошибка здесь ни на что не влияет.
  Future<void> warmup(List<String> styles) async {
    for (final style in styles) {
      await styleArtUri(style);
    }
  }

  /// Обновить уведомление после действий вне кнопок play/pause
  /// (смена стиля, сида): дергается из UI рядом с вызовом контроллера.
  /// Замечание: на Android 13+ показ уведомления требует разрешения
  /// POST_NOTIFICATIONS — включается вручную в настройках приложения
  /// (звук и фон работают независимо от него).
  Future<void> sync() async {
    await _handler?.syncFromController();
  }

  Future<void> dispose() async {
    await _interruptionSub?.cancel();
    await _noisySub?.cancel();
  }
}

/// Хендлер системных кнопок (уведомление, гарнитура, Android Auto).
/// Делегирует в PlayerController, затем публикует состояние.
/// На Android в пару к состоянию стартует/останавливает нативный поток.
class ContAudioHandler extends BaseAudioHandler {
  ContAudioHandler(this.controller);

  final PlayerController controller;

  Future<void> _sync() async {
    final s = controller.status;
    final playing = s?.running == true && s?.paused == false;
    final style = s?.styleName ?? controller.currentStyle;
    final seed = s?.seedCode ?? '';
    mediaItem.add(MediaItem(
      id: style,
      album: 'Continuum',
      title: _title(style),
      artist: seed.isEmpty ? 'Continuum' : 'Сид $seed',
      artUri: await styleArtUri(style),
    ));
    playbackState.add(playbackState.value.copyWith(
      controls: [if (playing) MediaControl.pause else MediaControl.play],
      androidCompactActionIndices: const [0],
      processingState: AudioProcessingState.ready,
      playing: playing,
    ));
  }

  /// Вызывается мостом/UI после внешних изменений (стиль, сид).
  Future<void> syncFromController() async => _sync();

  @override
  Future<void> play() async {
    await controller.ensurePlaying();
    if (Platform.isAndroid) {
      try {
        await playbackPullThreadStart(
          styleName: controller.styleName,
          seed: controller.currentSeed,
        );
      } catch (e) {
        debugPrint('[audio] handler thread start failed: $e');
      }
    }
    await _sync();
  }

  @override
  Future<void> pause() async {
    if (Platform.isAndroid) {
      await playbackPullThreadStop();
    }
    await controller.ensurePaused();
    await _sync();
  }

  @override
  Future<void> stop() async {
    // У плеера stop = пауза + убрать уведомление.
    if (Platform.isAndroid) {
      await playbackPullThreadStop();
    }
    await controller.ensurePaused();
    playbackState.add(playbackState.value.copyWith(
      controls: const [MediaControl.play],
      processingState: AudioProcessingState.idle,
      playing: false,
    ));
  }

  String _title(String style) =>
      style.isEmpty ? 'Continuum' : style[0].toUpperCase() + style.substring(1);
}

/// Затемненная картинка стиля для уведомления (файл в кэше).
/// Общая для хендлера и прогрева: первый тап не тормозит на декоде.
Future<Uri?> styleArtUri(String style) async {
  try {
    final dir = await getTemporaryDirectory();
    final file = File('${dir.path}/cont_art_$style.png');
    if (await file.exists()) return Uri.file(file.path);
    final data = await rootBundle.load('assets/images/style_$style.png');
    final decoded = img.decodeImage(data.buffer.asUint8List());
    if (decoded == null) return null;
    for (final pixel in decoded) {
      pixel.r = (pixel.r * 0.55).toInt();
      pixel.g = (pixel.g * 0.55).toInt();
      pixel.b = (pixel.b * 0.55).toInt();
    }
    await file.writeAsBytes(img.encodePng(decoded));
    return Uri.file(file.path);
  } catch (e) {
    debugPrint('[audio] art unavailable: $e');
    return null;
  }
}
