import 'dart:async';
import 'dart:io';

import 'package:flutter/services.dart';

enum BackendState { idle, starting, playing, stopping, unsupported, error }

class PlaybackSnapshot {
  const PlaybackSnapshot({
    required this.state,
    this.presetName,
    this.elapsedMs,
  });

  final BackendState state;
  final String? presetName;
  final int? elapsedMs;

  bool get isPlaying =>
      state == BackendState.playing || state == BackendState.starting;
}

abstract class PlaybackBackend {
  BackendState get state;
  String get status;
  bool get isPlaying;

  Future<void> play(String preset);
  Future<void> pause();
  Future<void> selectPreset(String preset);
  Future<void> dispose();
  void setPlaybackListener(void Function(PlaybackSnapshot snapshot)? listener);

  void applyControls({
    required double tempo,
    required double swing,
    required Map<String, double> volumes,
  });
}

class BackendPlayer implements PlaybackBackend {
  BackendPlayer() : _backend = _createBackend();

  final PlaybackBackend _backend;

  @override
  BackendState get state => _backend.state;

  @override
  String get status => _backend.status;

  @override
  bool get isPlaying => _backend.isPlaying;

  @override
  Future<void> play(String preset) => _backend.play(preset);

  @override
  Future<void> pause() => _backend.pause();

  @override
  Future<void> selectPreset(String preset) => _backend.selectPreset(preset);

  @override
  Future<void> dispose() => _backend.dispose();

  @override
  void setPlaybackListener(void Function(PlaybackSnapshot snapshot)? listener) {
    _backend.setPlaybackListener(listener);
  }

  @override
  void applyControls({
    required double tempo,
    required double swing,
    required Map<String, double> volumes,
  }) {
    _backend.applyControls(
      tempo: tempo,
      swing: swing,
      volumes: volumes,
    );
  }
}

PlaybackBackend _createBackend() {
  if (Platform.isWindows || Platform.isLinux || Platform.isMacOS) {
    return DesktopProcessBackend();
  }

  if (Platform.isAndroid) {
    return AndroidAudioBackend();
  }

  if (Platform.isIOS) {
    return UnsupportedBackend('iOS audio is not connected yet');
  }

  return UnsupportedBackend('Platform is not supported');
}

class DesktopProcessBackend implements PlaybackBackend {
  Process? _process;
  String? _processPreset;
  BackendState _state = BackendState.idle;
  String _status = 'Ready';
  void Function(PlaybackSnapshot snapshot)? _listener;

  @override
  BackendState get state => _state;

  @override
  String get status => _status;

  @override
  bool get isPlaying =>
      _state == BackendState.playing || _state == BackendState.starting;

  @override
  Future<void> play(String preset) async {
    if (isPlaying) {
      return;
    }

    if (_process != null) {
      if (_processPreset != null &&
          _processPreset!.toLowerCase() != preset.toLowerCase()) {
        await _stopProcess(notify: false);
      } else {
        _state = BackendState.playing;
        _status = 'Playing';
        _sendLine('resume');
        await _process!.stdin.flush().catchError((_) {});
        _notify(preset: _processPreset ?? preset);
        return;
      }
    }

    _state = BackendState.starting;
    _status = 'Starting';

    try {
      final launch = await _resolveLaunch(preset);
      _process = await Process.start(
        launch.executable,
        launch.arguments,
        workingDirectory: launch.workingDirectory,
      );
      unawaited(_process!.stdout.drain<void>());
      unawaited(_process!.stderr.drain<void>());

      unawaited(
        _process!.exitCode.then((code) {
          if (_state != BackendState.stopping) {
            _process = null;
            _processPreset = null;
            _state = code == 0 ? BackendState.idle : BackendState.error;
            _status = code == 0 ? 'Stopped' : 'Backend exited: $code';
            _notify();
          }
        }),
      );

      _state = BackendState.playing;
      _status = 'Playing';
      _processPreset = preset;
      _notify(preset: preset);
    } catch (_) {
      _state = BackendState.error;
      _status = 'Backend not found';
      _process = null;
      _processPreset = null;
      _notify(preset: preset);
    }
  }

  @override
  Future<void> pause() async {
    if (_process == null) {
      _state = BackendState.idle;
      _status = 'Paused';
      _notify();
      return;
    }

    _state = BackendState.stopping;
    _status = 'Paused';
    _sendLine('pause');
    await _process!.stdin.flush().catchError((_) {});
    _state = BackendState.idle;
    _notify(preset: _processPreset);
  }

