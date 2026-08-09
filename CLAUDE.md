# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

An Android interval timer for weightlifting ("Weightlifting Timer" on the
launcher, package `com.pcorreia.wltimer`), built as a Rust + Tauri 2 app with a
vanilla-TypeScript frontend. `README.md` documents the user-facing behaviour and
the markdown workout format; `INSTALL.md` covers sideloading. Read the README's
"Workout format" and "Identity" sections before touching the parser or any
store — the format is the on-disk schema and the interchange format at once.

## Commands

```sh
cargo test -p wltimer-core                # the real test suite (~85 unit tests)
cargo test -p wltimer-core parser::       # one module
cargo test -p wltimer-core restore_clamps # one test, by name substring
cargo clippy --workspace --all-targets -- -D warnings
npm run build                             # tsc && vite build — the only check src/ gets
npm run tauri dev                         # desktop preview; needs libwebkit2gtk-4.1-dev librsvg2-dev
```

CI (`.github/workflows/ci.yml`) runs `cargo test --workspace --locked`, clippy
with `-D warnings`, and `npm run build`. The Rust toolchain is **pinned to
1.93.1 in the workflow, deliberately not in a `rust-toolchain.toml`** (that
would strand the locally installed Android targets); bump it by hand alongside
a local `rustup update`.

There is no frontend test suite. Changes to `src/` are only type-checked, so
they need a run through the actual app.

Android build (toolchain paths per README/INSTALL):

```sh
export JAVA_HOME=~/Android/jdk-17.0.20+8 ANDROID_HOME=~/Android/sdk
export NDK_HOME=$ANDROID_HOME/ndk/27.2.12479018
npm run android:apk    # tauri android build + scripts/copy-apk.mjs → wltimer-<version>.apk
```

`src-tauri/gen/android` is generated but **committed with two manual patches**,
both in `MainActivity.kt`: `FLAG_KEEP_SCREEN_ON` (set for the whole app, so the
display never sleeps mid-workout) and the `FileSaver` JavaScript bridge that
exposes `window.WltimerFiles` for saving markdown through Android's system file
picker (`src/files.ts` falls back to a browser download without it).
Regenerating that directory silently drops both.

## Architecture

Three layers, strictly one-directional:

- **`core/`** — pure Rust, no Tauri dependency. Data model, markdown
  parser/serializer, timer engine, and all four on-disk stores. Everything
  worth testing lives here and is unit-tested in-file.
- **`src-tauri/`** — thin shell. `lib.rs` builds `AppState` (engine behind a
  `Mutex`, the stores, the current `RunOrigin`) and registers ~35 commands;
  `commands.rs` is the whole API surface plus the 200 ms ticker.
- **`src/`** — vanilla TS + Vite. `main.ts` is a hash router
  (`#/quick`, `#/library`, `#/calendar`, `#/view/<slug>`, `#/edit/<slug>`,
  `#/run/<target>`); each `screens/*.ts` exports a `render…` returning a
  cleanup function. `api.ts` mirrors every command's Rust types by hand — a
  change to a command's serialized shape must be mirrored there.

### The timer

`Workout::flatten()` (`core/src/model.rs`) turns a workout into a flat
`Vec<Phase>` (Prepare → Work/Rest per interval → BlockRest). `Engine`
(`core/src/engine.rs`) walks that vector against `Instant`s — it holds no
timer of its own. The backend ticker in `commands.rs::spawn_ticker` calls
`engine.advance(now)` every 200 ms and emits `timer:tick` (a `Snapshot`) plus
one `timer:cue` per `Cue` (3-2-1 pre-alerts, phase starts, finish). The
frontend listens to both and owns all sound and vibration; the backend is
silent. The run screen gets its static data once via a `RunPlan`, so ticks
carry only indices and remaining milliseconds.

`after_cues` is where side effects hang off the tick: crossing into a new phase
rewrites the saved session, and `Cue::Finished` records the run on the calendar
and clears it.

### Sessions and `RunOrigin`

At most one session exists at a time (`core/src/session.rs`). It embeds the
whole `Workout`, so resuming does not depend on the library or calendar entry
still existing, and it is scoped to its local date — a session from yesterday
is dropped and the workout starts over. `Engine::restore` clamps a
`phase_idx` past the end, so a session saved against a since-edited workout
still resumes.

`RunOrigin` is the one piece of state that decides what finishing a run *means*:
a `Day` origin flips that calendar entry to done, `Library`/`Adhoc` append a new
done entry. It also produces the `#/run/<target>` route string.

### Identity and version (`core/src/ids.rs`, `core/src/time.rs`)

Every stored document carries two preamble bullets under its title: `- id:` (a
UUID) and `- updated:` (when it last changed). Both live in the markdown so
they survive leaving the device, where a filename and an mtime do not. Reading
is tolerant — hand-written files need neither, and a malformed value reads as
absent rather than stranding the file — but writing is not: the stores call
`ensure_id` and `set_updated` so nothing reaches disk without both.

The id is what makes re-import update rather than duplicate, and what lets a
plan sync match the days it scheduled last time — including recognising a day
you already finished, which a sync must never replace. Copies (scheduling a
template, promoting a day entry, recording a finished run) mint a *new* id.
**When adding a code path that writes a workout, decide explicitly whether it
preserves the id or mints one.** The timestamp needs no such care: every write
overwrites it, which is also why it is not on the `Workout` model — it never
has to survive an edit round-trip, and a second copy in the model could drift
from the document. Calendar entries follow the same rule for the same reason
(`days.rs:60`).

`core` cannot read the wall clock — `chrono` is pulled in without its `clock`
feature — so every store write takes a `now` from the shell, the way `Engine`
takes `Instant`s. `commands.rs::now()` is the single source. Timestamps are
canonical UTC at second precision so that string order is chronological order;
`time::canonical` normalises anything else on the way in.

The migrations in each store's `new()` read file mtimes **before** running,
because the id backfill rewrites files and would otherwise stamp the whole
library with the moment of the upgrade. Calendar entries never inherit their
own date — a workout planned for next month would take a stamp in the future
and beat every later edit.

### Storage

Four stores, all under the app-data dir, all zstd-compressed via `core/src/zio.rs`
(which sniffs the magic bytes on read, so plain files still load — that is how
legacy `.md` files migrate):

| Store | Path | Contents |
| --- | --- | --- |
| `store.rs` | `workouts/<slug>.md.zst` | library templates, markdown |
| `days.rs` | `days/<YYYY-MM-DD>.json.zst` | calendar entries, each with its own markdown copy |
| `plan.rs` | `plans/<slug>.md.zst` | multi-day training plans |
| `session.rs` | app-data root | the single in-flight session |

Calendar entries embed a full markdown copy rather than referencing a template,
so day entries are independent of the library.

## Conventions

Comments here explain *why*, not what — the non-obvious constraint or the
decision that was weighed (see `zio.rs`, `ids.rs`, `ci.yml`). Match that; don't
add narration of the code below it.

Commit subjects are sentence-case and describe the user-visible change, not the
implementation: "Keep Pause and Skip on screen when the phone is turned
sideways", "Show the next part's cues instead of the whole workout ahead".
