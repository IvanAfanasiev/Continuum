import 'dart:async';

import 'package:flutter/material.dart';

import 'package:continuum/src/audio_bridge.dart';
import 'package:continuum/src/player_controller.dart';
import 'package:continuum/src/rust/playback.dart';
import 'package:continuum/src/seed_input.dart';

/// Tracks arranged by group shelves. Shelf order comes from the backend
/// (`groups` are in canonical order), with slot order within each shelf.
/// Empty shelves are skipped. No frontend-side hardcoded lists.
List<(String, List<TrackInfo>)> _groupedTracks(PlayerController c) {
  final out = <(String, List<TrackInfo>)>[];
  for (final g in c.groups) {
    final items = c.tracks.where((t) => t.group == g.name).toList();
    if (items.isNotEmpty) out.add((g.name, items));
  }
  final known = c.groups.map((g) => g.name).toSet();
  final rest = c.tracks.where((t) => !known.contains(t.group)).toList();
  if (rest.isNotEmpty) out.add(('', rest));
  return out;
}

/// Group labels and icons for end users (without TODO-style words).
const _groupMeta = {
  'bass': (Icons.speaker, 'Bass'),
  'kick': (Icons.album, 'Kick'),
  'snare': (Icons.circle, 'Snare'),
  'hat': (Icons.donut_small, 'Hat'),
  'chords': (Icons.piano, 'Chords'),
  'lead': (Icons.music_note, 'Lead'),
  'texture': (Icons.grain, 'Atmosphere'),
};

/// Artwork paths. Files live in assets/images/ (see the README there):
/// background `bg_<style>.png`, preview `style_<style>.png`.
/// No file — black background (errorBuilder), the app doesn't crash.
class AppArtwork {
  static String background(String style) =>
      'assets/images/bg_${style.toLowerCase()}.png';

  static String preview(String style) =>
      'assets/images/style_${style.toLowerCase()}.png';

  static const String appIcon = 'assets/images/app_icon.png';
}

/// Screen background: full-screen style image + a dark veil for
/// white-text readability. Without a file — just black.
class StyleBackground extends StatelessWidget {
  const StyleBackground({super.key, required this.style});

  final String style;

  @override
  Widget build(BuildContext context) {
    return Stack(
      fit: StackFit.expand,
      children: [
        Container(color: Colors.black),
        Image.asset(
          AppArtwork.background(style),
          fit: BoxFit.cover,
          errorBuilder: (_, _, _) => const SizedBox.shrink(),
        ),
        Container(color: Colors.black.withValues(alpha: 0.45)),
      ],
    );
  }
}

/// 48x48 mini style preview. Without a file — a black square.
class StylePreview extends StatelessWidget {
  const StylePreview({super.key, required this.style});

  final String style;

  @override
  Widget build(BuildContext context) {
    return ClipRRect(
      borderRadius: BorderRadius.circular(8),
      child: SizedBox(
        width: 48,
        height: 48,
        child: Image.asset(
          AppArtwork.preview(style),
          fit: BoxFit.cover,
          errorBuilder: (_, _, _) => Container(color: Colors.black),
        ),
      ),
    );
  }
}

/// Human-readable slot names for the instruments section.
const _slotMeta = {
  'bass': 'Bass',
  'kick': 'Kick',
  'pad': 'Pad',
  'snare': 'Snare',
  'hat': 'Hat',
  'pluck': 'Pluck',
  'keys': 'Keys',
  'kick808': '808',
  'vinyl': 'Vinyl',
  'guitar': 'Guitar',
  'shaker': 'Shaker',
  'piano': 'Piano',
};

String _styleTitle(String style) {
  if (style.isEmpty) return 'Club';
  switch (style) {
    case 'club':
      return 'Club';
    case 'lofi':
      return 'Lo-Fi';
    default:
      return style[0].toUpperCase() + style.substring(1);
  }
}