  @override
  Future<void> selectPreset(String preset) async {
    final samePreset = _processPreset != null &&
        _processPreset!.toLowerCase() == preset.toLowerCase();
    if (_process != null && samePreset) {
      _notify(preset: preset);
      return;
    }

    final wasPlaying = isPlaying;
    if (_process != null) {
      await _stopProcess(notify: false);
    }

    if (wasPlaying) {
      await play(preset);
      return;
    }

    _notify(preset: preset);
  }

  @override
  Future<void> dispose() => _stopProcess(notify: true);

  @override
  void setPlaybackListener(void Function(PlaybackSnapshot snapshot)? listener) {
    _listener = listener;
  }

  @override
  void applyControls({
    required double tempo,
    required double swing,
    required Map<String, double> volumes,
  }) {
    if (!isPlaying) {
      return;
    }

    _sendLine('tempo ${tempo.toStringAsFixed(3)}');
    _sendLine('swing ${swing.toStringAsFixed(3)}');
    for (final entry in volumes.entries) {
      _sendLine('instrument ${entry.key} ${entry.value.toStringAsFixed(3)}');
    }
  }

  void _sendLine(String line) {
    try {
      _process?.stdin.writeln(line);
    } catch (_) {
      _state = BackendState.error;
      _status = 'Backend is not responding';
      _notify();
    }
  }

  void _notify({String? preset}) {
    _listener?.call(PlaybackSnapshot(state: _state, presetName: preset));
  }

  Future<void> _stopProcess({required bool notify}) async {
    if (_process == null) {
      _state = BackendState.idle;
      _status = 'Stopped';
      if (notify) {
        _notify();
      }
      return;
    }

    _state = BackendState.stopping;
    _status = 'Stopped';
    _sendLine('stop');
    await _process!.stdin.flush().catchError((_) {});
    final code = await _process!.exitCode.timeout(
      const Duration(seconds: 2),
      onTimeout: () {
        _process!.kill();
        return -1;
      },
    );
    _process = null;
    _processPreset = null;
    _state = code == 0 || code == -1 ? BackendState.idle : BackendState.error;
    if (notify) {
      _notify();
    }
  }

  Future<_LaunchCommand> _resolveLaunch(String preset) async {
    final repoRoot = await _findRepoRoot();
    final envPath = Platform.environment['CONTINUUM_BACKEND'];
    final exeName = Platform.isWindows ? 'continuum.exe' : 'continuum';
    final candidates = <File>[
      if (envPath != null && envPath.isNotEmpty) File(envPath),
      File(
        '${repoRoot.path}${Platform.pathSeparator}target${Platform.pathSeparator}debug${Platform.pathSeparator}$exeName',
      ),
      File(
        '${repoRoot.path}${Platform.pathSeparator}backend${Platform.pathSeparator}target${Platform.pathSeparator}debug${Platform.pathSeparator}$exeName',
      ),
    ];

    for (final candidate in candidates) {
      if (await candidate.exists()) {
        return _LaunchCommand(
          executable: candidate.path,
          arguments: [preset.toLowerCase()],
          workingDirectory: repoRoot.path,
        );
      }
    }

    return _LaunchCommand(
      executable: 'cargo',
      arguments: ['run', '-p', 'continuum', '--', preset.toLowerCase()],
      workingDirectory: repoRoot.path,
    );
  }

  Future<Directory> _findRepoRoot() async {
    var cursor = Directory.current;

    for (var depth = 0; depth < 8; depth++) {
      final backendManifest = File(
        '${cursor.path}${Platform.pathSeparator}backend${Platform.pathSeparator}Cargo.toml',
      );
      if (await backendManifest.exists()) {
        return cursor;
      }

      final parent = cursor.parent;
      if (parent.path == cursor.path) {
        break;
      }
      cursor = parent;
    }

    return Directory.current.parent;
  }
}

class AndroidAudioBackend implements PlaybackBackend {
  static const MethodChannel _channel = MethodChannel('continuum/audio');

  BackendState _state = BackendState.idle;
  String _status = 'Ready';
  void Function(PlaybackSnapshot snapshot)? _listener;

  AndroidAudioBackend() {
    _channel.setMethodCallHandler(_handleNativeCall);
  }

  @override
  BackendState get state => _state;

  @override
  String get status => _status;

  @override
  bool get isPlaying =>
      _state == BackendState.playing || _state == BackendState.starting;

