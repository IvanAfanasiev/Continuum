import 'dart:io' show Platform;
import 'dart:math' as math;

import 'package:flutter/foundation.dart';

import 'package:cont/src/rust/api/playback.dart';
import 'package:cont/src/rust/playback.dart';

/// Состояние плеера: снимки из бэкенда + локальные выборы фронта.
/// Все управляющие вызовы идут в бэкенд и возвращают свежий статус —
/// UI всегда показывает факт, а не предположение.
class PlayerController extends ChangeNotifier {
  PlaybackStatus? status;
  List<TrackInfo> tracks = const [];
  List<GroupInfo> groups = const [];
  List<String> styles = const [];

  /// Стили с полками каталога (`club`/`rock`/`chill`).
  List<StyleMeta> stylesMeta = const [];

  /// Режим списка стилей: `group` (по полкам) или `alpha` (по алфавиту).
  String styleSortMode = 'group';

  /// Диагностика живого тракта (кадры/ноты/очередь) для баннера.
  EngineStats? stats;

  /// Кадры прошлого опроса: если при игре счетчик стоит —
  /// поток встал (оборванный маршрут без ошибки), чинит сторожок движка.
  BigInt? _lastFrames;

  /// Поток встал, хотя должен играть. Сбрасывается сам, как только
  /// кадры снова растут (сторожок пересоздал поток).
  bool streamStalled = false;

  /// Момент последнего старта: тишина в первые секунды — норма
  /// (открывается устройство), баннер ждёт 6 секунд.
  DateTime? startedAt;

  /// Выбранный стиль и сид для следующего старта.
  String currentStyle = 'club';
  BigInt currentSeed = BigInt.from(424242);

  /// 6 символов для ячеек ввода (без дефиса). Синхронизируется
  /// со статусом после действий с бэкендом.
  String boxCode = '000000';

  bool get running => status?.running ?? false;
  bool get paused => status?.paused ?? false;
  String get styleName => status?.styleName ?? currentStyle;
  String get seedCode => status?.seedCode ?? '';

  /// Первичная загрузка: статус, списки, без автозапуска звука.
  Future<void> init() async {
    stylesMeta = await playbackStyles();
    styles = [for (final s in stylesMeta) s.name];
    // Стиль обязан быть выбран: чиним протухший выбор первым из списка,
    // иначе панели пустые, а кнопки молчат.
    if (!styles.contains(currentStyle)) {
      currentStyle = styles.isNotEmpty ? styles.first : currentStyle;
    }
    await refresh();
    if (status != null) {
      if (!styles.contains(status!.styleName) && styles.isNotEmpty) {
        currentStyle = styles.first;
      } else {
        currentStyle = status!.styleName;
      }
      currentSeed = status!.seed;
      boxCode = status!.seedCode.replaceAll('-', '');
    }
    notifyListeners();
  }

  /// Полка стиля (`club`/`rock`/`chill`, '' если неизвестно).
  String styleGroupOf(String name) {
    for (final s in stylesMeta) {
      if (s.name == name) return s.group;
    }
    return '';
  }

  /// Стили в порядке панели: по полкам (порядок первого появления
  /// полки в списке бэкенда, внутри — по алфавиту) или все по алфавиту.
  /// Никаких зашитых списков полок во фронте — порядок задает бэкенд.
  List<String> sortedStyles() {
    final names = styles.toList();
    if (styleSortMode == 'alpha') {
      names.sort();
      return names;
    }
    final order = <String>[];
    for (final s in stylesMeta) {
      if (!order.contains(s.group)) order.add(s.group);
    }
    names.sort((a, b) {
      final ca = order.indexOf(styleGroupOf(a));
      final cb = order.indexOf(styleGroupOf(b));
      return ca != cb ? ca.compareTo(cb) : a.compareTo(b);
    });
    return names;
  }

  void toggleStyleSort() {
    styleSortMode = styleSortMode == 'group' ? 'alpha' : 'group';
    notifyListeners();
  }

  Future<void> refresh() async {
    status = await playbackStatus();
    if (status?.running == true) {
      try {
        stats = await playbackEngineStats();
      } catch (_) {
        // Статистика — необязательна, статус уже есть.
      }
      // Ступор потока: играем не на паузе, прошло 6+ секунд со старта,
      // а счетчик кадров стоит на месте прошлого опроса.
      final s = stats;
      final started = startedAt;
      if (s != null &&
          !(status?.paused ?? true) &&
          started != null &&
          DateTime.now().difference(started).inSeconds >= 6) {
        streamStalled = _lastFrames != null && s.frames == _lastFrames;
        _lastFrames = s.frames;
      } else {
        streamStalled = false;
        if (s != null) _lastFrames = s.frames;
      }
    } else {
      stats = null;
      streamStalled = false;
      _lastFrames = null;
    }
    notifyListeners();
  }