/// Audio-path diagnostics for the banner (null — everything is growing as expected).
/// Silence in the first seconds after start is normal (the device is opening).
String? _statsDiag(PlayerController c) {
  if (!(c.running && !c.paused)) return null;
  if ((c.status?.engineError ?? '').isNotEmpty) return null;
  final stats = c.stats;
  final started = c.startedAt;
  if (stats == null || started == null) return null;
  if (DateTime.now().difference(started).inSeconds < 6) return null;
  if (stats.frames == BigInt.zero) {
    final cfg =
        stats.audioConfig.isEmpty ? 'unknown output' : stats.audioConfig;
    return 'Audio stream did not start ($cfg). '
        'Notes generated: ${stats.pushed}. '
        'Check media volume and Bluetooth.';
  }
  if (stats.pushed == BigInt.zero) {
    return 'The generator is not producing notes. Please report this.';
  }
  if (stats.consumed == BigInt.zero) {
    return 'Notes are not reaching the engine. Please report this.';
  }
  if (c.streamStalled) {
    final n = stats.restarts;
    return 'Audio stream stopped, reconnecting'
        '${n == BigInt.zero ? '' : ' (attempt ${n.toString()})'}. '
        'If it hangs, switch output (headphones/speaker).';
  }
  if (stats.dropped > BigInt.zero) {
    return 'Rhythm gaps: ${stats.dropped} notes dropped under load. '
        'Lower the energy or close background tasks.';
  }
  if (stats.reanchors > BigInt.from(3)) {
    return 'Generator is lagging: ${stats.reanchors} grid re-anchors. '
        'Try turning off some instruments in Mix.';
  }
  return null;
}

/// Notice banner under the seed: engine error or audio-path diagnostics.
class _NoticeBanner extends StatelessWidget {
  const _NoticeBanner({
    required this.icon,
    required this.color,
    required this.text,
  });

  final IconData icon;
  final Color color;
  final String text;

  @override
  Widget build(BuildContext context) {
    return Container(
      margin: const EdgeInsets.only(top: 12),
      padding: const EdgeInsets.symmetric(horizontal: 12, vertical: 8),
      decoration: BoxDecoration(
        color: color.withValues(alpha: 0.15),
        borderRadius: BorderRadius.circular(8),
      ),
      child: Row(
        mainAxisSize: MainAxisSize.min,
        children: [
          Icon(icon, size: 16, color: color),
          const SizedBox(width: 6),
          Flexible(
            child: Text(
              text,
              style: const TextStyle(fontSize: 12),
              maxLines: 3,
              overflow: TextOverflow.ellipsis,
            ),
          ),
        ],
      ),
    );
  }
}

class PlayerScreen extends StatefulWidget {
  const PlayerScreen({super.key, required this.controller, required this.bridge});

  final PlayerController controller;
  final AudioBridge bridge;

  @override
  State<PlayerScreen> createState() => _PlayerScreenState();
}

class _PlayerScreenState extends State<PlayerScreen> {
  final _scaffoldKey = GlobalKey<ScaffoldState>();
  Timer? _poller;

  @override
  void initState() {
    super.initState();
    widget.controller.addListener(_onController);
    // Light status polling while playing: the frontend sees backend truth.
    _poller = Timer.periodic(const Duration(seconds: 3), (_) {
      if (widget.controller.running) widget.controller.refresh();
    });
  }

  void _onController() {
    if (mounted) setState(() {});
  }

