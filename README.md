# wltimer

Weightlifting interval timer for Android, built with Rust + Tauri 2.

A workout is a sequence of **blocks** (exercises), each with its own number of
intervals, work time, optional rest between intervals, and optional rest after
the block. Workouts are written in markdown, saved on the device, and the
markdown prose of each block is shown on screen while it runs. Work and rest
phases get distinct colors, with beeps + amber flashes at 3‑2‑1 before every
transition and distinct sounds for work/rest starts.

## Workout format

```markdown
# Monday Squats

## Back Squat
- intervals: 5
- work: 2:00
- rest: 1:30
- rest after: 3:00
- color: #e11d48

Cues: brace hard, hit depth, drive up fast.

## Bench Press
- intervals: 3
- work: 1:00
```

- `# Title` — workout name (required)
- `## Heading` — starts an exercise block (at least one required)
- Bullet params per block: `work` (required), `intervals` (default 1), `rest`,
  `rest after`, `color`. Times are seconds (`90`) or `M:SS` (`1:30`).
- Everything else in the block is free markdown shown during the exercise.

## Project layout

- `core/` — pure Rust crate: data model, markdown→workout parser, timer engine
  (all unit-tested; no Tauri dependency)
- `src-tauri/` — Tauri shell: file store (`.md` files in app data), commands,
  200 ms ticker emitting `timer:tick` / `timer:cue` events
- `src/` — frontend (vanilla TypeScript + Vite): library, editor, run screens;
  WebAudio beeps and vibration

## Develop

```sh
cargo test -p wltimer-core   # core unit tests
npm install
npm run tauri dev            # desktop preview (needs libwebkit2gtk-4.1-dev librsvg2-dev)
```

## Build for Android

Requires: JDK 17, Android SDK + NDK, Rust target `aarch64-linux-android`.

```sh
export JAVA_HOME=~/Android/jdk-17.0.20+8
export ANDROID_HOME=~/Android/sdk
export NDK_HOME=$ANDROID_HOME/ndk/27.2.12479018
npm run tauri android build -- --apk --target aarch64
adb install src-tauri/gen/android/app/build/outputs/apk/universal/release/app-universal-release.apk
```

The generated Android project (`src-tauri/gen/android`) carries one manual
patch: `FLAG_KEEP_SCREEN_ON` in `MainActivity.kt`, so the screen stays awake
during a workout.
