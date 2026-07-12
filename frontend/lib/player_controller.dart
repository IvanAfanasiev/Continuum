import 'dart:async';

import 'package:flutter/material.dart';

import 'backend_player.dart';

class ContinuumPlayerController extends ChangeNotifier {
  ContinuumPlayerController() {
    _player.setPlaybackListener(_handlePlaybackSnapshot);
  }

  static const Duration _presetCommitDelay = Duration(milliseconds: 200);

  final BackendPlayer _player = BackendPlayer();
  final List<PresetState> _presets = [
    PresetState(
      name: 'Ambient',
      color: const Color(0xff7dd3fc),
      instruments: const ['Pad', 'Piano', 'Bass', 'Triangle'],
      tempo: 1.0,
      swing: 0.5,
      backgroundAsset: 'assets/backgrounds/Ambient.jfif',
    ),
    PresetState(
      name: 'Jazz',
      color: const Color(0xffffc86b),
      instruments: const ['Piano', 'Bass', 'Kick', 'Ride', 'Hihat'],
      tempo: 1.0,
      swing: 0.5,
      backgroundAsset: 'assets/backgrounds/Jazz.png',
    ),
  ];

  int _presetIndex = 1;
  Timer? _presetCommitTimer;
  bool _presetCommitRunning = false;
  bool _playbackCommitRunning = false;
  bool _disposed = false;
  bool? _optimisticIsPlaying;
  bool? _pendingPlaybackState;
  String? _pendingPresetName;
  String? _localPresetIntentName;
  String? _committedPresetName;
  DateTime? _lastPresetIntentAt;

  PresetState get preset => _presets[_presetIndex];
  List<PresetState> get presets => List.unmodifiable(_presets);
  bool get isPlaying => _optimisticIsPlaying ?? _player.isPlaying;
  String get status => _player.status;
  BackendState get state => _player.state;

  Future<void> togglePlayback() {
    _cancelPendingPresetCommit();
    _pendingPlaybackState = !isPlaying;
    _optimisticIsPlaying = _pendingPlaybackState;
    _localPresetIntentName = preset.name;
    _startPlaybackCommit();
    _notify();
    return Future.value();
  }

  Future<void> switchPreset(int direction) {
    _presetIndex = (_presetIndex + direction) % _presets.length;
    if (_presetIndex < 0) {
      _presetIndex = _presets.length - 1;
    }

    _queuePresetCommit(preset.name);
    _notify();
    return Future.value();
  }

  void updatePreset(PresetState next) {
    _presets[_presetIndex] = next;
    _sendControls(next);
    _notify();
  }

  @override
  void dispose() {
    _disposed = true;
    _presetCommitTimer?.cancel();
    _player.setPlaybackListener(null);
    unawaited(_player.dispose());
    super.dispose();
  }

  void _handlePlaybackSnapshot(PlaybackSnapshot snapshot) {
    if (_disposed) {
      return;
    }

    final presetName = snapshot.presetName;
    var changedPreset = false;
    final localIntent = _localPresetIntentName;

    if (presetName != null &&
        localIntent != null &&
        !_samePresetName(presetName, localIntent)) {
      return;
    }

    if (presetName != null) {
      _committedPresetName = presetName;
      final nextIndex = _presets.indexWhere(
        (preset) => _samePresetName(preset.name, presetName),
      );
      if (nextIndex >= 0 && nextIndex != _presetIndex) {
        _presetIndex = nextIndex;
        changedPreset = true;
      }
    }

    if (presetName != null &&
        localIntent != null &&
        _samePresetName(presetName, localIntent) &&
        _pendingPresetName == null &&
        !_presetCommitRunning) {
      _localPresetIntentName = null;
    }

    if (snapshot.isPlaying && changedPreset) {
      _sendControls(preset);
    }
    _notify();
  }

  void _queuePresetCommit(String presetName) {
    _pendingPresetName = presetName;
    _localPresetIntentName = presetName;
    _lastPresetIntentAt = DateTime.now();
    _schedulePresetCommit();
  }

  void _schedulePresetCommit() {
    _presetCommitTimer?.cancel();
    if (_presetCommitRunning || _pendingPresetName == null) {
      return;
    }

    final lastIntentAt = _lastPresetIntentAt;
    final elapsed = lastIntentAt == null
        ? _presetCommitDelay
        : DateTime.now().difference(lastIntentAt);
    final wait = elapsed >= _presetCommitDelay
        ? Duration.zero
        : _presetCommitDelay - elapsed;

    _presetCommitTimer = Timer(wait, _startPresetCommit);
  }

