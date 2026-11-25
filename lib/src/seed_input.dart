import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

/// Six seed input boxes in the spirit of a Telegram code: they work as one input.
/// Typing moves focus forward, clearing an empty box moves back, junk paste is cut.
class SeedInput extends StatefulWidget {
  const SeedInput({
    super.key,
    required this.initialCode,
    required this.onSubmit,
    required this.onDice,
    this.onPrevious,
    this.compact = false,
  });

  /// 6 characters without the dash for prefill.
  final String initialCode;
  final void Function(String code) onSubmit;
  final VoidCallback onDice;
  final VoidCallback? onPrevious;

  /// Compact layout for phones: boxes smaller than the style title.
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
      // External update (dice) — only when not typing.
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
      // Paste: take the last character.
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
    // On a phone the input shouldn't compete with the style title.
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
            // Tapping outside the boxes clears focus: the keyboard won't pop up
            // from the last selected slot.
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
              // Zero right/top/bottom + 1px left: glyph optical compensation —
              // otherwise the character sits slightly left of center.
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
    cells.add(const SizedBox(width: 4));
    if (widget.onPrevious != null) {
      cells.add(
        IconButton(
          tooltip: 'Previous seed',
          onPressed: widget.onPrevious,
          icon: const Icon(Icons.undo),
        ),
      );
    }
    cells.add(
      IconButton(
        tooltip: 'Random seed',
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

/// Uppercase as you type.
class UpperCaseFormatter extends TextInputFormatter {
  @override
  TextEditingValue formatEditUpdate(
    TextEditingValue oldValue,
    TextEditingValue newValue,
  ) {
    return newValue.copyWith(text: newValue.text.toUpperCase());
  }
}
