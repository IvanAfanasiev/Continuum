import 'dart:async';

import 'package:flutter/material.dart';

import 'package:cont/src/audio_bridge.dart';
import 'package:cont/src/player_controller.dart';
import 'package:cont/src/rust/playback.dart';
import 'package:cont/src/seed_input.dart';

/// Треки в порядке полок групп (как 8 фейдеров выше):
/// порядок групп — из _groupMeta, внутри — порядок бэкенда.
List<TrackInfo> _orderedTracks(PlayerController c) {
  const groupOrder = [
    'bass',
    'kick',
    'snare',
    'hat',
    'chords',
    'lead',
    'vocal',
    'texture'
  ];
  final list = c.tracks.toList();
  list.sort((a, b) {
    final ga = groupOrder.indexOf(a.group);
    final gb = groupOrder.indexOf(b.group);
    final ca = ga < 0 ? groupOrder.length : ga;
    final cb = gb < 0 ? groupOrder.length : gb;
    return ca != cb ? ca.compareTo(cb) : a.id.compareTo(b.id);
  });
  return list;
}

/// Подписи и иконки групп для обычного пользователя (без слов-TODO).
const _groupMeta = {
  'bass': (Icons.speaker, 'Бас'),
  'kick': (Icons.album, 'Кик'),
  'snare': (Icons.circle, 'Снейр'),
  'hat': (Icons.donut_small, 'Хэт'),
  'chords': (Icons.piano, 'Аккорды'),
  'lead': (Icons.music_note, 'Лид'),
  'vocal': (Icons.mic, 'Вокал'),
  'texture': (Icons.grain, 'Атмосфера'),
};

/// Пути арта. Файлы лежат в assets/images/ (см. README там):
/// фон `bg_<стиль>.png`, превью `style_<стиль>.png`.
/// Нет файла — черный фон (errorBuilder), приложение не падает.
class AppArtwork {
  static String background(String style) =>
      'assets/images/bg_${style.toLowerCase()}.png';

  static String preview(String style) =>
      'assets/images/style_${style.toLowerCase()}.png';

  static const String appIcon = 'assets/images/app_icon.png';
}

/// Фон экрана: картинка стиля на весь экран + темная вуаль для
/// читаемости белого текста. Без файла — просто черный.
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

/// Мини-превью стиля 48x48. Без файла — черный квадрат.
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

/// Русские имена слотов для секции инструментов.
const _slotMeta = {
  'bass': 'Бас',
  'kick': 'Кик',
  'pad': 'Пэд',
  'snare': 'Снейр',
  'hat': 'Хэт',
  'pluck': 'Щипок',
  'keys': 'Клавиши',
  'kick808': '808',
  'laser': 'Лазер',
  'vinyl': 'Винил',
  'vocal': 'Вокал',
  'flute': 'Флейта',
  'festival': 'Фестиваль',
  'crash': 'Крэш',
  'guitar': 'Гитара',
  'rain': 'Дождь',
  'fuzz': 'Фазз',
  'acid': 'Эйсид',
};

String _styleTitle(String style) {
  if (style.isEmpty) return 'Club';
  return style[0].toUpperCase() + style.substring(1);
}

/// Диагностика тракта для баннера (null — все растет как надо).
/// Первые секунды после старта молчание — норма (открывается устройство).
String? _statsDiag(PlayerController c) {
  if (!(c.running && !c.paused)) return null;
  if ((c.status?.engineError ?? '').isNotEmpty) return null;
  final stats = c.stats;
  final started = c.startedAt;
  if (stats == null || started == null) return null;
  if (DateTime.now().difference(started).inSeconds < 6) return null;
  if (stats.frames == BigInt.zero) {
    final cfg =
        stats.audioConfig.isEmpty ? 'конфиг неизвестен' : stats.audioConfig;
    return 'Звуковой поток не запустился ($cfg). '
        'Ноты генератора: ${stats.pushed}. '
        'Проверь громкость медиа и Bluetooth.';
  }
  if (stats.pushed == BigInt.zero) {
    return 'Генератор не выдаёт ноты — напиши разработчику.';
  }
  if (stats.consumed == BigInt.zero) {
    return 'Ноты не доходят до движка — напиши разработчику.';
  }
  if (c.streamStalled) {
    final n = stats.restarts;
    return 'Звуковой поток остановился — переподключаю'
        '${n == BigInt.zero ? '' : ' (попытка ${n.toString()})'}. '
        'Если висит — смени маршрут (наушники/динамик).';
  }
  return null;
}