  @override
  void dispose() {
    _poller?.cancel();
    widget.controller.removeListener(_onController);
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final c = widget.controller;
    // Landscape: hide the buttons, slide the panels out immediately; keep
    // the center narrow — the portrait style is preserved.
    final landscape =
        MediaQuery.of(context).size.aspectRatio > 1.0;
    // Background: style artwork.
    if (landscape) {
      return Scaffold(
        backgroundColor: Colors.black,
        body: Stack(
          children: [
            StyleBackground(style: c.styleName),
            SafeArea(
              child: Row(
                children: [
                  _SidePanel(child: _MixPanelBody(controller: c)),
                  Expanded(
                    child: Center(
                      child: ConstrainedBox(
                        constraints: const BoxConstraints(maxWidth: 520),
                        child: _CenterColumn(
                          controller: c,
                          bridge: widget.bridge,
                          showSideButtons: false,
                        ),
                      ),
                    ),
                  ),
                  _SidePanel(
                    child: _StylesPanelBody(
                        controller: c, bridge: widget.bridge),
                  ),
                ],
              ),
            ),
          ],
        ),
      );
    }
    return Scaffold(
      key: _scaffoldKey,
      backgroundColor: Colors.black,
      // Wide edge-swipe area: the menu opens by gesture,
      // not only by buttons (the default 20pt is hard to hit).
      drawerEdgeDragWidth: 72,
      // Clear focus on both opening AND closing the menu: otherwise FocusScope
      // returns focus to the seed box when the drawer closes (+keyboard).
      // Can't disable focus entirely — then the seed can't be typed,
      // so clear it aggressively everywhere except the input itself.
      onDrawerChanged: (_) {
        FocusManager.instance.primaryFocus?.unfocus();
        WidgetsBinding.instance.addPostFrameCallback((_) {
          FocusManager.instance.primaryFocus?.unfocus();
        });
      },
      drawer: Drawer(child: _MixPanelBody(controller: c)),
      endDrawer:
          Drawer(child: _StylesPanelBody(controller: c, bridge: widget.bridge)),
      body: Stack(
        children: [
          StyleBackground(style: c.styleName),
          SafeArea(
            child: _CenterColumn(
              controller: c,
              bridge: widget.bridge,
              showSideButtons: true,
              onOpenMix: () => _scaffoldKey.currentState?.openDrawer(),
              onOpenStyles: () => _scaffoldKey.currentState?.openEndDrawer(),
            ),
          ),
        ],
      ),
    );
  }
}

/// Landscape side panel: fixed 300pt, semi-transparent
/// over the shared background. This keeps the center a constant size.
class _SidePanel extends StatelessWidget {
  const _SidePanel({required this.child});

  final Widget child;

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      width: 300,
      child: Container(
        color: Colors.black.withValues(alpha: 0.45),
        child: child,
      ),
    );
  }
}

/// Center: style name, seed, centered play (+ side buttons
/// in portrait only).
class _CenterColumn extends StatelessWidget {
  const _CenterColumn({
    required this.controller,
    required this.bridge,
    required this.showSideButtons,
    this.onOpenMix,
    this.onOpenStyles,
  });

  final PlayerController controller;
  final AudioBridge bridge;
  final bool showSideButtons;
  final VoidCallback? onOpenMix;
  final VoidCallback? onOpenStyles;

  /// Double-tap guard: while an FFI call is in flight, the second is ignored
  /// (otherwise play/pause arrive in completion order, not press order).
  static bool _toggleInFlight = false;

