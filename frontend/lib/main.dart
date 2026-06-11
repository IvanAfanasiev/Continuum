import 'dart:async';

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'backend_player.dart';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  SystemChrome.setSystemUIOverlayStyle(
    const SystemUiOverlayStyle(
      statusBarColor: Colors.transparent,
      statusBarIconBrightness: Brightness.light,
      systemNavigationBarColor: Color(0xff0d0f12),
      systemNavigationBarIconBrightness: Brightness.light,
    ),
  );
  runApp(const ContinuumApp());
}

class ContinuumApp extends StatelessWidget {
  const ContinuumApp({super.key});

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      debugShowCheckedModeBanner: false,
      title: 'Continuum',
      theme: ThemeData(
        useMaterial3: true,
        brightness: Brightness.dark,
        colorScheme: ColorScheme.fromSeed(
          seedColor: const Color(0xff6ed6c5),
          brightness: Brightness.dark,
          primary: const Color(0xff6ed6c5),
          secondary: const Color(0xffffc86b),
        ).copyWith(surface: const Color(0xff17191d)),
        scaffoldBackgroundColor: const Color(0xff0d0f12),
        sliderTheme: const SliderThemeData(
          trackHeight: 3,
          thumbShape: RoundSliderThumbShape(enabledThumbRadius: 8),
        ),
      ),
      home: const ContinuumHome(),
    );
  }
}

class ContinuumHome extends StatefulWidget {
  const ContinuumHome({super.key});

  @override
  State<ContinuumHome> createState() => _ContinuumHomeState();
}

class _ContinuumHomeState extends State<ContinuumHome> {
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
  String? _pendingPresetName;
  String? _localPresetIntentName;
  String? _committedPresetName;
  DateTime? _lastPresetIntentAt;

  PresetState get _preset => _presets[_presetIndex];

  @override
  void initState() {
    super.initState();
    _player.setPlaybackListener(_handlePlaybackSnapshot);
  }

  @override
  void dispose() {
    _presetCommitTimer?.cancel();
    _player.setPlaybackListener(null);
    unawaited(_player.dispose());
    super.dispose();
  }

  void _handlePlaybackSnapshot(PlaybackSnapshot snapshot) {
    if (!mounted) {
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
    }

    setState(() {
      if (presetName != null) {
        final nextIndex = _presets.indexWhere(
          (preset) => preset.name.toLowerCase() == presetName.toLowerCase(),
        );
        if (nextIndex >= 0 && nextIndex != _presetIndex) {
          _presetIndex = nextIndex;
          changedPreset = true;
        }
      }
    });

    if (presetName != null &&
        localIntent != null &&
        _samePresetName(presetName, localIntent) &&
        _pendingPresetName == null &&
        !_presetCommitRunning) {
      _localPresetIntentName = null;
    }

    if (snapshot.isPlaying && changedPreset) {
      _sendControls(_preset);
    }
  }

  Future<void> _togglePlayback() async {
    _cancelPendingPresetCommit();
    _localPresetIntentName = _preset.name;

    if (_player.isPlaying) {
      await _player.pause();
      await _player.selectPreset(_preset.name);
      _committedPresetName = _preset.name;
    } else {
      await _player.play(_preset.name);
      _committedPresetName = _preset.name;
      _sendControls(_preset);
    }

    _localPresetIntentName = null;
    if (mounted) {
      setState(() {});
    }
  }

