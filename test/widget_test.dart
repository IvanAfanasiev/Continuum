// Smoke tests for the player's pure Dart helpers. They don't start Rust and don't
// require an audio device — they run in a plain `flutter test`.

import 'package:flutter_test/flutter_test.dart';

import 'package:continuum/src/player_controller.dart';

void main() {
  group('seedCodeToBigInt', () {
    test('XXX-XXX code — a number in base 36', () {
      expect(seedCodeToBigInt('000000'), BigInt.zero);
      expect(seedCodeToBigInt('000001'), BigInt.one);
      expect(seedCodeToBigInt('00000Z'), BigInt.from(35));
      expect(seedCodeToBigInt('000010'), BigInt.from(36));
    });

    test('case does not matter, garbage is skipped', () {
      expect(seedCodeToBigInt('ab-c!d'), seedCodeToBigInt('ABCD'));
      expect(seedCodeToBigInt(''), BigInt.zero);
    });

    test('full code fits in CODE_SPACE = 36^6', () {
      final max = seedCodeToBigInt('ZZZZZZ');
      expect(max, BigInt.from(36).pow(6) - BigInt.one);
    });

    test('code and number are inverse conversions', () {
      for (final n in [BigInt.zero, BigInt.from(42), BigInt.from(424242)]) {
        expect(seedCodeToBigInt(seedCodeFromBigInt(n)), n);
      }
      expect(seedCodeFromBigInt(BigInt.from(424242)), '0093CI');
    });
  });
}
