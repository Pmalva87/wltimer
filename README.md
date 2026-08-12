# wltimer — Weightlifting Timer

Weightlifting interval timer for Android ("Weightlifting Timer" on the
launcher), built with Rust + Tauri 2.

A workout is a sequence of **parts** (exercises), each with its own number of
intervals, work time, optional rest between intervals, and optional rest after
the part. Workouts are built in a single form-based **builder** (steppers for
times/intervals, optional per-part names and notes/cues) which can START a
one-off timer immediately or Save to the library. Markdown is the storage and
interchange format: upload a `.md` file to populate the builder, flip the
"Markdown" toggle to edit the same workout as text, or Copy a workout's
markdown to the clipboard to export it. Tapping a workout — in the library or on
the calendar — opens a **read-only view** listing its parts, times and notes,
with its id at the top. Editing is reached from there rather than from the
list, so it is never a mis-tap away.
Notes are shown on screen while the
part runs. Work and rest phases get distinct colors, with beeps + amber
flashes at 3‑2‑1 before every transition, distinct sounds for work/rest
starts, and a large whole-workout countdown at the top of the run screen.
Through every rest, and through a part's **last interval**, the next part's
cues (which is where loads tend to be written) lead that panel in a box of
their own, above the cues for what is being done, and the line under the timer
reads "next: block rest, then Bench Press". The bar can then be reloaded the
moment it is racked, which matters most when a part has no rest after it and
there is no later chance to read them. Ordinary work intervals stay bare: the
current cues, nothing else.

Leaving the run screen mid-workout — or the phone killing the app — does not
throw the workout away: it is kept as a **paused session**, and starting that
workout again offers RESUME — picking up in the same phase, at the same second
— alongside "Start over". A session only lives for its day; the next morning
the workout simply starts from the top.

A part with no rest after it runs straight into the next one, which is far more
often a slip than a choice, so it is flagged in amber wherever the workout is
shown: under that part in the builder (live, as you change the rest) and in the
read-only view, as a chip on its row in the workout list and on the calendar,
and as a one-line heads-up on the start screen. It is only ever a warning — the
workout saves and runs exactly as written.

## Workout format

```markdown
# Monday Squats
- id: 9f2c8e1a-4b7d-4c2e-9a11-6f0d3e5b8c74

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
- `- id:` — a UUID under the title identifying this workout. Optional in a file
  you write by hand; the app adds one when it saves. See [Identity](#identity).
- `- updated:` — when the document last changed, UTC (`2026-08-09T13:45:31Z`).
  App-managed: rewritten on every save, and nothing you need to write by hand.
  See [Identity](#identity).
- `## Heading` — starts an exercise block (at least one required)
- Bullet params per block: `work` (required), `intervals` (default 1), `rest`,
  `rest after`, `color`. Times are seconds (`90`) or `M:SS` (`1:30`).
- Everything else in the block is free markdown shown during the exercise.

## Identity

Every stored workout carries a UUID in its own markdown, so identity travels
with the file rather than with its filename or its position in a list. It also
carries the time it last changed, as `- updated:`, for the same reason and in
the same place: a file that leaves the phone loses its filesystem timestamp, so
the only version marker that survives an export is one written inside the
document.

Identity answers "is this the same workout"; the timestamp answers what
identity cannot — "is this copy newer than the one already here". Comparing two
copies is a plain string comparison, because every stamp is written in one
canonical form (UTC, whole seconds) chosen so that alphabetical order *is*
chronological order.

What identity buys you:

- Re-importing a workout you exported and edited elsewhere **updates the
  original** instead of adding "Squats (2)". A file with no id is treated as a
  new workout, so hand-written `.md` uploads still work as before.
- Plan days carry one id each, inherited by the calendar entry they schedule.
  Re-syncing a plan matches on that id, so it updates its days rather than
  duplicating them — even if you re-upload the plan file as a new plan.
- **A workout you have already finished is never replaced or duplicated by a
  sync.** The done entry stands; no planned copy appears beside it.
- Copies get their own identity: scheduling a library template on a date,
  promoting a day entry into the library, and recording a finished run all mint
  a new id, so no two objects ever claim the same one.

Workouts, plans and calendar entries saved before ids existed are given one
automatically the first time the app opens after upgrading. The same pass fills
in missing timestamps, from the best evidence still on disk: a finished
workout's own completion time, and otherwise the file's last-modified date.
Anything with no evidence at all is left unstamped rather than given a guessed
date — an absent stamp reads as "oldest", so it loses a comparison instead of
winning one it has not earned.

## Calendar

The 📅 calendar schedules workouts on dates: upload a `.md`, pick from the
library (a **copy** — day entries are independent of templates), or build one
in place. Finishing any run records it on the day you did it (planned entries
flip to done with a timestamp; library/one-off runs append a done entry). A
workout you finish on a different day than it was planned for **moves to the
day you did it**, keeping its id — the calendar records what happened, not what
was scheduled, and a plan re-sync still recognises the day as finished. Entries
can be moved between days and explicitly promoted into the library.

