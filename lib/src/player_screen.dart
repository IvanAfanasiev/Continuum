import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import 'package:cont/src/audio_bridge.dart';
import 'package:cont/src/player_controller.dart';
import 'package:cont/src/rust/api/playback.dart';
import 'package:cont/src/rust/playback.dart';
import 'package:cont/src/seed_input.dart';

/// Треки, разложенные по полкам групп. Порядок полок — из бэкенда
/// (`groups` идут в каноническом порядке), внутри — порядок слотов.
/// Пустые полки пропускаются. Никаких своих списков во фронте.
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
  'guitar': 'Гитара',
  'rain': 'Дождь',
  'fuzz': 'Фазз',
  'shaker': 'Шейкер',
  'formula': 'Формула',
  'wavetable': 'Вейвтейбл',
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

  /// Режим фона: фото стиля или живой круговой график мотива.
  bool _graphMode = false;

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
    // Фон: фото стиля или живой график (переключатель под сидом).
    Widget background() => _graphMode
        ? _MotifWheel(controller: c)
        : StyleBackground(style: c.styleName);
    if (landscape) {
      return Scaffold(
        backgroundColor: Colors.black,
        body: Stack(
          children: [
            background(),
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
                          graphMode: _graphMode,
                          onToggleGraph: () =>
                              setState(() => _graphMode = !_graphMode),
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
          background(),
          SafeArea(
            child: _CenterColumn(
              controller: c,
              bridge: widget.bridge,
              showSideButtons: true,
              onOpenMix: () => _scaffoldKey.currentState?.openDrawer(),
              onOpenStyles: () => _scaffoldKey.currentState?.openEndDrawer(),
              graphMode: _graphMode,
              onToggleGraph: () =>
                  setState(() => _graphMode = !_graphMode),
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
    required this.graphMode,
    required this.onToggleGraph,
  });

  final PlayerController controller;
  final AudioBridge bridge;
  final bool showSideButtons;
  final VoidCallback? onOpenMix;
  final VoidCallback? onOpenStyles;
  /// Фон-график вместо фото + переключатель режима.
  final bool graphMode;
  final VoidCallback onToggleGraph;

  Future<void> _toggle(BuildContext context) async {
    FocusManager.instance.primaryFocus?.unfocus();
    final c = controller;
    if (c.running && !c.paused) {
      await bridge.pause();
    } else if (!await bridge.play()) {
      // Трек не создался (нет устройства): объясняем, а не молчим.
      if (context.mounted) {
        ScaffoldMessenger.of(context).showSnackBar(
          const SnackBar(
            content: Text('Не удалось запустить звук: нет устройства вывода'),
          ),
        );
      }
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
              // Переключатель фона: фото стиля / живой график мотива.
              TextButton.icon(
                onPressed: onToggleGraph,
                icon: Icon(
                  graphMode ? Icons.photo : Icons.donut_large,
                  size: 14,
                ),
                label: Text(
                  graphMode ? 'Фото' : 'График',
                  style: const TextStyle(fontSize: 11),
                ),
              ),
              // Диагностика тракта: поток не стартует / генератор молчит.
              if (_statsDiag(c) case final diag? when diag.isNotEmpty)
                _NoticeBanner(
                  icon: Icons.hearing,
                  color: Colors.amber,
                  text: diag,
                ),
              // Бортовой журнал: видно без USB, что творится в тракте.
              // Жив ли поток — панель выяснит сама опросом.
              _DiagPanel(bridge: bridge, controller: c, visible: c.running),
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

/// Бортовой журнал нативного потока: последние строки + кнопки.
/// Опрашивается раз в 3с (в ритме поллера статуса) — живых тиков нет,
/// чтобы не будить перестройки ради диагностики.
class _DiagPanel extends StatefulWidget {
  const _DiagPanel(
      {required this.bridge, required this.controller, required this.visible});

  final AudioBridge bridge;
  final PlayerController controller;
  final bool visible;

  @override
  State<_DiagPanel> createState() => _DiagPanelState();
}

class _DiagPanelState extends State<_DiagPanel> {
  bool _open = false;
  bool _alive = false;
  List<String> _lines = const [];
  Timer? _poller;

  @override
  void initState() {
    super.initState();
    _poller = Timer.periodic(const Duration(seconds: 3), (_) => _poll());
    _poll();
  }

  @override
  void dispose() {
    _poller?.cancel();
    super.dispose();
  }

  Future<void> _poll() async {
    if (!mounted) return;
    try {
      final alive = await widget.bridge.pullActive();
      // Строки забираем всегда: посмертный диагноз мертвого потока
      // важнее, чем скрытие пустоты.
      final lines = await widget.bridge.diagLog();
      if (!mounted) return;
      setState(() {
        _alive = alive;
        _lines = lines;
      });
    } catch (_) {
      // Диагностика необязательна — молча пропускаем.
    }
  }

  @override
  Widget build(BuildContext context) {
    if (!widget.visible) return const SizedBox.shrink();
    final lines = _lines;
    return Padding(
      padding: const EdgeInsets.only(top: 8),
      child: Column(
        mainAxisSize: MainAxisSize.min,
        children: [
          GestureDetector(
            onTap: () => setState(() => _open = !_open),
            child: Text(
              _open
                  ? 'Скрыть журнал'
                  : 'Журнал тракта (${lines.length})${_alive ? '' : ' — поток спит'} · движок v${widget.controller.status?.engineVersion ?? '?'}',
              style: const TextStyle(fontSize: 11, color: Colors.white54),
            ),
          ),
          if (_open) ...[
            const SizedBox(height: 4),
            Container(
              constraints: const BoxConstraints(maxHeight: 160),
              padding: const EdgeInsets.all(6),
              decoration: BoxDecoration(
                color: Colors.white.withValues(alpha: 0.06),
                borderRadius: BorderRadius.circular(6),
              ),
              child: SingleChildScrollView(
                child: SelectableText(
                  lines.isEmpty
                      ? 'пока пусто — запусти генерацию'
                      : lines.join('\n'),
                  style: const TextStyle(
                    fontSize: 10,
                    color: Colors.white70,
                    fontFamily: 'monospace',
                  ),
                ),
              ),
            ),
            Row(
              mainAxisSize: MainAxisSize.min,
              children: [
                TextButton.icon(
                  onPressed: lines.isEmpty
                      ? null
                      : () => Clipboard.setData(
                          ClipboardData(text: lines.join('\n'))),
                  icon: const Icon(Icons.copy, size: 14),
                  label: const Text(
                    'Копировать',
                    style: TextStyle(fontSize: 11),
                  ),
                ),
                TextButton.icon(
                  onPressed: !_alive
                      ? null
                      : () async {
                          final path =
                              await widget.bridge.startCapture();
                          if (context.mounted) {
                            ScaffoldMessenger.of(context).showSnackBar(
                              SnackBar(
                                content: Text(path == null
                                    ? 'Запись не стартовала'
                                    : 'Пишу 30с сюда: $path'),
                              ),
                            );
                          }
                        },
                  icon: const Icon(Icons.fiber_manual_record, size: 14),
                  label: const Text(
                    'Запись 30с',
                    style: TextStyle(fontSize: 11),
                  ),
                ),
              ],
            ),
          ],
        ],
      ),
    );
  }
}

/// Круговой график-партитура: 4 орбиты главных голосов (кик снаружи,
/// тема внутри), точки — доли голоса за фразу на своих местах.
/// План фразы приходит целиком при ее старте; рука делает оборот
/// за фразу независимо, но в тех же часах — и «играет» точки:
/// близкая к стрелке пульсирует в тайминг.
/// Ход 20 Гц, опрос 10 Гц. Только пока играет и виден график.
class _MotifWheel extends StatefulWidget {
  const _MotifWheel({required this.controller});

  final PlayerController controller;

  @override
  State<_MotifWheel> createState() => _MotifWheelState();
}

class _MotifWheelState extends State<_MotifWheel> {
  Timer? _ticker;
  LiveVisual? _frame;

  /// Якорь руки от движка: фаза такта + момент замера. Между опросами
  /// рука интерполируется от якоря с точной скоростью BPM — плавно
  /// и в ногу с точками (те же часы, что и маски).
  double _anchorP = 0.0;
  DateTime _anchorT = DateTime.now();

  /// Маски голосов из последнего опроса (план текущей фразы):
  /// точки стоят с начала фразы на своих местах — опоздания нет.
  /// Рука и точки идут от одних часов движка, поэтому совпадают.
  int _liveKick = 0;
  int _liveSnare = 0;
  int _liveBass = 0;
  int _liveTheme = 0;

  /// Сглаженная фаза руки 0..1: тянется к цели (якорь + время × BPM),
  /// а не прыгает на нее — свингованные 16-е дышат внутри такта,
  /// опрос это видит с опозданием; тянучка ~120мс съедает щелчки.
  double _display = 0.0;

  double _bpm() =>
      (widget.controller.status?.tempoBpm ?? 126).toDouble();

  static const _sections = ['Main', 'Build', 'Drop', 'Post'];

  @override
  void initState() {
    super.initState();
    _anchorT = DateTime.now();
    // Тик 50мс: рука тянется к цели каждый тик (20 Гц, плавно),
    // опрос движка каждый 2-й тик (10 Гц): якорь свежий, точки
    // загораются почти в момент звучания (опоздание ~0.1с, не такт).
    var n = 0;
    _ticker = Timer.periodic(const Duration(milliseconds: 50), (_) async {
      if (!mounted) return;
      final running =
          widget.controller.running && !widget.controller.paused;
      if (running) {
        // Цель: якорь фразы + прошедшее время × скорость BPM.
        // Оборот руки — фраза (2 такта): при 120 BPM фраза ~4 с.
        final elapsed =
            DateTime.now().difference(_anchorT).inMilliseconds / 1000.0;
        final target = (_anchorP + elapsed * _bpm() / 480.0) % 1.0;
        var d = (target - _display) % 1.0;
        if (d > 0.5) d -= 1.0;
        if (d.abs() > 0.25) {
          // Смена сида/стиля — прыгаем сразу, тянуть нечего.
          _display = target;
        } else {
          _display = (_display + d * 0.4) % 1.0;
        }
      }
      n++;
      if (n % 2 != 0 || !running) {
        setState(() {});
        return;
      }
      try {
        final v = await playbackLiveVisual();
        if (mounted) {
          setState(() {
            _frame = v;
            // Якорь руки — фаза фразы каждый опрос: дрейфу негде копиться.
            _anchorP = v.phrasePos / v.phraseLen.clamp(1, 32);
            _anchorT = DateTime.now();
            // План фразы — как есть: точки совпадают с рукой.
            _liveKick = v.pkick;
            _liveSnare = v.psnare;
            _liveBass = v.pbass;
            _liveTheme = v.ptheme;
          });
        }
      } catch (_) {
        // Кадр необязателен — рука идет от якоря, колесо живет.
      }
    });
  }

  @override
  void dispose() {
    _ticker?.cancel();
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final v = _frame;
    // Рука — сглаженная фаза из тикера (на паузе стоит сама).
    final hand = _display;
    final bpm = _bpm();
    final phraseSecs = 480.0 / bpm;
    // Точки — живой такт из последнего опроса: загораются в момент
    // звучания под рукой (те же часы, рассинхрона нет).
    final kick = _liveKick;
    final snare = _liveSnare;
    final bass = _liveBass;
    final theme = _liveTheme;
    final energy = v?.energy ?? 0.0;
    return Container(
      color: Colors.black,
      child: Center(
        child: Column(
          mainAxisAlignment: MainAxisAlignment.center,
          children: [
            SizedBox(
              width: 280,
              height: 280,
              child: CustomPaint(
                painter: _WheelPainter(
                  kick: kick,
                  snare: snare,
                  bass: bass,
                  theme: theme,
                  phase: hand,
                  energy: energy,
                ),
              ),
            ),
            const SizedBox(height: 8),
            Text.rich(
              TextSpan(
                style:
                    const TextStyle(fontSize: 12, color: Colors.white54),
                children: [
                  const TextSpan(text: '● ', style: TextStyle(color: Color(0xFFFF5252))),
                  const TextSpan(text: 'кик  '),
                  const TextSpan(text: '● ', style: TextStyle(color: Color(0xFF4FC3F7))),
                  const TextSpan(text: 'снейр  '),
                  const TextSpan(text: '● ', style: TextStyle(color: Color(0xFF9CCC65))),
                  const TextSpan(text: 'бас  '),
                  const TextSpan(text: '● ', style: TextStyle(color: Colors.white)),
                  const TextSpan(text: 'тема'),
                ],
              ),
            ),
            const SizedBox(height: 2),
            Text(
              v == null
                  ? 'точки появятся с музыкой'
                  : 'фраза ${phraseSecs.toStringAsFixed(1)}с · ${bpm.toStringAsFixed(0)} BPM · E${(energy * 100).round()}% · ${_sections[v.section.clamp(0, 3)]}',
              style: const TextStyle(fontSize: 12, color: Colors.white54),
            ),
          ],
        ),
      ),
    );
  }
}

class _WheelPainter extends CustomPainter {
  _WheelPainter({
    required this.kick,
    required this.snare,
    required this.bass,
    required this.theme,
    required this.phase,
    required this.energy,
  });

  /// Маски голосов по долям такта (бит = 16-я доля).
  final int kick;
  final int snare;
  final int bass;
  final int theme;
  final double phase;
  final double energy;

  @override
  void paint(Canvas canvas, Size size) {
    final center = Offset(size.width / 2, size.height / 2);
    final r = size.shortestSide / 2 * 0.92;
    // Фраза = 32 доли (2 такта по 16): позиции точек и руки.
    const n = 32;
    // Угол руки (0 — верх, по часовой).
    final handA = phase * 6.2831853;

    // Близость доли к руке 0..1 (1 — рука точно на точке):
    // точки пульсируют, когда стрелка проходит мимо в тайминг.
    double near(int i) {
      var d = (i / n * 6.2831853 - handA).abs() % 6.2831853;
      if (d > 3.14159265) d = 6.2831853 - d;
      return (1.0 - d / 0.35).clamp(0.0, 1.0);
    }

    // Орбита голоса: свой радиус — точки голосов не лежат друг на друге,
    // каждый голос читается отдельно. Доли и даунбит фразы крупнее.
    void voice(int mask, double orbit, Color color, double size) {
      // Едва видимая орбита.
      canvas.drawCircle(
        center,
        r * orbit,
        Paint()
          ..style = PaintingStyle.stroke
          ..strokeWidth = 1
          ..color = const Color(0x1AFFFFFF),
      );
      for (var i = 0; i < n; i++) {
        if ((mask & (1 << i)) == 0) continue;
        final ang = i / n * 6.2831853 - 1.5707963;
        final p = center +
            Offset(
                r * orbit * math.cos(ang), r * orbit * math.sin(ang));
        final px = near(i);
        // Доля такта (каждая 4-я) и даунбит фразы крупнее: метр виден.
        final beat = i % 4 == 0 ? 1.0 : 0.0;
        final down = i == 0 ? 1.5 : 0.0;
        canvas.drawCircle(
          p,
          size + beat + down + 3.0 * px * px,
          Paint()
            ..color = px > 0.55
                ? const Color(0xFFFFB300)
                : color,
        );
      }
    }

    // Свечение центра от энергии.
    final glow = (energy.clamp(0.0, 1.2) / 1.2 * r * 0.35).clamp(4.0, r * 0.4);
    canvas.drawCircle(
      center,
      glow,
      Paint()..color = const Color(0x22FFB300),
    );

    // Голоса на своих орбитах: тема внутри, кик снаружи.
    voice(theme, 0.44, const Color(0xCCFFFFFF), 3.0);
    voice(bass, 0.61, const Color(0xFF9CCC65), 3.5);
    voice(snare, 0.78, const Color(0xFF4FC3F7), 3.5);
    voice(kick, 0.95, const Color(0xFFFF5252), 4.0);

    // Рука фразы через все орбиты: оборот за фразу, идет от BPM.
    final a = phase * 6.2831853 - 1.5707963;
    canvas.drawLine(
      center,
      center + Offset(r * 0.98 * math.cos(a), r * 0.98 * math.sin(a)),
      Paint()
        ..strokeWidth = 1.5
        ..color = const Color(0x66FFFFFF),
    );
  }

  @override
  bool shouldRepaint(_WheelPainter old) => true;
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
            onCommit: (v) => c.setTempo(v),
          ),
          _VolumeRow(
            icon: Icons.bolt,
            tooltip: 'Энергия',
            value: c.status?.intensity ?? 1.0,
            max: 2.0,
            percent: true,
            onCommit: (v) => c.setIntensity(v),
          ),
          // Инструменты, сгруппированные по полкам (порядок полок —
          // как 8 типов выше было). Групповых фейдеров нет: ручка
          // инструмента — единственная, потолок ей не нужен.
          if (c.tracks.isNotEmpty) ...[
            const Divider(height: 32),
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
                    (t.volume <= 0.0 ? ' — выкл' : ''),
                // Шкала — % от базы стиля, потолок 100%:
                // выше базы слот не поднять.
                value: t.baseVolume > 0
                    ? (t.volume / t.baseVolume).clamp(0.0, 1.0)
                    : 0.0,
                max: 1.0,
                percent: true,
                dimmed: t.volume <= 0.0,
                onCommit: (v) => c.setTrackVolume(t.id, v * t.baseVolume),
              ),
            ],
          ],
        ],
      ),
    );
  }
}

/// Строка слайдера: локальный отклик во время драга, один коммит
/// в бэкенд по отпусканию. Иначе каждый тик драга — это 2–3 FFI
/// туда-обратно плюс перестройка всего экрана: на телефоне слайдер
/// вешает UI. Откат бэкенда (защита последнего слота) приезжает
/// сам через notifyListeners после коммита.
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

  /// Затемнение строки (группа/слот выключены стилем).
  final bool dimmed;

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
    // Внешнее значение (коммит/откат) — только вне драга,
    // иначе ползунок вырывают из-под пальца.
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