  void _startPresetCommit() {
    _presetCommitTimer = null;
    if (_presetCommitRunning) {
      return;
    }

    final presetName = _pendingPresetName;
    if (presetName == null) {
      return;
    }

    final lastIntentAt = _lastPresetIntentAt;
    if (lastIntentAt != null) {
      final elapsed = DateTime.now().difference(lastIntentAt);
      if (elapsed < _presetCommitDelay) {
        _schedulePresetCommit();
        return;
      }
    }

    _pendingPresetName = null;
    _presetCommitRunning = true;
    unawaited(_commitPresetSelection(presetName));
  }

  void _startPlaybackCommit() {
    if (_playbackCommitRunning) {
      return;
    }

    final shouldPlay = _pendingPlaybackState;
    if (shouldPlay == null) {
      return;
    }

    _pendingPlaybackState = null;
    _playbackCommitRunning = true;
    unawaited(_commitPlaybackState(shouldPlay, preset.name));
  }

  Future<void> _commitPlaybackState(bool shouldPlay, String presetName) async {
    try {
      if (shouldPlay) {
        await _player.play(presetName);
        _committedPresetName = presetName;
        if (_samePresetName(preset.name, presetName)) {
          _sendControls(preset);
        }
      } else {
        await _player.pause();
        if (_committedPresetName == null ||
            !_samePresetName(_committedPresetName!, presetName)) {
          await _player.selectPreset(presetName);
        }
        _committedPresetName = presetName;
      }
    } catch (_) {
      _notify();
    } finally {
      _playbackCommitRunning = false;
      if (_disposed) {
        return;
      }

      if (_pendingPlaybackState != null) {
        _startPlaybackCommit();
        return;
      }

      _optimisticIsPlaying = null;
      _localPresetIntentName = null;
      _notify();
    }
  }

  Future<void> _commitPresetSelection(String presetName) async {
    try {
      if (_committedPresetName != null &&
          _samePresetName(_committedPresetName!, presetName)) {
        if (_samePresetName(preset.name, presetName)) {
          _sendControls(preset);
          _localPresetIntentName = null;
        }
        _notify();
        return;
      }

      await _player.selectPreset(presetName);
      if (_disposed) {
        return;
      }

      _committedPresetName = presetName;
      if (_samePresetName(preset.name, presetName)) {
        _sendControls(preset);
        _localPresetIntentName = null;
      }

      _notify();
    } catch (_) {
      _notify();
    } finally {
      _presetCommitRunning = false;
      if (!_disposed && _pendingPresetName != null) {
        _schedulePresetCommit();
      }
    }
  }

  void _cancelPendingPresetCommit() {
    _presetCommitTimer?.cancel();
    _presetCommitTimer = null;
    _pendingPresetName = null;
    _lastPresetIntentAt = null;
  }

  void _sendControls(PresetState preset) {
    _player.applyControls(
      tempo: preset.tempo,
      swing: preset.swing,
      volumes: preset.volumes.map(
        (instrument, value) => MapEntry(
          instrument,
          _instrumentVolumeToBackend(value),
        ),
      ),
    );
  }

  void _notify() {
    if (!_disposed) {
      notifyListeners();
    }
  }

  bool _samePresetName(String first, String second) {
    return first.toLowerCase() == second.toLowerCase();
  }
}

class PresetState {
  PresetState({
    required this.name,
    required this.color,
    required this.instruments,
    required this.tempo,
    required this.swing,
    required this.backgroundAsset,
    Map<String, double>? volumes,
  }) : volumes = volumes ??
            {
              for (final instrument in instruments)
                instrument: _defaultVolume(),
            };

  final String name;
  final Color color;
  final List<String> instruments;
  final double tempo;
  final double swing;
  final String? backgroundAsset;
  final Map<String, double> volumes;

  PresetState copyWith({
    double? tempo,
    double? swing,
    Map<String, double>? volumes,
  }) {
    return PresetState(
      name: name,
      color: color,
      instruments: instruments,
      tempo: tempo ?? this.tempo,
      swing: swing ?? this.swing,
      backgroundAsset: backgroundAsset,
      volumes: volumes ?? Map<String, double>.from(this.volumes),
    );
  }
}

double _defaultVolume() {
  return 0.5;
}

double _instrumentVolumeToBackend(double value) {
  final normalized = value.clamp(0.0, 1.0).toDouble();
  if (normalized <= 0.5) {
    return normalized * 2.0;
  }
  return 1.0 + (normalized - 0.5);
}
