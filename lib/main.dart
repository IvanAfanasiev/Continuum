import 'dart:async';

import 'package:flutter/material.dart';

import 'package:cont/src/audio_bridge.dart';
import 'package:cont/src/player_controller.dart';
import 'package:cont/src/player_screen.dart';
import 'package:cont/src/rust/frb_generated.dart';

/// Этапы запуска с живым логом: если на устройстве виснет или падает,
/// видно ГДЕ (а не черный экран). Лог дублируется в logcat.
class BootLog extends ChangeNotifier {
  final lines = <String>[];

  void add(String line) {
    final stamped = line;
    lines.add(stamped);
    debugPrint('[boot] $stamped');
    notifyListeners();
  }
}

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  FlutterError.onError = (details) {
    debugPrint('[flutter-error] ${details.exceptionAsString()}');
    FlutterError.presentError(details);
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
    setState(() {
      _ready = null;
      _error = null;
      _stack = null;
    });
    try {
      _log.add('RustLib.init...');
      await RustLib.init();
      _log.add('rust ok');
      final controller = PlayerController();
      final bridge = AudioBridge(controller);
      _log.add('audio service...');
      await bridge.init();
      _log.add('audio ok');
      _log.add('controller init...');
      await controller.init();
      _log.add('ready');
      // Прогрев артов уведомления в фоне: первый тап play/стиль
      // не будет ждать декод картинки.
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

/// Экран загрузки с живым логом: видно, на чем встал запуск.
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
              const Text('Continuum запускается…',
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

/// Экран ошибки старта: короткое сообщение + первые строки стека.
/// Полный текст уходит в logcat (`adb logcat | findstr boot`).
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

  /// Первые строки стека: фреймворк добавляет ~100 одинаковых кадров,
  /// для диагностики достаточно десятка.
  String get _shortStack {
    final lines = stack.toString().split('\n');
    const head = 12;
    final shown = lines.take(head).join('\n');
    return lines.length > head ? '$shown\n… (${lines.length - head} строк скрыто)' : shown;
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
              const Text('Не удалось запуститься',
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
                label: const Text('Повторить'),
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