## Backup

Everything the app holds lives in its private app-data directory, which no
file manager can reach and no `adb` command can pull off a release build. The
only way your workouts exist anywhere but this phone is to export them.

**Workouts → Backup → ⇩ Back up all** writes one `.md` file —
`wltimer-backup-2026-08-09.md` — containing every library workout, every plan,
and every calendar entry, done and planned. On Android that opens the system
save dialog, so picking Drive, Dropbox or Nextcloud as the destination puts it
off the phone in one action. The name carries the date, so successive backups
sit beside each other in the folder instead of replacing one another.

**↺ Restore** reads one back. So does either 📂 Upload button — a backup file
is recognised by what it is, not by which button you used.

What a restore will and will not do:

- **It never duplicates.** Every document is matched by the `- id:` it carries,
  so restoring the same file twice changes nothing the second time.
- **It never overwrites something newer.** A document whose `- updated:` stamp
  is older than the copy already on the phone is left alone and counted as
  "already up to date".
- **It never replaces a workout you have finished.** A done calendar entry is
  the record of what you actually did; nothing in a backup can overwrite it.
  A *planned* entry, though, is happily replaced by the finished version of
  itself — that is how a rebuilt phone gets its history back.
- **It never deletes.** Anything on the phone but absent from the backup stays.
  Restoring merges; it does not reset the app to the backup's state.
- **Restored documents keep their own timestamps.** A restore is not an edit,
  so it does not stamp your whole library with the moment you restored it.
- **A restored plan does not re-sync the calendar.** The backup's day entries
  are the record of what was scheduled, and re-planning on top of them would
  overwrite it.

Nothing is written until the whole file has parsed, so a damaged backup fails
with a line number instead of half-restoring.

The format is the same markdown as everything else, with each document
introduced by a comment line naming what it is:

```markdown
<!-- wltimer:backup exported=2026-08-09T13:45:31Z -->

<!-- wltimer:workout -->
# Monday Squats
- id: 9f2c8e1a-4b7d-4c2e-9a11-6f0d3e5b8c74
...

<!-- wltimer:day date=2026-08-09 status=done completed=2026-08-09T18:22:10Z -->
# Monday Squats
- id: 4b7d3e5b-8c74-4c2e-9a11-6f0d3e5b8c74
...
```

Each section holds its document unchanged, so a backup can be read, edited and
re-imported like any other `.md` — and a section can simply be cut out of one
and uploaded on its own.

## Storage

All app data lives in the private app-data directory, zstd-compressed
per file:

```
workouts/<slug>.md.zst        # library templates: the markdown document
days/<YYYY-MM-DD>.json.zst    # calendar: JSON array of entries, each with its
                              # own markdown copy + status/completed_at/source
```

Legacy plain `.md` library files migrate automatically at startup.

## Project layout

- `core/` — pure Rust crate: data model, markdown↔workout parser/serializer,
  timer engine (all unit-tested; no Tauri dependency)
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

CI (`.github/workflows/ci.yml`) runs on every push to `main` and every pull
request: `cargo test --workspace` plus `cargo clippy -- -D warnings`, and
`npm run build` (`tsc && vite build`) for the frontend. There is no automated
test for the UI in `src/` — only the type-check — so frontend changes still
need a run through the app.

## Build for Android

Requires: JDK 17, Android SDK + NDK, Rust target `aarch64-linux-android`.

```sh
export JAVA_HOME=~/Android/jdk-17.0.20+8
export ANDROID_HOME=~/Android/sdk
export NDK_HOME=$ANDROID_HOME/ndk/27.2.12479018
npm run android:apk
adb install src-tauri/gen/android/app/build/outputs/apk/universal/release/wltimer-<version>.apk
```

`npm run android:apk` builds the APK and copies it alongside the original as
`wltimer-<version>.apk` (version from `package.json`), so each build has a
distinct filename — useful when sideloading via a cloud share, since some
downloaders/browsers reuse a cached file for a name they've already fetched.

The generated Android project (`src-tauri/gen/android`) carries two manual
patches, both in `MainActivity.kt`:

- `FLAG_KEEP_SCREEN_ON`, so the screen stays awake during a workout.
- a `FileSaver` JavaScript bridge exposing `window.WltimerFiles`, so exporting
  a `.md` goes through Android's system file picker. Without it `src/files.ts`
  falls back to its anchor-download path, which is meant for the desktop
  preview and gives the phone no way to place the file.

Regenerating that directory drops both, silently.

## Install on a phone

See [INSTALL.md](INSTALL.md) — sideloading over `adb` or by copying the APK to
the phone, plus signing/upgrade rules and troubleshooting.
