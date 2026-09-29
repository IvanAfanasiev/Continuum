# App artwork contract

Put files here with exact names below — code picks them up automatically
(no code changes needed). Missing file = black placeholder, app won't crash.

Styles (3): club (Club), rock (Rock), lofi (Chill).

## Backgrounds (one per style, portrait-friendly)

- `bg_club.png` — hard beatmaker: 909, sub, acid squelch
- `bg_rock.png` — mechanical concert: wall of fuzz
- `bg_lofi.png` — warm dusty room, sunset window

Used in: `lib/src/player_screen.dart` → `StyleBackground`
(full-screen cover + 45% dark veil for text contrast).

## Style previews (small squares in the style list)

- `style_<name>.png` for each name above
  (recommended 256x256, same art as backgrounds, cropped)

Used in: `StylePreview` widget.

## App icon

- `app_icon.png` — 1024x1024 master artwork.
  After placing it, run: `dart run flutter_launcher_icons`
  (config `flutter_icons:` is already in pubspec.yaml).