  Future<void> _switchPreset(int direction) {
    setState(() {
      _presetIndex = (_presetIndex + direction) % _presets.length;
      if (_presetIndex < 0) {
        _presetIndex = _presets.length - 1;
      }
    });

    _queuePresetCommit(_preset.name);
    return Future.value();
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

  Future<void> _commitPresetSelection(String presetName) async {
    try {
      if (_committedPresetName != null &&
          _samePresetName(_committedPresetName!, presetName)) {
        if (_samePresetName(_preset.name, presetName)) {
          _sendControls(_preset);
          _localPresetIntentName = null;
        }
        if (mounted) {
          setState(() {});
        }
        return;
      }

      await _player.selectPreset(presetName);
      if (!mounted) {
        return;
      }

      _committedPresetName = presetName;
      if (_samePresetName(_preset.name, presetName)) {
        _sendControls(_preset);
        _localPresetIntentName = null;
      }

      setState(() {});
    } catch (_) {
      if (mounted) {
        setState(() {});
      }
    } finally {
      _presetCommitRunning = false;
      if (mounted && _pendingPresetName != null) {
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

  bool _samePresetName(String first, String second) {
    return first.toLowerCase() == second.toLowerCase();
  }

  void _updatePreset(PresetState next) {
    setState(() {
      _presets[_presetIndex] = next;
    });
    _sendControls(next);
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

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      body: Stack(
        children: [
          Positioned.fill(
            child: _Backdrop(
              color: _preset.color,
              asset: _preset.backgroundAsset,
            ),
          ),
          SafeArea(
            child: Padding(
              padding: const EdgeInsets.symmetric(horizontal: 18, vertical: 14),
              child: Column(
                children: [
                  const _TopBar(),
                  Expanded(
                    child: Stack(
                      children: [
                        Positioned.fill(child: _PresetDeck(preset: _preset)),
                        Positioned.fill(
                          child: _InteractionLayer(
                            preset: _preset,
                            isPlaying: _player.isPlaying,
                            onPlayPressed: _togglePlayback,
                            onPreviousPressed: () => _switchPreset(-1),
                            onNextPressed: () => _switchPreset(1),
                          ),
                        ),
                      ],
                    ),
                  ),
                  const SizedBox(height: 88),
                ],
              ),
            ),
          ),
          DraggableScrollableSheet(
            initialChildSize: 0.16,
            minChildSize: 0.14,
            maxChildSize: 0.70,
            snap: true,
            snapSizes: const [0.16, 0.70],
            builder: (context, scrollController) {
              return ControlSheet(
                preset: _preset,
                controller: scrollController,
                onChanged: _updatePreset,
              );
            },
          ),
        ],
      ),
    );
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

class _Backdrop extends StatelessWidget {
  const _Backdrop({required this.color, required this.asset});

  final Color color;
  final String? asset;

  @override
  Widget build(BuildContext context) {
    return Stack(
      fit: StackFit.expand,
      children: [
        const ColoredBox(color: Color(0xff0d0f12)),
        if (asset != null)
          Image.asset(
            asset!,
            fit: BoxFit.cover,
            errorBuilder: (context, error, stackTrace) {
              return const SizedBox.shrink();
            },
          ),
        DecoratedBox(
          decoration: BoxDecoration(
            gradient: LinearGradient(
              begin: Alignment.topLeft,
              end: Alignment.bottomRight,
              colors: [
                Color.alphaBlend(
                  color.withOpacity(asset == null ? 0.14 : 0.22),
                  const Color(0xff0d0f12).withOpacity(asset == null ? 1.0 : 0.70),
                ),
                const Color(0xff111318).withOpacity(asset == null ? 1.0 : 0.78),
                Color.alphaBlend(
                  const Color(0xffffc86b).withOpacity(0.08),
                  const Color(0xff0d0f12).withOpacity(asset == null ? 1.0 : 0.76),
                ),
              ],
            ),
          ),
        ),
      ],
    );
  }
}

class _TopBar extends StatelessWidget {
  const _TopBar();

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Row(
      children: [
        _ContinuumMark(color: theme.colorScheme.primary),
        const SizedBox(width: 10),
        Text(
          'Continuum',
          style: theme.textTheme.titleMedium?.copyWith(
            fontWeight: FontWeight.w700,
          ),
        ),
        const Spacer(),
      ],
    );
  }
}

class _PresetDeck extends StatelessWidget {
  const _PresetDeck({required this.preset});

  final PresetState preset;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Stack(
      alignment: Alignment.center,
      children: [
        Align(
          alignment: Alignment.topCenter,
          child: Padding(
            padding: const EdgeInsets.only(top: 34),
            child: Text(
              preset.name,
              maxLines: 1,
              overflow: TextOverflow.ellipsis,
              style: theme.textTheme.displayMedium?.copyWith(
                fontWeight: FontWeight.w800,
                letterSpacing: 0,
              ),
            ),
          ),
        ),
        Align(
          alignment: const Alignment(0, 0.58),
          child: Padding(
            padding: const EdgeInsets.symmetric(horizontal: 12),
            child: Wrap(
              alignment: WrapAlignment.center,
              spacing: 8,
              runSpacing: 8,
              children: [
                for (final instrument in preset.instruments)
                  _InstrumentChip(label: instrument, color: preset.color),
              ],
            ),
          ),
        ),
      ],
    );
  }
}

class _InteractionLayer extends StatelessWidget {
  const _InteractionLayer({
    required this.preset,
    required this.isPlaying,
    required this.onPlayPressed,
    required this.onPreviousPressed,
    required this.onNextPressed,
  });

  final PresetState preset;
  final bool isPlaying;
  final Future<void> Function() onPlayPressed;
  final Future<void> Function() onPreviousPressed;
  final Future<void> Function() onNextPressed;

  @override
  Widget build(BuildContext context) {
    return Row(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Expanded(
          flex: 24,
          child: _TransparentAction(
            icon: Icons.chevron_left,
            iconSize: 42,
            tooltip: 'Previous style',
            color: Colors.white70,
            onPressed: onPreviousPressed,
          ),
        ),
        Expanded(
          flex: 52,
          child: _TransparentAction(
            icon: isPlaying ? Icons.pause : Icons.play_arrow,
            iconSize: 68,
            tooltip: isPlaying ? 'Pause' : 'Play',
            color: preset.color,
            onPressed: onPlayPressed,
          ),
        ),
        Expanded(
          flex: 24,
          child: _TransparentAction(
            icon: Icons.chevron_right,
            iconSize: 42,
            tooltip: 'Next style',
            color: Colors.white70,
            onPressed: onNextPressed,
          ),
        ),
      ],
    );
  }
}

class _TransparentAction extends StatelessWidget {
  const _TransparentAction({
    required this.icon,
    required this.iconSize,
    required this.tooltip,
    required this.color,
    required this.onPressed,
  });

  final IconData icon;
  final double iconSize;
  final String tooltip;
  final Color color;
  final Future<void> Function() onPressed;

  @override
  Widget build(BuildContext context) {
    return Tooltip(
      message: tooltip,
      child: Material(
        color: Colors.transparent,
        child: InkWell(
          splashColor: color.withOpacity(0.08),
          highlightColor: color.withOpacity(0.04),
          onTap: () => unawaited(onPressed()),
          child: Center(
            child: Icon(icon, size: iconSize, color: color.withOpacity(0.92)),
          ),
        ),
      ),
    );
  }
}

class _InstrumentChip extends StatelessWidget {
  const _InstrumentChip({required this.label, required this.color});

  final String label;
  final Color color;

  @override
  Widget build(BuildContext context) {
    return DecoratedBox(
      decoration: BoxDecoration(
        color: Color.alphaBlend(
          color.withOpacity(0.16),
          const Color(0xff17191d),
        ),
        border: Border.all(color: color.withOpacity(0.32)),
        borderRadius: BorderRadius.circular(8),
      ),
      child: Padding(
        padding: const EdgeInsets.symmetric(horizontal: 10, vertical: 6),
        child: Text(label, style: Theme.of(context).textTheme.labelMedium),
      ),
    );
  }
}

class ControlSheet extends StatelessWidget {
  const ControlSheet({
    super.key,
    required this.preset,
    required this.controller,
    required this.onChanged,
  });

  final PresetState preset;
  final ScrollController controller;
  final ValueChanged<PresetState> onChanged;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    final bottomInset = MediaQuery.viewPaddingOf(context).bottom;
    return DecoratedBox(
      decoration: BoxDecoration(
        color: const Color(0xff17191d),
        borderRadius: const BorderRadius.vertical(top: Radius.circular(8)),
        border: Border(top: BorderSide(color: Colors.white.withOpacity(0.08))),
        boxShadow: const [
          BoxShadow(
            color: Colors.black54,
            blurRadius: 28,
            offset: Offset(0, -8),
          ),
        ],
      ),
      child: ListView(
        controller: controller,
        padding: EdgeInsets.fromLTRB(18, 10, 18, 28 + bottomInset),
        children: [
          Center(
            child: Container(
              width: 46,
              height: 4,
              decoration: BoxDecoration(
                color: Colors.white38,
                borderRadius: BorderRadius.circular(2),
              ),
            ),
          ),
          const SizedBox(height: 16),
          Row(
            children: [
              Icon(Icons.tune, color: theme.colorScheme.primary),
              const SizedBox(width: 10),
              Text(
                'Settings',
                style: theme.textTheme.titleMedium?.copyWith(
                  fontWeight: FontWeight.w700,
                ),
              ),
              const Spacer(),
              Text(
                preset.name,
                style: theme.textTheme.labelLarge?.copyWith(
                  color: Colors.white60,
                ),
              ),
            ],
          ),
          const SizedBox(height: 18),
          _ControlSlider(
            icon: Icons.speed,
            label: 'Tempo',
            value: preset.tempo,
            min: 0.65,
            max: 1.35,
            defaultValue: 1.0,
            valueLabel: '${(preset.tempo * 100).round()}%',
            onChanged: (value) => onChanged(preset.copyWith(tempo: value)),
          ),
          _ControlSlider(
            icon: Icons.shuffle,
            label: 'Swing',
            value: preset.swing,
            min: 0.0,
            max: 1.0,
            defaultValue: 0.5,
            valueLabel: '${(preset.swing * 100).round()}%',
            onChanged: (value) => onChanged(preset.copyWith(swing: value)),
          ),
          const SizedBox(height: 10),
          Text(
            'Instruments',
            style: theme.textTheme.titleSmall?.copyWith(color: Colors.white70),
          ),
          const SizedBox(height: 8),
          for (final instrument in preset.instruments)
            _ControlSlider(
              icon: _instrumentIcon(instrument),
              label: instrument,
              value: preset.volumes[instrument] ?? 0.5,
              min: 0.0,
              max: 1.0,
              defaultValue: 0.5,
              valueLabel:
                  '${((preset.volumes[instrument] ?? 0.5) * 100).round()}%',
              onChanged: (value) {
                final volumes = Map<String, double>.from(preset.volumes);
                volumes[instrument] = value;
                onChanged(preset.copyWith(volumes: volumes));
              },
            ),
        ],
      ),
    );
  }
}

class _ControlSlider extends StatelessWidget {
  const _ControlSlider({
    required this.icon,
    required this.label,
    required this.value,
    required this.min,
    required this.max,
    required this.defaultValue,
    required this.valueLabel,
    required this.onChanged,
  });

