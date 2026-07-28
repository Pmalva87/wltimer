# wltimer training-plan format

Write a multi-day training plan as a single markdown document in exactly this
format. The app imports such files directly ("Upload plan") and schedules one
workout per dated day on its calendar.

Rules:

- `# Plan Name` — exactly one, first heading in the file. Required.
- `## YYYY-MM-DD: Day Name` — one section per training day. The date is
  required, must be unique within the plan, and days need not be consecutive
  (skipped dates are rest days).
- `### Exercise Name` — the exercises of that day. Inside each exercise,
  bullet lines set parameters (all optional except `work`):
  - `intervals`: whole number >= 1 — how many work intervals (default 1)
  - `work`: duration of each work interval — required, greater than 0
  - `rest`: rest between intervals of this exercise
  - `rest after`: rest after this exercise, before the next one
  - `color`: screen color for work phases, `#rgb` or `#rrggbb`
- Durations are written as `M:SS` (e.g. `2:00`), `H:MM:SS`, or plain
  seconds (e.g. `90`).
- Every other line inside an exercise is free markdown shown on screen while
  it runs (cues, notes, target weights).
- Uploading a new version of a plan replaces all of its still-planned days
  from today onward; completed days are never touched.

Example:

# 5/3/1 — Cycle 1

## 2026-07-30: Heavy Squats
### Back Squat
- intervals: 5
- work: 2:00
- rest: 3:00

Brace hard, hit depth, drive up fast.

### Front Squat
- intervals: 3
- work: 1:30
- rest: 2:00

## 2026-08-01: Bench Day
### Bench Press
- intervals: 3
- work: 1:00
- rest: 0:45
