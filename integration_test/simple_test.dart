// Real FFI smoke test: after initialization the backend must return
// a valid status snapshot (style, seed code, engine version).

import 'package:flutter_test/flutter_test.dart';
import 'package:integration_test/integration_test.dart';

import 'package:continuum/src/rust/frb_generated.dart';
import 'package:continuum/src/rust/api/playback.dart';

void main() {
  IntegrationTestWidgetsFlutterBinding.ensureInitialized();

  setUpAll(() async {
    await RustLib.init();
  });

  testWidgets('backend responds with status without starting sound', (_) async {
    final status = await playbackStatus();
    expect(status.engineVersion, greaterThan(0));
    expect(status.styleName, isNotEmpty);
    expect(status.seedCode, matches(RegExp(r'^[0-9A-Z]{3}-[0-9A-Z]{3}$')));
    expect(status.running, isFalse);
  });

  testWidgets('style list is non-empty and contains club', (_) async {
    final styles = await playbackStyleNames();
    expect(styles, isNotEmpty);
    expect(styles, contains('club'));
  });
}
