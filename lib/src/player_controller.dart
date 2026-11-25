import 'dart:io' show Platform;
import 'dart:math' as math;

import 'package:flutter/foundation.dart';

import 'package:continuum/src/rust/api/playback.dart';
import 'package:continuum/src/rust/playback.dart';

/// Player state: snapshots from the backend + local frontend choices.
/// All control calls go to the backend and return fresh status —
/// the UI always shows facts, not guesses.
class PlayerController extends ChangeNotifier {
  PlaybackStatus? status;
  List<TrackInfo> tracks = const [];
  List<GroupInfo> groups = const [];
  List<String> styles = const [];

  /// Styles with catalog shelves (`club`/`rock`/`chill`).
  List<StyleMeta> stylesMeta = const [];

  /// Style list mode: `group` (by shelf) or `alpha` (alphabetically).
  String styleSortMode = 'group';

  /// Live audio-path diagnostics (frames/notes/queue) for the banner.
  EngineStats? stats;

  /// Frame count from the previous poll: if it stays put while playing —
  /// the thread has stalled (a broken route without an error); the engine's
  /// watchdog fixes it.
  BigInt? _lastFrames;

  /// Thread stalled although it should be playing. Resets itself as soon as
  /// frames start growing again (the watchdog recreated the thread).
  bool streamStalled = false;

  /// Time of the last start: silence in the first seconds is normal
  /// (the device is opening), the banner waits 6 seconds.
  DateTime? startedAt;

  /// Selected style and seed for the next start.
  String currentStyle = 'club';
  BigInt currentSeed = BigInt.from(424242);

  /// Seed history for the "back" button (A/B comparison without retyping).
  final List<BigInt> _seedHistory = [BigInt.from(424242)];
  int _seedIndex = 0;

  bool get canGoBackSeed => _seedIndex > 0;

  void _rememberSeed(BigInt seed) {
    if (seed == _seedHistory[_seedIndex]) return;
    if (_seedIndex < _seedHistory.length - 1) {
      _seedHistory.removeRange(_seedIndex + 1, _seedHistory.length);
    }
    _seedHistory.add(seed);
    _seedIndex = _seedHistory.length - 1;
    if (_seedHistory.length > 32) {
      _seedHistory.removeAt(0);
      _seedIndex--;
    }
  }

  /// 6 characters for the input boxes (without the dash). Synced
  /// with status after backend actions.
  String boxCode = seedCodeFromBigInt(BigInt.from(424242));

  bool get running => status?.running ?? false;
  bool get paused => status?.paused ?? false;
  String get styleName => status?.styleName ?? currentStyle;
  String get seedCode => status?.seedCode ?? '';

  /// Initial load: status and lists, without auto-starting sound.
  Future<void> init() async {
    stylesMeta = await playbackStyles();
    styles = [for (final s in stylesMeta) s.name];
    // A style must be selected: fix a stale choice with the first in the list,
    // otherwise panels are empty and buttons do nothing.
    if (!styles.contains(currentStyle)) {
      currentStyle = styles.isNotEmpty ? styles.first : currentStyle;
    }
    await refresh();
    // The mixer must be visible before the first Play press: otherwise the
    // instrument panel is empty and the lineup can't be set up in advance.
    await refreshMix();
    if (status != null) {
      if (!styles.contains(status!.styleName) && styles.isNotEmpty) {
        currentStyle = styles.first;
      } else {
        currentStyle = status!.styleName;
      }
      currentSeed = status!.seed;
      _rememberSeed(currentSeed);
      boxCode = status!.seedCode.replaceAll('-', '');
    }
    notifyListeners();
  }

  /// A style's shelf (`club`/`rock`/`chill`, '' if unknown).
  String styleGroupOf(String name) {
    for (final s in stylesMeta) {
      if (s.name == name) return s.group;
    }
    return '';
  }