  Future<void> refreshMix() async {
    tracks = await playbackTracks();
    groups = await playbackGroups();
    notifyListeners();
  }

  Future<void> togglePlay() async {
    final s = status;
    if (s == null || !s.running) {
      // Android играет сам (pull через AudioTrack): бэкенд только
      // рендерит, без cpal-потока. Остальные платформы — как раньше.
      status = Platform.isAndroid
          ? await playbackPullStart(styleName: currentStyle, seed: currentSeed)
          : await playbackStart(styleName: currentStyle, seed: currentSeed);
      tracks = await playbackTracks();
      groups = await playbackGroups();
      currentStyle = status?.styleName ?? currentStyle;
      currentSeed = status?.seed ?? currentSeed;
      startedAt = DateTime.now();
    } else {
      status = await playbackSetPaused(paused: !s.paused);
    }
    notifyListeners();
  }

  /// Гарантированно играет (для системных кнопок и прерываний наоборот).
  Future<void> ensurePlaying() async {
    final s = status;
    if (s == null || !s.running) {
      await togglePlay();
    } else if (s.paused) {
      status = await playbackSetPaused(paused: false);
      notifyListeners();
    }
  }

  /// Гарантированно на паузе. Вызывается и прерыванием аудиофокуса.
  Future<void> ensurePaused() async {
    await setPausedExternal(true);
  }

  Future<void> setPausedExternal(bool paused) async {
    final s = status;
    if (s != null && s.running && s.paused != paused) {
      status = await playbackSetPaused(paused: paused);
      notifyListeners();
    }
  }

  Future<void> chooseStyle(String name) async {
    currentStyle = name;
    status = await playbackSetStyle(styleName: name);
    tracks = await playbackTracks();
    groups = await playbackGroups();
    notifyListeners();
  }

  Future<void> submitSeedCode(String code) async {
    if (running) {
      status = await playbackSetSeedCode(code: code);
      currentSeed = status?.seed ?? currentSeed;
      boxCode = status?.seedCode.replaceAll('-', '') ?? boxCode;
      // Сид меняет состав (гости из пула): обновляем панели фактом.
      tracks = await playbackTracks();
      groups = await playbackGroups();
    } else {
      // Бэкенд спит — запоминаем локально, применится на старте.
      currentSeed = seedCodeToBigInt(code);
      boxCode = code.toUpperCase();
    }
    notifyListeners();
  }

  Future<void> randomSeed() async {
    if (running) {
      status = await playbackRandomSeed();
      currentSeed = status?.seed ?? currentSeed;
      boxCode = status?.seedCode.replaceAll('-', '') ?? boxCode;
      tracks = await playbackTracks();
      groups = await playbackGroups();
    } else {
      const alphabet = '0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ';
      final rnd = math.Random();
      final chars =
          List.generate(6, (_) => alphabet[rnd.nextInt(36)]).join();
      boxCode = chars;
      currentSeed = seedCodeToBigInt(chars);
    }
    notifyListeners();
  }

  Future<void> setGroupVolume(String group, double volume) async {
    status = await playbackSetGroupVolume(groupName: group, volume: volume);
    tracks = await playbackTracks();
    groups = await playbackGroups();
    notifyListeners();
  }

  /// Ручка отдельного инструмента (0..1 абсолютных). Ноль исключает слот
  /// из генерации и DSP; бэкенд отклоняет глушение последнего слышимого
  /// (статус вернет факт — ползунок откатится сам).
  Future<void> setTrackVolume(BigInt id, double volume) async {
    status = await playbackSetTrack(instrumentId: id, volume: volume);
    tracks = await playbackTracks();
    notifyListeners();
  }

  Future<void> setTempo(double bpm) async {
    status = await playbackSetTempoBpm(bpm: bpm);
    notifyListeners();
  }

  Future<void> setIntensity(double intensity) async {
    status = await playbackSetIntensity(intensity: intensity);
    notifyListeners();
  }
}

/// `XXX-XXX` (0-9A-Z) в число для старта. Мусор пропускаем —
/// бэкенд при приеме все равно санитизирует.
BigInt seedCodeToBigInt(String code) {
  const alphabet = '0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ';
  var value = BigInt.zero;
  for (final ch in code.toUpperCase().split('')) {
    final idx = alphabet.indexOf(ch);
    if (idx < 0) continue;
    value = value * BigInt.from(36) + BigInt.from(idx);
  }
  return value;
}
