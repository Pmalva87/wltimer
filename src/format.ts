/** The workout-format specification, exportable as a .md file so it can be
 * given to an LLM (or a human) to author workouts the app can import. */
export const FORMAT_GUIDE = `# wltimer workout format

Write a workout as a single markdown document in exactly this format. The app
imports such files directly (Upload .md).

Rules:

- \`# Title\` — exactly one, first heading in the file; names the workout. Required.
- \`- id: <uuid>\` — directly under the title, before the first \`##\`. A random
  UUID identifying this workout.
  - Writing a **new** workout: generate a fresh random UUID.
  - Editing a workout that **already has one**: keep it exactly as it is. The id
    is how the app recognises the workout, so importing the edited file updates
    the existing workout instead of adding a second copy of it.
  - Deliberately making a **copy** to keep alongside the original: give it a new
    UUID, or the import will overwrite the original.
  - If you omit it entirely the file still imports, and the app assigns an id on
    save — but then it imports as a brand-new workout every time.
- \`- updated: <timestamp>\` — app-managed; leave it alone. It records when the
  document last changed, as UTC RFC 3339 (\`2026-08-09T13:45:31Z\`), and the app
  rewrites it on every save. Omit it when writing a new workout by hand.
- \`## Heading\` — starts an exercise block. At least one block is required.
- Inside a block, bullet lines of the form \`- key: value\` set parameters.
  Recognized keys (all optional except \`work\`):
  - \`intervals\`: whole number >= 1 — how many work intervals (default 1)
  - \`work\`: duration of each work interval — required, greater than 0
  - \`rest\`: rest between intervals of this exercise
  - \`rest after\`: rest after this exercise, before the next one
  - \`color\`: screen color for work phases, \`#rgb\` or \`#rrggbb\`
- Durations are written as \`M:SS\` (e.g. \`2:00\`), \`H:MM:SS\`, or plain
  seconds (e.g. \`90\`).
- Every other line inside a block is free markdown shown on screen while that
  exercise runs (cues, notes, target weights).
- The timer automatically adds a 10-second "get ready" countdown at the start.

Example:

# Monday Squats
- id: 9f2c8e1a-4b7d-4c2e-9a11-6f0d3e5b8c74

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
`;

/** The training-plan format specification, exportable as a .md file so it can
 * be given to an LLM (or a human) to author multi-day plans the app imports. */
export const PLAN_FORMAT_GUIDE = `# wltimer training-plan format

Write a multi-day training plan as a single markdown document in exactly this
format. The app imports such files directly ("Upload plan") and schedules each
dated section as a workout on its calendar.

Rules:

- \`# Plan Name\` — exactly one, first heading in the file. Required.
- \`- id: <uuid>\` — app-managed; leave it alone. Sits under the plan title and
  identifies the plan file itself, which is how re-uploading a corrected plan
  updates the existing one instead of creating a second copy. Omit it when
  writing a new plan by hand; **keep it exactly as it is** when revising a plan
  exported from the app.
- \`- updated: <timestamp>\` — app-managed; leave it alone. Sits under the plan
  title, records when the plan file last changed as UTC RFC 3339
  (\`2026-08-09T13:45:31Z\`), and is rewritten on every save. Omit it when
  writing a new plan by hand.
- \`## YYYY-MM-DD: Day Name\` — one section per workout. The date is required
  and days need not be consecutive (skipped dates are rest days). A date **may
  repeat**: two sections on one date schedule two workouts that day, in the
  order written.
- \`- id: <uuid>\` — directly under each day's heading, before its first
  \`###\`. A random UUID, different for every day in the plan.
  - Writing a **new** plan: generate a fresh UUID per day.
  - Revising an **existing** plan: keep each day's id exactly as it is, including
    when you change that day's exercises or move it to another date. The id is
    how the app recognises the day it already scheduled, so the calendar entry
    is updated or moved rather than duplicated.
  - Give a genuinely new day a new UUID. Deleting a day from the file removes its
    still-planned entry from the calendar.
- \`### Exercise Name\` — the exercises of that day. Inside each exercise,
  bullet lines set parameters (all optional except \`work\`):
  - \`intervals\`: whole number >= 1 — how many work intervals (default 1)
  - \`work\`: duration of each work interval — required, greater than 0
  - \`rest\`: rest between intervals of this exercise
  - \`rest after\`: rest after this exercise, before the next one
  - \`color\`: screen color for work phases, \`#rgb\` or \`#rrggbb\`
- Durations are written as \`M:SS\` (e.g. \`2:00\`), \`H:MM:SS\`, or plain
  seconds (e.g. \`90\`).
- Every other line inside an exercise is free markdown shown on screen while
  it runs (cues, notes, target weights).
- Re-importing a plan matches each day to what it scheduled before, by id, from
  today onward: edited days are updated in place, re-dated days move and keep
  their history, and days dropped from the file are unscheduled. A day you have
  already completed is never replaced and never duplicated. Dates in the past
  are left alone entirely.

Example:

# 5/3/1 — Cycle 1

## 2026-07-30: Heavy Squats
- id: 3c1f7a92-8e04-4b1d-90aa-2d7c65f0e1b3
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
- id: b57e0d41-6a92-4f38-8c05-1e9b4a72dd60
### Bench Press
- intervals: 3
- work: 1:00
- rest: 0:45
`;