  final IconData icon;
  final String label;
  final double value;
  final double min;
  final double max;
  final double defaultValue;
  final String valueLabel;
  final ValueChanged<double> onChanged;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Padding(
      padding: const EdgeInsets.symmetric(vertical: 6),
      child: Row(
        children: [
          Icon(icon, size: 22, color: Colors.white70),
          const SizedBox(width: 12),
          SizedBox(
            width: 92,
            child: Text(label, maxLines: 1, overflow: TextOverflow.ellipsis),
          ),
          Expanded(
            child: Stack(
              alignment: Alignment.center,
              children: [
                Positioned.fill(
                  child: CustomPaint(
                    painter: _SliderDefaultPainter(
                      percent: ((defaultValue - min) / (max - min))
                          .clamp(0.0, 1.0)
                          .toDouble(),
                    ),
                  ),
                ),
                Slider(
                  value: value.clamp(min, max).toDouble(),
                  min: min,
                  max: max,
                  onChanged: onChanged,
                ),
              ],
            ),
          ),
          SizedBox(
            width: 48,
            child: Text(
              valueLabel,
              textAlign: TextAlign.end,
              style: theme.textTheme.labelMedium?.copyWith(
                color: Colors.white60,
              ),
            ),
          ),
        ],
      ),
    );
  }
}

