import 'dart:async';
import 'dart:io' show File, Platform;
import 'dart:ui' show PlatformDispatcher;

import 'package:flutter/material.dart';
import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart';

import 'package:continuum/src/audio_bridge.dart';
import 'package:continuum/src/player_controller.dart';
import 'package:continuum/src/player_screen.dart';
import 'package:continuum/src/rust/frb_generated.dart';

/// Startup stages with a live log: if it hangs or crashes on a device,
/// you can see WHERE (instead of a black screen). The log is duplicated to logcat.
class BootLog extends ChangeNotifier {
  final lines = <String>[];

  void add(String line) {
    final stamped = line;
    lines.add(stamped);
    debugPrint('[boot] $stamped');
    notifyListeners();
  }
}

/// Find the native library on desktop. The generated loader only looks into
/// `rust/target/release/`, which breaks `flutter run` in debug mode and
/// standalone bundles. Android/iOS keep using the generated loader (null).
ExternalLibrary? _nativeLibrary() {
  if (Platform.isAndroid || Platform.isIOS) return null;
  const stem = 'rust_lib_continuum';
  final name = Platform.isWindows
      ? '$stem.dll'
      : Platform.isMacOS
          ? 'lib$stem.dylib'
          : 'lib$stem.so';
  final exeDir = File(Platform.resolvedExecutable).parent.path;
  for (final path in <String>[
    'rust/target/release/$name',
    'rust/target/debug/$name',
    '$exeDir/$name',
    name,
  ]) {
    final file = File(path);
    if (file.existsSync()) return ExternalLibrary.open(file.absolute.path);
  }
  return null;
}

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  FlutterError.onError = (details) {
    debugPrint('[flutter-error] ${details.exceptionAsString()}');
    FlutterError.presentError(details);
  };
  // Async errors outside runApp (FFI, timers) would otherwise be lost silently.
  PlatformDispatcher.instance.onError = (error, stack) {
    debugPrint('[async-error] $error\n$stack');
    return true;
  };
  runApp(const BootApp());
}

class _Ready {
  _Ready(this.controller, this.bridge);

  final PlayerController controller;
  final AudioBridge bridge;
}

class BootApp extends StatefulWidget {
  const BootApp({super.key});

  @override
  State<BootApp> createState() => _BootAppState();
}

class _BootAppState extends State<BootApp> {
  final _log = BootLog();
  _Ready? _ready;
  Object? _error;
  StackTrace? _stack;

  @override
  void initState() {
    super.initState();
    _boot();
  }

  Future<void> _boot() async {
    // Retry after an error: old subscriptions/handlers must not live
    // alongside the new ones (otherwise events arrive in two controllers).
    await _ready?.bridge.dispose();
    setState(() {
      _ready = null;
      _error = null;
      _stack = null;
    });
    try {
      _log.add('RustLib.init...');
      await RustLib.init(externalLibrary: _nativeLibrary());
      _log.add('rust ok');
      final controller = PlayerController();
      final bridge = AudioBridge(controller);
      _log.add('audio service...');
      await bridge.init();
      _log.add('audio ok');
      _log.add('controller init...');
      await controller.init();
      _log.add('ready');
      // Warm up notification artwork in the background: the first play/style tap
      // won't wait for image decoding.
      unawaited(bridge.warmup(controller.styles));
      if (!mounted) return;
      setState(() => _ready = _Ready(controller, bridge));
    } catch (e, st) {
      debugPrint('[boot] FAILED: $e\n$st');
      if (!mounted) return;
      setState(() {
        _error = e;
        _stack = st;
      });
    }
  }

  @override
  void dispose() {
    unawaited(_ready?.bridge.dispose());
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final ready = _ready;
    if (ready != null) {
      return ContinuumApp(
        controller: ready.controller,
        bridge: ready.bridge,
      );
    }
    return MaterialApp(
      title: 'Continuum',
      debugShowCheckedModeBanner: false,
      theme: ThemeData.dark(useMaterial3: true),
      home: _error == null
          ? BootLoading(log: _log)
          : BootError(
              error: _error!,
              stack: _stack,
              onRetry: _boot,
            ),
    );
  }
}

/// Loading screen with a live log: shows where startup got stuck.
class BootLoading extends StatelessWidget {
  const BootLoading({super.key, required this.log});

  final BootLog log;

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: Colors.black,
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Column(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              const CircularProgressIndicator(),
              const SizedBox(height: 24),
              const Text('Starting Continuum…',
                  style: TextStyle(fontSize: 16)),
              const SizedBox(height: 16),
              Expanded(
                child: ListenableBuilder(
                  listenable: log,
                  builder: (context, _) => ListView(
                    children: [
                      for (final line in log.lines)
                        Text(line,
                            style: TextStyle(
                                fontSize: 12,
                                color: Colors.white.withValues(alpha: 0.6),
                                fontFamily: 'monospace')),
                    ],
                  ),
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// Startup error screen: short message + first lines of the stack trace.
/// The full text goes to logcat (`adb logcat | findstr boot`).
class BootError extends StatelessWidget {
  const BootError({
    super.key,
    required this.error,
    required this.stack,
    required this.onRetry,
  });

  final Object error;
  final StackTrace? stack;
  final VoidCallback onRetry;

  /// First lines of the stack trace: the framework adds ~100 identical frames,
  /// a dozen is enough for diagnostics.
  String get _shortStack {
    final lines = stack.toString().split('\n');
    const head = 12;
    final shown = lines.take(head).join('\n');
    return lines.length > head
        ? '$shown\n… (${lines.length - head} lines hidden)'
        : shown;
  }

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: Colors.black,
      body: SafeArea(
        child: Padding(
          padding: const EdgeInsets.all(24),
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              const Icon(Icons.error_outline, size: 48, color: Colors.red),
              const SizedBox(height: 16),
              const Text('Failed to start',
                  textAlign: TextAlign.center,
                  style: TextStyle(fontSize: 18, fontWeight: FontWeight.w600)),
              const SizedBox(height: 12),
              Expanded(
                child: SingleChildScrollView(
                  child: Text(
                    '$error\n$_shortStack',
                    style: TextStyle(
                        fontSize: 12,
                        color: Colors.white.withValues(alpha: 0.7),
                        fontFamily: 'monospace'),
                  ),
                ),
              ),
              const SizedBox(height: 12),
              FilledButton.icon(
                onPressed: onRetry,
                icon: const Icon(Icons.refresh),
                label: const Text('Retry'),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

class ContinuumApp extends StatelessWidget {
  const ContinuumApp({
    super.key,
    required this.controller,
    required this.bridge,
  });

  final PlayerController controller;
  final AudioBridge bridge;

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      title: 'Continuum',
      debugShowCheckedModeBanner: false,
      theme: ThemeData.dark(useMaterial3: true),
      home: PlayerScreen(controller: controller, bridge: bridge),
    );
  }
}
