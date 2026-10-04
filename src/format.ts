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
dated section as a workout on its calendar — or, when the section is marked as
a competition, adds it to the app's competitions instead.

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
  - Give a genuinely new day a new UUID.
- \`- deleted: true\` — under a day's heading, in place of its exercises: a
  request to **remove that day** from the plan. It needs the day's \`- id:\` to
  say which day, and nothing else — no \`###\` sections, since there is no
  workout left to describe. A marker naming a day the plan does not have is
  ignored, and removing every day of a plan is refused (delete the plan
  instead). Never write one into a plan you are authoring; it only makes sense
  in a file uploaded to change a plan that already exists.
- A file need not contain the whole plan. Uploading one holding only the days
  you changed updates exactly those and leaves every other day standing, which
  is the normal way to fix a plan: add days by writing new sections, change
  them by editing theirs, remove them with \`- deleted: true\`. Days are matched
  by id alone — a section with no id can only be new, so it is added rather
  than replacing anything.
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
- \`- kind: competition\` — under a day's heading, alongside its \`- id:\`: this
  day is a **meet**, not a workout. It goes to the app's competitions, not onto
  the calendar as training. Write it as a competition document one heading
  level deeper — \`### Snatch\` and \`### Clean & Jerk\` holding
  \`- <n>: <kg> planned\` attempts and \`- [ ] <kg> x <reps>\` warmup lines
  (optionally under \`#### Warmup\`) — plus any of \`- bodyweight:\`,
  \`- category:\`, \`- age group:\`, \`- org:\`, \`- target:\` under the heading.
  The heading's date is the meet's date. Once its warmup has begun on the
  phone, a meet is never changed by a plan again.
- Re-importing a plan matches each day to what it scheduled before, by id, from
  today onward: edited days are updated in place, and re-dated days move and
  keep their history. A day you have already completed is never replaced and
  never duplicated. Dates in the past are left alone entirely.

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

## 2026-11-14: Nationals
- id: 7d2e9c40-1f3b-4a6d-8e5c-9b0a1c2d3e4f
- kind: competition
- category: 89 kg
### Snatch
- 1: 95 planned
- 2: 99 planned
- 3: 102 planned
#### Warmup
- [ ] 20 x 5
- [ ] 60 x 2
- [ ] 85 x 1

### Clean & Jerk
- 1: 120 planned

Example of a fix — upload this to change only these two days of the plan
above, dropping the squats and reworking the bench. Everything the file does
not mention is left exactly as it is:

# 5/3/1 — Cycle 1
- id: 0f2c8b76-4d31-4a55-9e17-83c6b0a4f2de

## 2026-07-30: Heavy Squats
- id: 3c1f7a92-8e04-4b1d-90aa-2d7c65f0e1b3
- deleted: true

## 2026-08-01: Bench Day
- id: b57e0d41-6a92-4f38-8c05-1e9b4a72dd60
### Bench Press
- intervals: 5
- work: 1:00
- rest: 1:00
`;

/** The competition-format specification, exportable like the other two so a
 * meet can be written off the phone and uploaded. */
export const COMP_FORMAT_GUIDE = `# wltimer competition format

Write a weightlifting meet as a single markdown document in exactly this
format. The app imports it through any upload button under Workouts — it
recognises a meet by its \`- kind: competition\` line, whichever button you
use. To put a meet inside a training plan instead, see the plan format guide:
it is the same document one heading level deeper.

Rules:

- \`# Meet Name\` — exactly one, first heading in the file. Required.
- \`- kind: competition\` — directly under the title. Required: it is what
  makes the file a meet rather than a workout.
- \`- id: <uuid>\` — app-managed. Omit it when writing a new meet; **keep it
  exactly as it is** when revising one exported from the app, so the upload
  updates that meet instead of adding a second copy. An upload replaces the
  meet as it stands on the phone, ticked warmups and results included.
- \`- updated: <timestamp>\` — app-managed; leave it alone or omit it.
- Optional bullets under the title, each at most once unless noted:
  - \`date\`: \`YYYY-MM-DD\`
  - \`bodyweight\`: in kg, e.g. \`88.4\`
  - \`category\`: the weight class you enter, free text, e.g. \`89 kg\`
  - \`age group\`: e.g. \`M40\`, \`Senior\`, \`Junior\`
  - \`org\`: who sanctions the meet, comma-separated, e.g. \`BWL, IWF\` (may repeat)
  - \`organizer\`: who runs it
  - \`target\`: a total you want on the day, number first, e.g. \`230 today\`
    (may repeat)
  - \`registered\`: \`yes\` or \`no\`
- \`## Snatch\` and \`## Clean & Jerk\` — one section per lift, each optional.
  - Attempts: \`- 1: <kg>\`, \`- 2: <kg>\`, \`- 3: <kg>\`, each followed by a
    result word or nothing:
    - \`planned\` — a weight you have not told the table yet
    - nothing — declared, not yet lifted
    - \`good\` / \`miss\` — taken
  - Warmup sets: checkbox lines \`- [ ] <kg> x <reps>\` (reps default to 1;
    \`- [x]\` marks a set done). Anywhere in the lift's section; a
    \`### Warmup\` heading above them is optional. Write them lightest first.
  - Any other line in a lift's section is free notes shown with that lift.
- Weights are kilograms, whole or decimal (\`42.5\`).
- \`## Qualification\` — optional: the entry standard of a meet you are
  chasing, which your other meets' totals are checked against.
  - \`- from: YYYY-MM-DD\`, \`- to: YYYY-MM-DD\` — the window a total must be
    set in.
  - \`- counts: BWL, FPH\` — whose meets count.
  - \`- needs: <kg>\` — the total required. For a table (e.g. masters), one
    \`### <age group> <class>\` row per standard, each with its own
    \`- needs:\`.

Example:

# Nationals 2026
- kind: competition
- date: 2026-11-14
- bodyweight: 88.4
- category: 89 kg
- org: BWL
- age group: M40
- target: 230 today

## Snatch
- 1: 95 planned
- 2: 99 planned
- 3: 102 planned

### Warmup
- [ ] 20 x 5
- [ ] 50 x 3
- [ ] 70 x 2
- [ ] 85 x 1

## Clean & Jerk
- 1: 120 planned
- 2: 125 planned
- 3: 128 planned

### Warmup
- [ ] 60 x 3
- [ ] 90 x 2
- [ ] 110 x 1

Example of a meet you are chasing:

# Europeans 2027
- kind: competition
- date: 2027-05-20
- age group: M40
- category: 89 kg

## Qualification
- from: 2026-06-01
- to: 2027-03-31
- counts: BWL, IWF

### M35 89 kg
- needs: 215

### M40 89 kg
- needs: 205
`;
