# Continuum — Project Context

## What it is

Continuum is a generative music engine: a Flutter frontend on top of a Rust
synthesis engine. It produces endless 30–60 second pieces with a written form,
a finale, and a seam into the next piece. All sound is synthesized — no samples.

The goal is music that is **style-recognizable, alive, and dense** not by note
count but through an ensemble contract (anchors, pockets, budget, performance),
one riff foundation per seed, style transitions at seams, an instrument cast
from the style palette, and several written form variants.

## Structure

- `lib/`, `android/`, `ios/`, `linux/`, `macos/`, `windows/` — Flutter app.
- `rust/` — audio engine.
  - `rust/src/styles/` — styles and score generation:
    - `mod.rs` — `Style` trait, `RoleGuide`, shared `render_style`,
      `play_guide_with_riff`, groove poles, phrase breath, `Style::cast`,
      `signature_slots`, `arrangement_for_seed`, tests;
    - `cast.rs` — deterministic seed cast from the style palette;
    - `contract.rs` — `StyleContract` (bass/kick interlock, melodic onset budget,
      register ceiling, phrase velocity arc);
    - `riff.rs` — `RiffCell`/`RiffKind`: one riff foundation per seed;
    - `transition.rs` — style transitions at the "build → drop" seam;
    - `club.rs`, `lofi.rs` — guides, palettes, form variants, transitions, riff kind.
  - `rust/src/piece.rs` — written 30–60 s form: full arc, layers, proportional
    bar fitting, and the piece joint (`piece::Transition`; not to be confused
    with a style transition).
  - `rust/src/arranger.rs` — form, sections, energy, motif, measure context
    (`section_measure_index`, `previous_section`), per-seed form variant.
  - `rust/src/composer.rs` — bar cycle, timing, ties, visual snapshot.
  - `rust/src/instruments/` — synthesizers, DSP primitives, and `NoteEvent`.
  - `rust/src/engine.rs` — mixer, sidechain, reverb, limiter.
  - `rust/src/space.rs` — reverb/space.
  - `rust/src/playback.rs` — rendering, PCM16 export, live cast rebuild.
  - `rust/src/api/` — public FFI surface for Dart.
- `build/`, `rust/build/` — regenerable build artifacts (gitignored).
- `context.md` — this file.

## Design invariants

- Do not regress the sound approved at `f6c072f`: no timbre or mixer changes
  without A/B listening.
- One seed = one style riff foundation. The riff is a mask over existing voices,
  not a new layer. Do not restore the rejected separate lead.
- `Style::ensemble()` is a palette, not a whitelist. A cast must keep the
  foundation, signature slots, and transition slots.
- Every piece is a full `MainMotif → BuildUp → Drop → PostChorus` arc within
  30–60 s; sections are never truncated into fragments.
- Loudness comes only from `Slot::base_volume`, manual listener edits, and group
  faders. No generated loudness automation.
- Note length is a composition decision: it depends on instrument, style, and seed.
- A transition is a gesture at the seam, not a permanent section pad; the tail
  decays and slots are never duplicated.
- Live changes take effect from the next bar; a manually edited mixer list
  survives seed/style changes until reset.
- Phone-safe: bass/kick stay centered, no excessive low-end widening, CPU bounded.
- No samples: synthesis only.

## Current behavior

### Generation

- `styles::Harmony` defines per-seed mode, progression pool, harmonic rhythm,
  key shift, and density. `render_style` passes it to `play_guide_with_riff`
  and `transition::render`. The global `song.prog` remains a fallback.
- `Melody` is a data-driven phrase grid: onset accents rotate by section/visit,
  cell order is permuted/sequenced, echo voices are shifted by 1–2 eighths,
  and the cadence sounds even with an empty mask.
- `arranger::remix_for_cycle` varies `motif_b` and progression order per cycle.
- The riff has three seed mutations plus a pickup; chord snap can reach the
  7th/9th degree while cadences still resolve to the tonic.
- The Lead group (`Pluck`, `Guitar`, `GuitarLead`) is disabled in the product:
  forced to zero in `apply_style_mix`, hidden from `tracks()`/`groups()`, and
  rejected by `set_track`.