  Future<void> _toggle(BuildContext context) async {
    if (_toggleInFlight) return;
    _toggleInFlight = true;
    FocusManager.instance.primaryFocus?.unfocus();
    try {
      final c = controller;
      if (c.running && !c.paused) {
        await bridge.pause();
      } else if (!await bridge.play()) {
        // The track wasn't created (no device): explain instead of staying silent.
        if (context.mounted) {
          ScaffoldMessenger.of(context).showSnackBar(
            const SnackBar(
              content: Text('Could not start audio: no output device'),
            ),
          );
        }
      }
    } catch (e) {
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          SnackBar(content: Text('Playback error: $e')),
        );
      }
    } finally {
      _toggleInFlight = false;
    }
  }

  Future<void> _seed(String code) async {
    try {
      await controller.submitSeedCode(code);
      await bridge.sync();
    } catch (e) {
      debugPrint('[seed] $e');
    }
  }

  Future<void> _dice(BuildContext context) async {
    FocusManager.instance.primaryFocus?.unfocus();
    try {
      await controller.randomSeed();
      await bridge.sync();
    } catch (e) {
      debugPrint('[seed] $e');
    }
  }

  Future<void> _previousSeed() async {
    FocusManager.instance.primaryFocus?.unfocus();
    try {
      await controller.previousSeed();
      await bridge.sync();
    } catch (e) {
      debugPrint('[seed] $e');
    }
  }

  @override
  Widget build(BuildContext context) {
    final c = controller;
    final playing = c.running && !c.paused;
    return Column(
      children: [
        // Center: style name and seed.
        Expanded(
          child: Column(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              Text(
                _styleTitle(c.styleName),
                style: const TextStyle(
                  fontSize: 40,
                  fontWeight: FontWeight.w700,
                  color: Colors.white,
                ),
              ),
              const SizedBox(height: 16),
              SeedInput(
                initialCode: c.boxCode,
                onSubmit: _seed,
                onDice: () => _dice(context),
                onPrevious: c.canGoBackSeed ? _previousSeed : null,
                compact: MediaQuery.of(context).size.width < 600,
              ),
              // Audio engine error: previously it was silence without explanation.
              if (c.status?.engineError case final err? when err.isNotEmpty)
                _NoticeBanner(
                  icon: Icons.warning_amber,
                  color: Colors.redAccent,
                  text: err,
                ),
              // Audio-path diagnostics: stream won't start / generator is silent.
              if (_statsDiag(c) case final diag? when diag.isNotEmpty)
                _NoticeBanner(
                  icon: Icons.hearing,
                  color: Colors.amber,
                  text: diag,
                ),
            ],
          ),
        ),
        // Lower half: centered play.
        Expanded(
          child: Row(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              if (showSideButtons) ...[
                IconButton(
                  tooltip: 'Mix and tempo',
                  iconSize: 32,
                  color: Colors.white,
                  onPressed: onOpenMix,
                  icon: const Icon(Icons.tune),
                ),
                const SizedBox(width: 32),
              ],
              Semantics(
                button: true,
                label: playing ? 'Pause' : 'Play',
                child: GestureDetector(
                  onTap: () => _toggle(context),
                  child: Container(
                    width: 96,
                    height: 96,
                    decoration: const BoxDecoration(
                      shape: BoxShape.circle,
                      color: Colors.white,
                    ),
                    child: Icon(
                      playing ? Icons.pause : Icons.play_arrow,
                      size: 56,
                      color: Colors.black,
                    ),
                  ),
                ),
              ),
              if (showSideButtons) ...[
                const SizedBox(width: 32),
                IconButton(
                  tooltip: 'Styles',
                  iconSize: 32,
                  color: Colors.white,
                  onPressed: onOpenStyles,
                  icon: const Icon(Icons.queue_music),
                ),
              ],
            ],
          ),
        ),
      ],
    );
  }
}


/// Left panel: tempo, energy, master sound and instrument levels.
/// Used both as a drawer (portrait) and as a side panel (landscape).
class _MixPanelBody extends StatelessWidget {
  const _MixPanelBody({required this.controller});

  final PlayerController controller;