/// Плашка-уведомление под сидом: ошибка движка или диагностика тракта.
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
    // Легкий опрос статуса, пока играет: фронт видит правду бэкенда.
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
    // Горизонталь: кнопки прячем, панели выдвинуты сразу; центр
    // держим узким — вертикальный стиль сохраняется.
    final landscape =
        MediaQuery.of(context).size.aspectRatio > 1.0;
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
      // Широкая зона свайпа от края: меню открывается жестом,
      // а не только кнопками (дефолтные 20pt не нащупать).
      drawerEdgeDragWidth: 72,
      // Гасим фокус при открытии И закрытии меню: FocusScope иначе
      // возвращает фокус в ячейку сида при закрытии шторки (+клавиатура).
      // Полностью запрещать фокус нельзя — иначе не напечатать сид,
      // поэтому гасим агрессивно везде, кроме самого ввода.
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

/// Боковая панель ландшафта: фиксированные 300pt, полупрозрачная
/// поверх общего фона. Центр за счет этого всегда одного размера.
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

/// Середина: название стиля, сид, play по центру (+ боковые кнопки
/// только в портрете).
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

  Future<void> _toggle(BuildContext context) async {
    FocusManager.instance.primaryFocus?.unfocus();
    final c = controller;
    if (c.running && !c.paused) {
      await bridge.pause();
    } else {
      await bridge.play();
    }
  }

  Future<void> _seed(String code) async {
    await controller.submitSeedCode(code);
    await bridge.sync();
  }

  Future<void> _dice(BuildContext context) async {
    FocusManager.instance.primaryFocus?.unfocus();
    await controller.randomSeed();
    await bridge.sync();
  }

  @override
  Widget build(BuildContext context) {
    final c = controller;
    final playing = c.running && !c.paused;
    return Column(
      children: [
        // Центр: название стиля и сид.
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
                compact: MediaQuery.of(context).size.width < 600,
              ),
              // Ошибка аудиодвижка: раньше была тишина без объяснений.
              if (c.status?.engineError case final err? when err.isNotEmpty)
                _NoticeBanner(
                  icon: Icons.warning_amber,
                  color: Colors.redAccent,
                  text: err,
                ),
              // Диагностика тракта: поток не стартует / генератор молчит.
              if (_statsDiag(c) case final diag? when diag.isNotEmpty)
                _NoticeBanner(
                  icon: Icons.hearing,
                  color: Colors.amber,
                  text: diag,
                ),
            ],
          ),
        ),
        // Нижняя половина: play по центру.
        Expanded(
          child: Row(
            mainAxisAlignment: MainAxisAlignment.center,
            children: [
              if (showSideButtons) ...[
                IconButton(
                  tooltip: 'Громкость и темп',
                  iconSize: 32,
                  color: Colors.white,
                  onPressed: onOpenMix,
                  icon: const Icon(Icons.tune),
                ),
                const SizedBox(width: 32),
              ],
              GestureDetector(
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
              if (showSideButtons) ...[
                const SizedBox(width: 32),
                IconButton(
                  tooltip: 'Стили',
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

/// Левая панель: темп и энергия сверху, затем группы.
/// Используется и в Drawer (портрет), и боком (ландшафт).
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
          const Text('Микс',
              style: TextStyle(fontSize: 20, fontWeight: FontWeight.w600)),
          const SizedBox(height: 8),
          _VolumeRow(
            icon: Icons.speed,
            tooltip: 'Темп',
            value: c.status?.tempoBpm ?? 126,
            min: 50,
            max: 180,
            integer: true,
            suffix: ' BPM',
            onChanged: (v) => c.setTempo(v),
          ),
          _VolumeRow(
            icon: Icons.bolt,
            tooltip: 'Энергия',
            value: c.status?.intensity ?? 1.0,
            max: 2.0,
            percent: true,
            onChanged: (v) => c.setIntensity(v),
          ),
          const Divider(height: 32),
          // Группы всегда доступны: ручка задает потолок громкости
          // для инструментов своей группы (слот выше потолка не уйдет).
          // Пустая группа тоже двигается — потолок запомнится и сработает,
          // как только включишь в ней инструмент.
          for (final g in c.groups)
            _VolumeRow(
              icon: _groupMeta[g.name]?.$1 ?? Icons.tune,
              tooltip: _groupMeta[g.name]?.$2 ?? g.name,
              value: g.volume,
              max: 1.0,
              percent: true,
              onChanged: (v) => c.setGroupVolume(g.name, v),
            ),
          // Все 16 инструментов доступны всегда: вайтлист стиля лишь
          // расставляет дефолты (вне списка — 0, внутри — 100% базы),
          // поднять любой вручную можно в любой момент. Смена стиля
          // снова сбросит уровни на дефолт. Шкала — % от базы стиля.
          if (c.tracks.isNotEmpty) ...[
            const Divider(height: 32),
            const Text('Инструменты',
                style: TextStyle(fontSize: 20, fontWeight: FontWeight.w600)),
            const SizedBox(height: 8),
            // Порядок — как полки групп: бас, ударные, аккорды, лиды,
            // вокал, атмосфера. Внутри группы — порядок слотов бэкенда.
            for (final t in _orderedTracks(c))
              _VolumeRow(
                icon: _groupMeta[t.group]?.$1 ?? Icons.music_note,
                tooltip: (_slotMeta[t.name] ?? t.name) +
                    (t.volume <= 0.0 ? ' — выкл' : ''),
                // Шкала — % от базы стиля, потолок 100%:
                // выше базы слот не поднять.
                value: t.baseVolume > 0
                    ? (t.volume / t.baseVolume).clamp(0.0, 1.0)
                    : 0.0,
                max: 1.0,
                percent: true,
                dimmed: t.volume <= 0.0,
                onChanged: (v) => c.setTrackVolume(t.id, v * t.baseVolume),
              ),
          ],
        ],
      ),
    );
  }
}

class _VolumeRow extends StatelessWidget {
  const _VolumeRow({
    required this.icon,
    required this.tooltip,
    required this.value,
    required this.max,
    required this.onChanged,
    this.min = 0.0,
    this.suffix = '',
    this.integer = false,
    this.percent = false,
    this.dimmed = false,
  });

  final IconData icon;
  final String tooltip;
  final double value;
  final double max;
  final double min;
  final String suffix;
  final bool integer;
  final bool percent;
  final void Function(double) onChanged;

  /// Затемнение строки (группа/слот выключены стилем).
  final bool dimmed;

  String get _text {
    if (suffix.isNotEmpty) return '${value.round()}$suffix';
    if (integer) return '${value.round()}';
    if (percent) return '${(value * 100).round()}%';
    return value.toStringAsFixed(2);
  }

  @override
  Widget build(BuildContext context) {
    return Opacity(
      opacity: dimmed ? 0.45 : 1.0,
      child: Row(
      children: [
        Tooltip(
          message: tooltip,
          child: Icon(icon, size: 24),
        ),
        const SizedBox(width: 8),
        Expanded(
          child: Slider(
            value: value.clamp(min, max),
            min: min,
            max: max,
            divisions: integer ? (max - min).round() : null,
            onChanged: (v) => onChanged(integer ? v.roundToDouble() : v),
          ),
        ),
        SizedBox(
          width: 56,
          child: Text(
            _text,
            textAlign: TextAlign.end,
            style: const TextStyle(fontSize: 12),
          ),
        ),
      ],
      ),
    );
  }
}

/// Правая панель: список стилей с превью и сортировкой
/// (по полкам Club/Rock/Chill или по алфавиту).
/// Используется и в endDrawer (портрет), и боком (ландшафт).
class _StylesPanelBody extends StatelessWidget {
  const _StylesPanelBody({required this.controller, required this.bridge});

  final PlayerController controller;
  final AudioBridge bridge;

  /// Заголовок полки: имя группы с заглавной буквы.
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
                const Text('Стили',
                    style:
                        TextStyle(fontSize: 20, fontWeight: FontWeight.w600)),
                const Spacer(),
                IconButton(
                  tooltip: c.styleSortMode == 'group'
                      ? 'Сортировать по алфавиту'
                      : 'Сортировать по группам',
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

  /// Имена, сгруппированные по полкам в порядке бэкенда
  /// (порядок первого появления полки). Возвращает пары
  /// (полка, имена) — пустые полки отсекает вызывающий.
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

/// Строка стиля: превью, название, галочка текущего.
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
            // ListTile фокус не забирает (в отличие от слайдеров
            // левого меню), поэтому гасим явно: иначе при закрытии
            // шторки FocusScope вернет фокус в ячейку сида.
            FocusManager.instance.primaryFocus?.unfocus();
            await c.chooseStyle(name);
            await bridge.sync();
            if (context.mounted && Scaffold.of(context).isDrawerOpen) {
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
