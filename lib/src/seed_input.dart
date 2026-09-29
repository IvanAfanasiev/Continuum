import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

/// 6 ячеек ввода сида в духе Telegram-кода: работают как один инпут.
/// Печать — фокус вперед, стирание пустой — назад, вставка мусора режется.
class SeedInput extends StatefulWidget {
  const SeedInput({
    super.key,
    required this.initialCode,
    required this.onSubmit,
    required this.onDice,
    this.compact = false,
  });

  /// 6 символов без дефиса для предзаполнения.
  final String initialCode;
  final void Function(String code) onSubmit;
  final VoidCallback onDice;

  /// Компактный вид для телефонов: ячейки меньше заголовка стиля.
  final bool compact;

  @override
  State<SeedInput> createState() => _SeedInputState();
}

class _SeedInputState extends State<SeedInput> {
  late final List<TextEditingController> _controllers;
  late final List<FocusNode> _nodes;
  late final List<String> _prev;

  @override
  void initState() {
    super.initState();
    _controllers = List.generate(6, (_) => TextEditingController());
    _nodes = List.generate(6, (_) => FocusNode());
    _prev = List.filled(6, '');
    _fill(widget.initialCode);
  }

  @override
  void didUpdateWidget(SeedInput oldWidget) {
    super.didUpdateWidget(oldWidget);
    if (oldWidget.initialCode != widget.initialCode &&
        !_nodes.any((n) => n.hasFocus)) {
      // Внешнее обновление (кубик) — только когда не печатаем.
      _fill(widget.initialCode);
    }
  }

  void _fill(String code) {
    final clean = code.toUpperCase().replaceAll(RegExp('[^0-9A-Z]'), '');
    for (var i = 0; i < 6; i++) {
      final ch = i < clean.length ? clean[i] : '';
      _controllers[i].text = ch;
      _prev[i] = ch;
    }
    setState(() {});
  }

  void _onChanged(int index, String value) {
    final was = _prev[index];
    if (value.length > 1) {
      // Вставка: берем последний символ.
      _controllers[index].text = value[value.length - 1];
    }
    final now = _controllers[index].text;
    _prev[index] = now;
    if (now.isNotEmpty && was.isEmpty && index < 5) {
      _nodes[index + 1].requestFocus();
    } else if (now.isEmpty && was.isNotEmpty && index > 0) {
      _nodes[index - 1].requestFocus();
    }
    final code = _controllers.map((c) => c.text).join();
    if (code.length == 6) {
      _nodes[index].unfocus();
      widget.onSubmit(code);
    }
    setState(() {});
  }

  @override
  void dispose() {
    for (final c in _controllers) {
      c.dispose();
    }
    for (final n in _nodes) {
      n.dispose();
    }
    super.dispose();
  }

  @override
  Widget build(BuildContext context) {
    final scheme = Theme.of(context).colorScheme;
    // На телефоне инпут не должен спорить с названием стиля.
    final boxSize = widget.compact ? 38.0 : 48.0;
    final boxHeight = widget.compact ? 46.0 : 56.0;
    final fontSize = widget.compact ? 17.0 : 20.0;
    final cells = <Widget>[];
    for (var i = 0; i < 6; i++) {
      cells.add(
        SizedBox(
          width: boxSize,
          height: boxHeight,
          child: TextField(
            controller: _controllers[i],
            focusNode: _nodes[i],
            textAlign: TextAlign.center,
            textAlignVertical: TextAlignVertical.center,
            // Тап мимо ячеек гасит фокус: клавиатура не всплывает
            // от последнего выбранного слота.
            onTapOutside: (_) => FocusScope.of(context).unfocus(),
            style: TextStyle(fontSize: fontSize, fontWeight: FontWeight.w600),
            maxLength: 2,
            buildCounter: (context,
                    {required currentLength,
                    required isFocused,
                    maxLength}) =>
                null,
            inputFormatters: [
              UpperCaseFormatter(),
              FilteringTextInputFormatter.allow(RegExp('[0-9A-Za-z]')),
            ],
            decoration: InputDecoration(
              filled: true,
              fillColor: scheme.surfaceContainerHighest,
              // Ноль справа/сверху/снизу + 1px слева: компенсация
              // оптики глифов — иначе символ сидит чуть левее центра.
              contentPadding: const EdgeInsets.only(left: 1),
              border: OutlineInputBorder(
                borderRadius: BorderRadius.circular(12),
                borderSide: BorderSide.none,
              ),
              focusedBorder: OutlineInputBorder(
                borderRadius: BorderRadius.circular(12),
                borderSide: BorderSide(color: scheme.primary, width: 2),
              ),
              counterText: '',
            ),
            onChanged: (v) => _onChanged(i, v),
          ),
        ),
      );
      if (i == 2) {
        cells.add(
          const Padding(
            padding: EdgeInsets.symmetric(horizontal: 4),
            child: Text('-', style: TextStyle(fontSize: 20)),
          ),
        );
      } else if (i < 5) {
        cells.add(const SizedBox(width: 6));
      }
    }
    cells.add(const SizedBox(width: 8));
    cells.add(
      IconButton(
        tooltip: 'Случайный сид',
        onPressed: widget.onDice,
        icon: const Icon(Icons.casino_outlined),
      ),
    );
    return Row(
      mainAxisAlignment: MainAxisAlignment.center,
      children: cells,
    );
  }
}

/// Верхний регистр прямо при вводе.
class UpperCaseFormatter extends TextInputFormatter {
  @override
  TextEditingValue formatEditUpdate(
    TextEditingValue oldValue,
    TextEditingValue newValue,
  ) {
    return newValue.copyWith(text: newValue.text.toUpperCase());
  }
}