  @override
  Widget build(BuildContext context) {
    final c = controller;
    return SafeArea(
      child: ListView(
        padding: const EdgeInsets.all(16),
        children: [
          const Text('Mix',
              style: TextStyle(fontSize: 20, fontWeight: FontWeight.w600)),
          const SizedBox(height: 8),
          _VolumeRow(
            icon: Icons.speed,
            tooltip: 'Tempo',
            value: c.status?.tempoBpm ?? 126,
            min: 50,
            max: 180,
            integer: true,
            suffix: ' BPM',
            onCommit: (v) => c.setTempo(v),
            onReset: c.resetTempo,
          ),
          _VolumeRow(
            icon: Icons.bolt,
            tooltip: 'Energy',
            value: c.status?.intensity ?? 1.0,
            max: 2.0,
            percent: true,
            onCommit: (v) => c.setIntensity(v),
            onReset: c.resetIntensity,
          ),
          const Divider(height: 24),
          Row(
            children: [
              const Expanded(
                child: Text('Sound',
                    style:
                        TextStyle(fontSize: 16, fontWeight: FontWeight.w600)),
              ),
              IconButton(
                onPressed: c.resetMasterFx,
                icon: const Icon(Icons.restart_alt),
                tooltip: 'Restore style sound',
                visualDensity: VisualDensity.compact,
              ),
            ],
          ),
          _VolumeRow(
            icon: Icons.blur_on,
            tooltip: 'Echo (reverb amount)',
            value: c.status?.reverbWet ?? 0.16,
            max: 0.6,
            percent: true,
            onCommit: (v) => c.setMasterFx(
              reverbWet: v,
              reverbSize: c.status?.reverbSize ?? 0.5,
              trebleHz: c.status?.trebleHz ?? 19000,
              sidechain: c.status?.sidechain ?? 1.0,
            ),
          ),
          _VolumeRow(
            icon: Icons.highlight,
            tooltip: 'Treble cutoff',
            value: (c.status?.trebleHz ?? 19000) / 1000.0,
            min: 2.0,
            max: 20.0,
            integer: true,
            suffix: ' kHz',
            onCommit: (v) => c.setMasterFx(
              reverbWet: c.status?.reverbWet ?? 0.16,
              reverbSize: c.status?.reverbSize ?? 0.5,
              trebleHz: v * 1000.0,
              sidechain: c.status?.sidechain ?? 1.0,
            ),
          ),
          _VolumeRow(
            icon: Icons.graphic_eq,
            tooltip: 'Sidechain (pump depth)',
            value: c.status?.sidechain ?? 1.0,
            max: 2.0,
            percent: true,
            onCommit: (v) => c.setMasterFx(
              reverbWet: c.status?.reverbWet ?? 0.16,
              reverbSize: c.status?.reverbSize ?? 0.5,
              trebleHz: c.status?.trebleHz ?? 19000,
              sidechain: v,
            ),
          ),
          _VolumeRow(
            icon: Icons.filter_alt,
            tooltip: 'Filter openness (on top of the arrangement)',
            value: c.status?.toneOpen ?? 0.0,
            min: -0.5,
            max: 0.5,
            onCommit: (v) => c.setToneOpen(v),
          ),
          if (c.tracks.isNotEmpty) ...[
            const Divider(height: 32),
            Row(
              children: [
                const Expanded(
                  child: Text('Instruments',
                      style: TextStyle(
                          fontSize: 16, fontWeight: FontWeight.w600)),
                ),
                IconButton(
                  onPressed: (c.status?.instrumentListEdited ?? false)
                      ? c.resetInstruments
                      : null,
                  icon: const Icon(Icons.restart_alt),
                  tooltip: 'Reset instrument list to style',
                  visualDensity: VisualDensity.compact,
                ),
              ],
            ),
            for (final entry in _groupedTracks(c)) ...[
              Padding(
                padding: const EdgeInsets.fromLTRB(4, 8, 4, 4),
                child: Text(
                  _groupMeta[entry.$1]?.$2 ?? entry.$1,
                  style: TextStyle(
                    fontSize: 13,
                    fontWeight: FontWeight.w600,
                    color: Theme.of(context)
                        .colorScheme
                        .onSurface
                        .withValues(alpha: 0.6),
                  ),
                ),
              ),
              for (final t in entry.$2)
                _VolumeRow(
                  icon: _groupMeta[t.group]?.$1 ?? Icons.music_note,
                  tooltip: (_slotMeta[t.name] ?? t.name) +
                      (t.volume <= 0.0 ? ' (off)' : ''),
                  // Absolute 0..100%: the style base is no longer a ceiling.
                  value: t.volume,
                  max: 1.0,
                  percent: true,
                  dimmed: t.volume <= 0.0,
                  onCommit: (v) => c.setTrackVolume(t.id, v),
                ),
            ],
          ],
        ],
      ),
    );
  }
}