  /// Styles in panel order: by shelf (order of the shelf's first appearance
  /// in the backend list, alphabetical within) or all alphabetically.
  /// No hardcoded shelf lists in the frontend — the backend sets the order.
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
        // Stats are optional; we already have status.
      }
      // Stalled thread: playing, not paused, 6+ seconds since start,
      // but the frame counter is unchanged from the previous poll.
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
      // Android plays on its own (pull via AudioTrack): the backend only
      // renders, without a cpal thread. Other platforms — as before.
      status = Platform.isAndroid
          ? await playbackPullStart(styleName: currentStyle, seed: currentSeed)
          : await playbackStart(styleName: currentStyle, seed: currentSeed);
      tracks = await playbackTracks();
      groups = await playbackGroups();
      currentStyle = status?.styleName ?? currentStyle;
      currentSeed = status?.seed ?? currentSeed;
      _rememberSeed(currentSeed);
      startedAt = DateTime.now();
    } else {
      status = await playbackSetPaused(paused: !s.paused);
    }
    notifyListeners();
  }

  /// Guaranteed to be playing (for system buttons; conversely, for interruptions).
  Future<void> ensurePlaying() async {
    final s = status;
    if (s == null || !s.running) {
      await togglePlay();
    } else if (s.paused) {
      status = await playbackSetPaused(paused: false);
      notifyListeners();
    }
  }

  /// Guaranteed to be paused. Also called by audio-focus interruption.
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
      _rememberSeed(currentSeed);
      boxCode = status?.seedCode.replaceAll('-', '') ?? boxCode;
      // A seed changes the lineup (guests from the pool): update panels with facts.
      tracks = await playbackTracks();
      groups = await playbackGroups();
    } else {
      // Backend asleep — remember locally, applied on start.
      currentSeed = seedCodeToBigInt(code);
      _rememberSeed(currentSeed);
      boxCode = code.toUpperCase();
    }
    notifyListeners();
  }

  Future<void> randomSeed() async {
    if (running) {
      status = await playbackRandomSeed();
      currentSeed = status?.seed ?? currentSeed;
      _rememberSeed(currentSeed);
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
      _rememberSeed(currentSeed);
    }
    notifyListeners();
  }

  /// One step back in seed history: cheap A/B comparison of two ideas.
  Future<void> previousSeed() async {
    if (!canGoBackSeed) return;
    _seedIndex--;
    final seed = _seedHistory[_seedIndex];
    if (running) {
      status = await playbackSetSeed(seed: seed);
      tracks = await playbackTracks();
      groups = await playbackGroups();
      currentSeed = status?.seed ?? seed;
      boxCode = status?.seedCode.replaceAll('-', '') ?? seedCodeFromBigInt(seed);
    } else {
      currentSeed = seed;
      boxCode = seedCodeFromBigInt(seed);
    }
    notifyListeners();
  }

  Future<void> setGroupVolume(String group, double volume) async {
    status = await playbackSetGroupVolume(groupName: group, volume: volume);
    tracks = await playbackTracks();
    groups = await playbackGroups();
    notifyListeners();
  }

  /// An individual instrument's fader (0..1 absolute). Zero excludes the slot
  /// from generation and DSP; the backend rejects muting the last audible one
  /// (status returns the fact — the slider rolls back on its own).
  Future<void> setTrackVolume(BigInt id, double volume) async {
    status = await playbackSetTrack(instrumentId: id, volume: volume);
    tracks = await playbackTracks();
    notifyListeners();
  }

  /// Reset the instrument list to the style defaults: clears the
  /// "edited by listener" flag so the style affects the lineup again.
  Future<void> resetInstruments() async {
    status = await playbackResetInstruments();
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

  Future<void> resetTempo() async {
    status = await playbackSetTempoBpm(bpm: 0);
    notifyListeners();
  }

  Future<void> resetIntensity() async {
    status = await playbackSetIntensity(intensity: 1.0);
    notifyListeners();
  }

  /// Master effects: reverb room, treble cutoff, sidechain.
  Future<void> setMasterFx({
    required double reverbWet,
    required double reverbSize,
    required double trebleHz,
    required double sidechain,
  }) async {
    status = await playbackSetMasterFx(
      reverbWet: reverbWet,
      reverbSize: reverbSize,
      trebleHz: trebleHz,
      sidechain: sidechain,
    );
    notifyListeners();
  }

  Future<void> resetMasterFx() async {
    status = await playbackResetMasterFx();
    notifyListeners();
  }

  /// Filter openness: an offset on top of the section envelope.
  Future<void> setToneOpen(double offset) async {
    status = await playbackSetToneOpen(offset: offset);
    notifyListeners();
  }
}

/// Seed number to canonical code `XXX-XXX` (0-9A-Z, 6 characters).
/// Used for the initial state before the backend responds.
String seedCodeFromBigInt(BigInt value) {
  const alphabet = '0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ';
  final space = BigInt.from(36).pow(6);
  var v = value % space;
  if (v.isNegative) v += space;
  final chars = List<String>.filled(6, '0');
  for (var i = 5; i >= 0; i--) {
    chars[i] = alphabet[(v % BigInt.from(36)).toInt()];
    v ~/= BigInt.from(36);
  }
  return chars.join();
}

/// `XXX-XXX` (0-9A-Z) to a number for startup. Garbage is skipped —
/// the backend sanitizes it on receipt anyway.
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
