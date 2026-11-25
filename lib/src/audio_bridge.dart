import 'dart:async';
import 'dart:io';

import 'package:audio_service/audio_service.dart';
import 'package:audio_session/audio_session.dart';
import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';
import 'package:image/image.dart' as img;
import 'package:path_provider/path_provider.dart';

import 'package:continuum/src/player_controller.dart';
import 'package:continuum/src/rust/api/playback.dart';

/// Bridge to the system player: background service, notification with pause,
/// audio focus ("main audio stream"). All sound is still synthesized by
/// Rust — the bridge only controls it and reports state.
///
/// On Android, sound comes from the native Rust thread (its own AudioTrack
/// via JNI): Dart only sends play/pause commands, it is not in the audio path.
///
/// If the native service is unavailable (desktop, init error), the bridge
/// silently falls back to direct controller calls — the UI doesn't notice.
class AudioBridge {
  AudioBridge(this._controller);

  final PlayerController _controller;
  ContAudioHandler? _handler;
  StreamSubscription? _interruptionSub;
  StreamSubscription? _becomingNoisySub;
  StreamSubscription? _noisySub;

  bool get available => _handler != null;

  Future<void> init() async {
    try {
      debugPrint('[audio] AudioService.init...');
      final handler = ContAudioHandler(_controller);
      await AudioService.init(
        builder: () => handler,
        config: const AudioServiceConfig(
          androidNotificationChannelId: 'com.continuum.app.channel.audio',
          androidNotificationChannelName: 'Continuum playback',
          // Pause does not drop foreground: restarting the service from the
          // background on Android 12+ would throw an exception without this.
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
        // A call/navigation app took audio focus — pause like a real player.
        if (event.begin) {
          debugPrint('[audio] interruption begin -> pause');
          unawaited(_controller.setPausedExternal(true));
        }
      });
      _becomingNoisySub = session.becomingNoisyEventStream.listen((_) {
        debugPrint('[audio] becoming noisy -> pause');
        unawaited(pause());
      });
      // Duplicate native signal (the track is throttled itself; the event is
      // for pausing state and the notification, if the main isolate is alive).
      if (Platform.isAndroid) {
        _noisySub = const EventChannel('continuum/audio_events')
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

  /// Play on Android: backend session + native audio thread.
  /// Other platforms — as before, through the controller directly.
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

  /// Native thread is alive (for the diagnostics screen).
  Future<bool> pullActive() => Platform.isAndroid
      ? playbackPullAlive()
      : Future.value(false);

  /// Native thread log for the diagnostics screen.
  Future<List<String>> diagLog() => Platform.isAndroid
      ? playbackPullDiag()
      : Future.value(const <String>[]);

  /// Capture the next 30 seconds of the stream to a file (to examine glitches).
  /// The name contains the style and seed — no need to guess the context.
  /// The path is the app's public folder. null — not playing.
  Future<String?> startCapture() async {
    if (!Platform.isAndroid) return null;
    try {
      final ext = await getExternalStorageDirectory();
      final dir = ext ?? await getTemporaryDirectory();
      final style = _controller.styleName.replaceAll(RegExp(r'[^a-z0-9]'), '');
      final seed = _controller.seedCode.replaceAll('-', '');
      final path =
          '${dir.path}/cont_capture_${style}_${seed}_${DateTime.now().millisecondsSinceEpoch}.f32';
      return await playbackPullCapture(path: path) ? path : null;
    } catch (_) {
      return null;
    }
  }

  /// Warm up the art cache after the style list is loaded: the first
  /// play/style tap won't stall on image decoding in the UI thread.
  /// Fire-and-forget: an error here has no effect.
  Future<void> warmup(List<String> styles) async {
    for (final style in styles) {
      await styleArtUri(style);
    }
  }

  /// Update the notification after actions outside the play/pause buttons
  /// (style or seed change): called from the UI right next to the controller call.
  /// Note: on Android 13+ showing a notification requires the
  /// POST_NOTIFICATIONS permission — enabled manually in app settings
  /// (sound and background playback work independently of it).
  Future<void> sync() async {
    await _handler?.syncFromController();
  }

  Future<void> dispose() async {
    await _interruptionSub?.cancel();
    await _becomingNoisySub?.cancel();
    await _noisySub?.cancel();
  }
}

/// Handler for system buttons (notification, headset, Android Auto).
/// Delegates to PlayerController, then publishes state.
/// On Android, starts/stops the native thread along with the state.
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
      artist: seed.isEmpty ? 'Continuum' : 'Seed $seed',
      artUri: await styleArtUri(style),
    ));
    playbackState.add(playbackState.value.copyWith(
      controls: [if (playing) MediaControl.pause else MediaControl.play],
      androidCompactActionIndices: const [0],
      processingState: AudioProcessingState.ready,
      playing: playing,
    ));
  }

  /// Called by the bridge/UI after external changes (style, seed).
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
    // For this player stop = pause + remove the notification.
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

/// Darkened style artwork for the notification (cached file).
/// Shared by the handler and warmup: the first tap doesn't stall on decoding.
Future<Uri?> styleArtUri(String style) async {
  final key = style.toLowerCase();
  try {
    final dir = await getTemporaryDirectory();
    final file = File('${dir.path}/cont_art_$key.png');
    if (await file.exists()) return Uri.file(file.path);
    final data = await rootBundle.load('assets/images/style_$key.png');
    // Decode and per-pixel processing run in a separate isolate: warming up
    // dozens of artworks on the main thread froze the UI on first launch.
    final out = await compute(_darkenArt, data.buffer.asUint8List());
    if (out == null) return null;
    await file.writeAsBytes(out);
    return Uri.file(file.path);
  } catch (e) {
    debugPrint('[audio] art unavailable: $e');
    return null;
  }
}

/// Top-level function for `compute`: darken and resize to 128 px.
List<int>? _darkenArt(Uint8List bytes) {
  final decoded = img.decodeImage(bytes);
  if (decoded == null) return null;
  final small = img.copyResize(decoded, width: 128);
  for (final pixel in small) {
    pixel.r = (pixel.r * 0.55).toInt();
    pixel.g = (pixel.g * 0.55).toInt();
    pixel.b = (pixel.b * 0.55).toInt();
  }
  return img.encodePng(small);
}
