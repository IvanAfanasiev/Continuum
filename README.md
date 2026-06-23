# Continuum

Continuum is an endless generative music player: choose a style, press play, and let the composition keep evolving continuously.
No AI. No prerecorded tracks. Just algorithms.

Continuum combines:
- Markov-based note generation
- Phrase-level planning
- Groove coordination between instruments
- Real-time synthesis
- Multi-layer arrangement
Every instrument follows a shared musical structure while keeping enough variation to avoid repetition.

## What It Does

Continuum generates music in real time. The app does not play fixed audio files;
it builds notes, rhythms, chords, instrument parts, and sound waves while it is running.

Currently includes multiple procedural music styles such as Jazz and Ambient.
Additional styles are planned.

## User Interface

Features:

- Select a music style
- Control tempo and swing
- Adjust instrument volumes
- Play and pause generation

### Controls

Open the bottom sheet to change:

- `Tempo` - speeds up or slows down generation without changing the selected style.
- `Swing` - changes the rhythmic feel.
- Instrument volumes - adjusts the relative mix of instruments used by the current preset.

## Tech Stack

- Rust - composition, synthesis, audio rendering, runtime controls.
- CPAL - desktop audio output.
- Flutter - desktop and mobile UI.
- Android Kotlin - mobile audio output, `AudioTrack`, audio focus, media session, media notification.
- C++ JNI - thin bridge between Android Kotlin and Rust.
- Cargo workspace - shared backend package and build configuration.

## Running The Backend

From the repository root:
```
cargo run -p continuum -- jazz
cargo run -p continuum -- ambient
cargo run -p continuum -- list
```

Check desktop backend:
```
cargo check -p continuum
```

Check the mobile-oriented Rust library:
```
cargo check -p continuum --lib --no-default-features --target aarch64-linux-android
```

Format Rust:
```
cargo fmt -p continuum
```

## Running The Frontend

From the repository root:
```
cd frontend
flutter pub get
flutter run -d windows
```

Run on Android:
```
cd frontend
flutter run -d <android-device-id>
```

Build Android APK:
```
cd frontend
flutter build apk
```

Build only arm64 Android APK:
```
cd frontend
flutter build apk --target-platform android-arm64
```

Build Windows:
```
cd frontend
flutter build windows
```

`flutter run` is a temporary debug/run command.

`flutter build windows` creates a release folder:
```
frontend/build/windows/x64/runner/Release/
```

## License

See `LICENSE`.