class _SliderDefaultPainter extends CustomPainter {
  const _SliderDefaultPainter({required this.percent});

  final double percent;

  @override
  void paint(Canvas canvas, Size size) {
    final x = size.width * percent;
    final centerY = size.height * 0.5;
    final paint = Paint()
      ..color = Colors.white.withOpacity(0.34)
      ..strokeWidth = 2
      ..strokeCap = StrokeCap.round;
    canvas.drawLine(
      Offset(x, centerY - 11),
      Offset(x, centerY + 11),
      paint,
    );
  }

  @override
  bool shouldRepaint(covariant _SliderDefaultPainter oldDelegate) {
    return oldDelegate.percent != percent;
  }
}

class _ContinuumMark extends StatelessWidget {
  const _ContinuumMark({required this.color, this.size = 28});

  final Color color;
  final double size;

  @override
  Widget build(BuildContext context) {
    return CustomPaint(
      size: Size.square(size),
      painter: _ContinuumMarkPainter(color: color),
    );
  }
}

class _ContinuumMarkPainter extends CustomPainter {
  const _ContinuumMarkPainter({required this.color});

  final Color color;

  @override
  void paint(Canvas canvas, Size size) {
    final diamond = Path()
      ..moveTo(size.width * 0.50, size.height * 0.04)
      ..lineTo(size.width * 0.96, size.height * 0.50)
      ..lineTo(size.width * 0.50, size.height * 0.96)
      ..lineTo(size.width * 0.04, size.height * 0.50)
      ..close();

    canvas.drawPath(
      diamond,
      Paint()..color = const Color(0xff17191d).withOpacity(0.94),
    );
    canvas.drawPath(
      diamond,
      Paint()
        ..color = color.withOpacity(0.42)
        ..style = PaintingStyle.stroke
        ..strokeWidth = 1.4,
    );

    canvas.save();
    canvas.clipPath(diamond);
    final barPaint = Paint()
      ..color = color
      ..strokeCap = StrokeCap.round
      ..strokeWidth = size.width * 0.105;
    final accentPaint = Paint()
      ..color = const Color(0xffffc86b)
      ..strokeCap = StrokeCap.round
      ..strokeWidth = size.width * 0.105;

    for (var index = 0; index < 5; index++) {
      final x = size.width * (0.26 + index * 0.12);
      final top = size.height * (0.70 - index * 0.09);
      final bottom = size.height * 0.78;
      canvas.drawLine(Offset(x, top), Offset(x, bottom), barPaint);
      canvas.drawLine(
        Offset(x, top - size.height * 0.12),
        Offset(x, top - size.height * 0.05),
        index.isEven ? accentPaint : barPaint,
      );
    }
    canvas.restore();
  }

  @override
  bool shouldRepaint(covariant _ContinuumMarkPainter oldDelegate) {
    return oldDelegate.color != color;
  }
}

IconData _instrumentIcon(String instrument) {
  return switch (instrument) {
    'Piano' => Icons.music_note,
    'Bass' => Icons.album,
    'Kick' => Icons.circle,
    'Ride' => Icons.radio_button_checked,
    'Hihat' => Icons.adjust,
    'Pad' => Icons.waves,
    'Triangle' => Icons.change_history,
    _ => Icons.music_note,
  };
}
