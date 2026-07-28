# wltimer workout format

Write a workout as a single markdown document in exactly this format. The app
imports such files directly (Upload .md).

Rules:

- `# Title` — exactly one, first heading in the file; names the workout. Required.
- `## Heading` — starts an exercise block. At least one block is required.
- Inside a block, bullet lines of the form `- key: value` set parameters.
  Recognized keys (all optional except `work`):
  - `intervals`: whole number >= 1 — how many work intervals (default 1)
  - `work`: duration of each work interval — required, greater than 0
  - `rest`: rest between intervals of this exercise
  - `rest after`: rest after this exercise, before the next one
  - `color`: screen color for work phases, `#rgb` or `#rrggbb`
- Durations are written as `M:SS` (e.g. `2:00`), `H:MM:SS`, or plain
  seconds (e.g. `90`).
- Every other line inside a block is free markdown shown on screen while that
  exercise runs (cues, notes, target weights).
- The timer automatically adds a 10-second "get ready" countdown at the start.

Example:

# Monday Squats

## Back Squat
- intervals: 5
- work: 2:00
- rest: 1:30
- rest after: 3:00

Brace hard, hit depth, drive up fast.

## Bench Press
- intervals: 3
- work: 1:00
- rest: 0:45
