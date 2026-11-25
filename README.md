# Continuum

Continuum is an endless generative music player. It does not play recorded
tracks and does not use samples: every note, rhythm, chord, instrument part and
sound wave is produced by mathematical algorithms in real time. No AI models,
no prerecorded audio files - just a deterministic music engine running live.

## The Idea

A six-character seed (`XXX-XXX`) is the score. It deterministically selects the
motif, harmonic progression, groove patterns, instrument cast and arrangement
variant, so the same seed always produces the same evolving composition while a
new seed produces a different one.

Generation works on several layers:

- **Deterministic randomness** — `splitmix64`/PCG streams derived from
  `(seed, slot, step, phrase)`, so nothing depends on wall-clock time.
- **Harmony and melody** — style-specific scales and progression pools, a seed
  motif with a protected hook and cadence, phrase-level development and a
  secondary theme for climaxes.
- **Rhythm** — per-slot step weights and groove poles, a bass/kick interlock,
  an onset budget and phrase velocity arcs that keep the mix from clogging.
- **Form** — written 30–60 second pieces with a full
  `Theme → Build-up → Drop → Post-chorus` arc and a seamless joint into the
  next piece, so playback can continue indefinitely.
- **Ensemble** — an instrument cast picked from a style palette; foundational
  voices always remain, optional colors are chosen by the seed.

The composer decides once per sixteenth note; the audio engine renders every
sample, scheduling notes at exact media frames with swing and a due queue.
Synthesis is subtractive and sample-free: oscillators, filters, envelopes,
saturation, sidechain, reverb and a master limiter. Live controls take effect
from the next bar without restarting playback.

Two styles are available: **Club** and **Lo-Fi**, each with its own mode,
progressions, rhythmic vocabulary, form and instrument palette.

## User Interface

- Select a style and enter or randomize a seed code; step back to compare two seeds.
- Play and pause the generative stream.
- Mix and tempo controls in the left drawer, style catalog in the right drawer.
- Circular artwork as the screen background; system media notification and
  audio focus on Android.

## Controls

| Where | What |
|---|---|
| Center | Play/Pause, seed, dice, previous seed |
| Left drawer "Mix" | Tempo, Energy, Sound, instrument levels, reset buttons |
| Right drawer "Styles" | Style catalog with artwork, sort by category/alphabet |

- `Tempo` speeds up or slows down generation without changing the style.
- `Energy` changes density and intensity of the arrangement.
- `Sound` holds echo, treble cutoff, sidechain depth and filter openness.
- Instrument volume is absolute 0–100%; zero excludes the instrument from
  generation and DSP.
- Reset buttons restore the style default for tempo, energy, the sound section
  or the instrument list.

## Tech Stack

- **Rust** — composition, synthesis, audio rendering and runtime controls
- **CPAL** — desktop audio output
- **Flutter / Dart** — desktop and mobile UI
- **Android Kotlin** — native `AudioTrack` output, audio focus, media session
  and notification
- **JNI** — bridge between Android Kotlin and the Rust audio thread
- **flutter_rust_bridge** — typed FFI layer between Dart and Rust

## Installation

Download the latest build for your platform from
[the releases page](../../releases/latest). Install the package, then launch
Continuum — no additional audio packs or sample libraries are required.

Building from source is described below.

## Running The Backend

From `rust/`:

```sh
cargo test
cargo clippy --all-targets

cargo run -- club 42 --soak 12
cargo run -- lofi 7 --render 64
cargo run -- lofi 7 --wav lofi.wav --secs 30
```

`--soak` prints offline metrics without an audio device: peak, RMS, note count
and the number of non-finite samples (must be zero).

## Running The Frontend

From the repository root:

```sh
flutter pub get
flutter run
```

Desktop builds look for the native library in `rust/target/release/`,
`rust/target/debug/` and next to the executable. A release library can be built
with `cd rust && cargo build --release`.

Android debug APK:

```sh
flutter build apk --debug
```

Checks:

```sh
flutter analyze
flutter test
```

If the public API in `rust/src/api/` changes, regenerate the Dart bridge with
`flutter_rust_bridge_codegen generate --config-file flutter_rust_bridge.yaml`.

Design notes and invariants are in `context.md`.

## License

Continuum is distributed under the Business Source License 1.1. The source is
public and may be copied, modified and used for personal, non-commercial
purposes; commercial use requires a separate license. On the Change Date
(2029-11-25) the code becomes available under Apache-2.0. See `LICENSE`.