/// Slider row: local response while dragging, one backend commit on release.
/// Otherwise every drag tick means 2–3 FFI round trips plus a rebuild of the
/// whole screen: on a phone the slider freezes the UI. A backend rollback
/// (protection of the last slot) arrives on its own via notifyListeners after commit.
class _VolumeRow extends StatefulWidget {
  const _VolumeRow({
    required this.icon,
    required this.tooltip,
    required this.value,
    required this.max,
    required this.onCommit,
    this.min = 0.0,
    this.suffix = '',
    this.integer = false,
    this.percent = false,
    this.dimmed = false,
    this.onReset,
  });

  final IconData icon;
  final String tooltip;
  final double value;
  final double max;
  final double min;
  final String suffix;
  final bool integer;
  final bool percent;
  final void Function(double) onCommit;

  /// Dim the row (group/slot disabled by the style).
  final bool dimmed;

  /// Optional button to reset this fader to the style value.
  final VoidCallback? onReset;

  @override
  State<_VolumeRow> createState() => _VolumeRowState();
}

class _VolumeRowState extends State<_VolumeRow> {
  late double _local;
  bool _dragging = false;

  @override
  void initState() {
    super.initState();
    _local = widget.value;
  }

  @override
  void didUpdateWidget(_VolumeRow oldWidget) {
    super.didUpdateWidget(oldWidget);
    // Apply an external value (commit/rollback) only outside a drag,
    // otherwise the slider is yanked out from under the finger.
    if (!_dragging && widget.value != oldWidget.value) {
      _local = widget.value;
    }
  }

  double get _shown => _dragging ? _local : widget.value;

  String _text(double value) {
    if (widget.suffix.isNotEmpty) return '${value.round()}${widget.suffix}';
    if (widget.integer) return '${value.round()}';
    if (widget.percent) return '${(value * 100).round()}%';
    return value.toStringAsFixed(2);
  }

  @override
  Widget build(BuildContext context) {
    final v = _shown.clamp(widget.min, widget.max);
    return Opacity(
      opacity: widget.dimmed ? 0.45 : 1.0,
      child: Row(
        children: [
          Tooltip(
            message: widget.tooltip,
            child: Icon(widget.icon, size: 24),
          ),
          const SizedBox(width: 8),
          Expanded(
            child: Slider(
              value: v,
              min: widget.min,
              max: widget.max,
              divisions:
                  widget.integer ? (widget.max - widget.min).round() : null,
              onChanged: (nv) {
                _dragging = true;
                setState(() {
                  _local = widget.integer ? nv.roundToDouble() : nv;
                });
              },
              onChangeEnd: (nv) {
                _dragging = false;
                final commit =
                    widget.integer ? nv.roundToDouble() : nv;
                widget.onCommit(commit);
              },
            ),
          ),
          SizedBox(
            width: 56,
            child: Text(
              _text(v),
              textAlign: TextAlign.end,
              style: const TextStyle(fontSize: 12),
            ),
          ),
          if (widget.onReset != null)
            IconButton(
              onPressed: widget.onReset,
              icon: const Icon(Icons.restart_alt, size: 18),
              tooltip: 'Reset to style default',
              visualDensity: VisualDensity.compact,
            ),
        ],
      ),
    );
  }
}

/// Right panel: style list with previews and sorting
/// (by Club/Rock/Chill shelves or alphabetically).
/// Used both as the endDrawer (portrait) and as a side panel (landscape).
class _StylesPanelBody extends StatelessWidget {
  const _StylesPanelBody({required this.controller, required this.bridge});

  final PlayerController controller;
  final AudioBridge bridge;

