# App artwork contract

Put files here with exact names below — code picks them up automatically
(no code changes needed). Missing file = black placeholder, app won't crash.

Styles and shelves: Club — club, house, techno, trance, dubstep, dnb,
electro, rave; Rock — poprock, hardrock, metal; Chill — lofi.

## Backgrounds (one per style, portrait-friendly)

- `bg_club.png` — dark tense club, strobe smoke
- `bg_house.png` — warm parquet dancefloor
- `bg_techno.png` — dark industrial hall, single strobe
- `bg_trance.png` — euphoric sunrise over crowd, lasers
- `bg_dubstep.png` — dark bass machinery, heavy smoke
- `bg_dnb.png` — night city, fast headlights
- `bg_electro.png` — neon grid, fat saw waves
- `bg_rave.png` — oldschool warehouse, smiley, strobes
- `bg_poprock.png` — sunny garage band stage
- `bg_hardrock.png` — dark arena, wall of amps
- `bg_metal.png` — black forge, sparks
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