- Only two styles remain: **Club** (C minor, i–VI–iv–v, density 1.0) and
  **Lo-Fi** (D minor without the sixth, 4–8 bar chords, density 0.78).

### Sound

- Hat: classic 808-metal ratios (205/304/370/523/540/800 Hz) through a 10 kHz
  band-pass at 2× oversampling and a 7 kHz high-pass.
- Snare/clap: three body modes and a three-pulse 909 clap.
- Bass: no second-harmonic interference, independent filter envelope,
  keytracking, 15% saw in the presence path.
- Guitar: fractional Karplus–Strong delay plus allpass, pick-position comb,
  velocity from `to_bits()`, deterministic stereo. Pluck: 1.0 s tail and pitch envelope.
- Pad/Keys: per-voice LFO/detune, soft electronic key transient for Rhodes.
- Piano: ported from the earlier Continuum backend — three detuned strings,
  nine inharmonic partials with individual decay, hammer/key transients,
  a 34 Hz high-pass soundboard, and 9/820/0.18/1100 ms ADSR. New slot `Piano = 23`.
- Engine: PCG32 noise, 3 ms lookahead limiter, table `fast_tanh`, TPDF dither,
  sidechain driven by an 180 Hz low-passed kick envelope, `retune()` without
  a reverb tail drop, O(1) volume lookup.
- Space: cross-feedback, 20 ms predelay, modulated allpass.
- Two-point BLEP was measurably better than four-point at Nyquist; 2×
  oversampling is used only for hat metal.

### Live controls and UI

- Public API in `rust/src/api/playback.rs`: `playback_set_master_fx`,
  `playback_reset_master_fx`, `playback_set_tone_open`. `PlaybackStatus`
  carries current values (`reverb_wet`, `reverb_size`, `treble_hz`,
  `sidechain`, `tone_open`). Manual swing was removed; the style/seed swing
  is still used.
- Master FX live above style defaults and survive seed/style/session changes;
  reset also clears filter openness. Openness is a process-global atomic in
  `core.rs`; neutral value 0 keeps the authored path and test determinism.
- `ENGINE_VERSION = 2` (audible changes).
- The Dart bridge is regenerated with:
  `flutter_rust_bridge_codegen generate --config-file flutter_rust_bridge.yaml`.
  FRB cannot parse array lengths declared through `const`, so `VinylVoice`
  uses literal `[f32; 3]` / `[usize; 3]`.
- Mixer is available before Play; instrument volume is absolute 0–100%;
  the debug journal and manual swing control were removed; tempo/energy/sound
  reset buttons exist; the two reverb sliders were merged into one "Echo".

## Verification

```sh
cd rust
cargo test
cargo clippy --all-targets
cargo run -- club 42 --soak 12
cargo run -- lofi 7 --wav lofi.wav --secs 30
```

From the repository root:

```sh
flutter analyze
flutter test
flutter build apk --debug
```

Latest results: `cargo test` 247 lib + 6 bin (0 failures),
`cargo clippy --all-targets` 0 warnings, `flutter analyze` 0 issues,
`flutter test` 4/4, `flutter build apk --debug` successful.
Offline soak for Club/Lo-Fi: peak 0.595 / 0.558, `non-finite: 0`.

## Git and naming

- The Flutter package, Rust crate, Android application id, iOS/macOS bundle id,
  and displayed app name are all **Continuum**.
- Generated FFI files (`lib/src/rust/frb_generated*.dart`,
  `rust/src/frb_generated.rs`) are committed and must be regenerated whenever
  the public Rust API changes.
- Build outputs, caches, local properties, signing keys, and logs are ignored
  by `.gitignore`.
- The motif wheel visualizer was removed from the UI. Polling masks at 10 Hz
  made dots late and made every window wrap repaint the whole dial; a proper
  version needs note events streamed from Rust with timestamps. The backend
  `LiveVisual` snapshot remains inert and can support a future implementation.
- The rejected riff grammar is kept in the local branch `backup/riff-grammar`
  (pointing at `a9dc665`); the current approach does not use it.