  /// Shelf title: group name capitalized.
  static String _groupTitle(String group) => group.isEmpty
      ? group
      : group[0].toUpperCase() + group.substring(1);

  @override
  Widget build(BuildContext context) {
    final c = controller;
    final names = c.sortedStyles();
    return SafeArea(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.start,
        children: [
          Padding(
            padding: const EdgeInsets.all(16),
            child: Row(
              children: [
                const Text('Styles',
                    style:
                        TextStyle(fontSize: 20, fontWeight: FontWeight.w600)),
                const Spacer(),
                IconButton(
                  tooltip: c.styleSortMode == 'group'
                      ? 'Sort alphabetically'
                      : 'Sort by category',
                  icon: Icon(c.styleSortMode == 'group'
                      ? Icons.sort_by_alpha
                      : Icons.category_outlined),
                  onPressed: c.toggleStyleSort,
                ),
              ],
            ),
          ),
          Expanded(
            child: ListView(
              padding: const EdgeInsets.symmetric(horizontal: 8),
              children: [
                if (c.styleSortMode == 'group')
                  for (final entry in _grouped(names, c)) ...[
                    if (entry.$2.isNotEmpty) ...[
                      Padding(
                        padding: const EdgeInsets.fromLTRB(8, 8, 8, 4),
                        child: Text(
                          _groupTitle(entry.$1),
                          style: TextStyle(
                            fontSize: 13,
                            fontWeight: FontWeight.w600,
                            color: Theme.of(context)
                                .colorScheme
                                .onSurface
                                .withValues(alpha: 0.6),
                          ),
                        ),
                      ),
                      for (final name in entry.$2)
                        _StyleTile(
                            name: name,
                            controller: c,
                            bridge: bridge),
                    ],
                  ]
                else
                  for (final name in names)
                    _StyleTile(
                        name: name, controller: c, bridge: bridge),
              ],
            ),
          ),
        ],
      ),
    );
  }

  /// Names grouped by shelf in backend order
  /// (order of the shelf's first appearance). Returns pairs
  /// (shelf, names) — the caller drops empty shelves.
  List<(String, List<String>)> _grouped(
      List<String> names, PlayerController c) {
    final order = <String>[];
    for (final n in names) {
      final g = c.styleGroupOf(n);
      if (!order.contains(g)) order.add(g);
    }
    final out = <(String, List<String>)>[];
    for (final g in order) {
      out.add((g, [for (final n in names) if (c.styleGroupOf(n) == g) n]));
    }
    return out;
  }
}

/// Style row: preview, title, checkmark for the current one.
class _StyleTile extends StatelessWidget {
  const _StyleTile({
    required this.name,
    required this.controller,
    required this.bridge,
  });

  final String name;
  final PlayerController controller;
  final AudioBridge bridge;

  @override
  Widget build(BuildContext context) {
    final c = controller;
    return Column(
      mainAxisSize: MainAxisSize.min,
      children: [
        ListTile(
          leading: StylePreview(style: name),
          title: Text(_styleTitle(name)),
          trailing: name == c.styleName
              ? const Icon(Icons.check, color: Colors.green)
              : null,
          onTap: () async {
            // ListTile doesn't take focus (unlike the sliders in the
            // left menu), so clear it explicitly: otherwise when the drawer
            // closes FocusScope returns focus to the seed box.
            FocusManager.instance.primaryFocus?.unfocus();
            await c.chooseStyle(name);
            await bridge.sync();
            // Styles live in the endDrawer — isDrawerOpen checks the left
            // Drawer, so the panel stayed open after selection.
            if (context.mounted && Scaffold.of(context).isEndDrawerOpen) {
              Navigator.of(context).pop();
              WidgetsBinding.instance.addPostFrameCallback((_) {
                FocusManager.instance.primaryFocus?.unfocus();
              });
            }
          },
        ),
        const SizedBox(height: 4),
      ],
    );
  }
}