  @override
  Future<void> play(String preset) async {
    if (isPlaying) {
      return;
    }

    _state = BackendState.starting;
    _status = 'Starting';

    try {
      await _channel.invokeMethod<void>('play', {
        'presetId': _presetId(preset),
      });
      _state = BackendState.playing;
      _status = 'Playing';
      _notify(presetName: preset);
    } catch (error) {
      _state = BackendState.error;
      _status = _androidErrorStatus(error);
      _notify(presetName: preset);
    }
  }

  @override
  Future<void> pause() async {
    if (_state == BackendState.idle) {
      _status = 'Paused';
      _notify();
      return;
    }

    _state = BackendState.stopping;
    _status = 'Paused';

    try {
      await _channel.invokeMethod<void>('pause');
      _state = BackendState.idle;
      _notify();
    } catch (_) {
      _state = BackendState.error;
      _status = 'Android audio is not responding';
      _notify();
    }
  }

  @override
  Future<void> selectPreset(String preset) async {
    try {
      await _channel.invokeMethod<void>('selectPreset', {
        'presetId': _presetId(preset),
      });
      _status = isPlaying ? 'Playing' : 'Paused';
      _notify(presetName: preset);
    } catch (error) {
      _state = BackendState.error;
      _status = _androidErrorStatus(error);
      _notify(presetName: preset);
    }
  }

  @override
  Future<void> dispose() => pause();

  @override
  void setPlaybackListener(void Function(PlaybackSnapshot snapshot)? listener) {
    _listener = listener;
    if (listener != null) {
      unawaited(_syncNativeState());
    }
  }

  @override
  void applyControls({
    required double tempo,
    required double swing,
    required Map<String, double> volumes,
  }) {
    if (!isPlaying) {
      return;
    }

    unawaited(
      _channel.invokeMethod<void>('controls', {
        'tempo': tempo,
        'swing': swing,
        'volumes': volumes,
      }),
    );
  }

  int _presetId(String preset) {
    return switch (preset.toLowerCase()) {
      'jazz' => 1,
      _ => 0,
    };
  }

  Future<void> _handleNativeCall(MethodCall call) async {
    if (call.method != 'playbackEvent') {
      return;
    }

    _applyNativeSnapshot(call.arguments);
  }

  Future<void> _syncNativeState() async {
    try {
      final snapshot = await _channel.invokeMethod<Object?>('state');
      _applyNativeSnapshot(snapshot);
    } catch (_) {
      // The sync request is best-effort; normal play/pause commands still
      // report their own status.
    }
  }

  void _applyNativeSnapshot(Object? value) {
    if (value is! Map) {
      return;
    }

    final isPlaying = value['isPlaying'] == true;
    final presetName = value['presetName'] as String?;
    final elapsedValue = value['elapsedMs'];
    final elapsedMs = elapsedValue is int
        ? elapsedValue
        : elapsedValue is num
            ? elapsedValue.toInt()
            : null;

    _state = isPlaying ? BackendState.playing : BackendState.idle;
    _status = isPlaying ? 'Playing' : 'Paused';
    _notify(presetName: presetName, elapsedMs: elapsedMs);
  }

  void _notify({String? presetName, int? elapsedMs}) {
    _listener?.call(
      PlaybackSnapshot(
        state: _state,
        presetName: presetName,
        elapsedMs: elapsedMs,
      ),
    );
  }

  String _androidErrorStatus(Object error) {
    if (error is PlatformException) {
      final message = error.message;
      if (message != null && message.isNotEmpty) {
        return 'Audio: ${_compact(message)}';
      }
    }
    return 'Android audio did not start';
  }

  String _compact(String value) {
    const maxLength = 54;
    final singleLine = value.replaceAll('\n', ' ').trim();
    if (singleLine.length <= maxLength) {
      return singleLine;
    }
    return '${singleLine.substring(0, maxLength - 3)}...';
  }
}

class UnsupportedBackend implements PlaybackBackend {
  UnsupportedBackend(this._status);

  final String _status;

  @override
  BackendState get state => BackendState.unsupported;

  @override
  String get status => _status;

  @override
  bool get isPlaying => false;

  @override
  Future<void> play(String preset) async {}

  @override
  Future<void> pause() async {}

  @override
  Future<void> selectPreset(String preset) async {}

  @override
  Future<void> dispose() async {}

  @override
  void setPlaybackListener(void Function(PlaybackSnapshot snapshot)? listener) {}

  @override
  void applyControls({
    required double tempo,
    required double swing,
    required Map<String, double> volumes,
  }) {}
}

class _LaunchCommand {
  const _LaunchCommand({
    required this.executable,
    required this.arguments,
    required this.workingDirectory,
  });

  final String executable;
  final List<String> arguments;
  final String workingDirectory;
}
