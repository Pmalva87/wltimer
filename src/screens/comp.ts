import {
  fmtKg,
  type CompSummary,
  type CompView,
  type Lift,
  type MarkView,
  type Qualification,
  type Reach,
  type TargetStatus,
  type TotalState,
} from "../api";
import { esc } from "./library";

/** One competition as a list row, shared by the Competitions screen. */
export function compRow(c: CompSummary): string {
  // A meet that no longer reads has no meet screen to delete it from, so its
  // row is the one place Delete still sits; a readable one deletes from inside.
  if (c.error) {
    return `<div class="workout broken">
              <a class="info tappable" href="#/compedit/${encodeURIComponent(c.slug)}">
                <span class="name">🏆 ${esc(c.name)}</span>
                <span class="meta error">${esc(c.error)}</span>
              </a>
              <div class="actions compact">
                <button class="btn danger compdelete" data-slug="${esc(c.slug)}">🗑 Delete</button>
              </div>
            </div>`;
  }
  const bits = [
    c.date ?? "no date",
    ...(c.organizer ? [`by ${esc(c.organizer)}`] : []),
    ...(c.orgs.length ? [esc(c.orgs.join(" · "))] : []),
    ...(c.age_group || c.category ? [esc([c.age_group, c.category].filter(Boolean).join(" "))] : []),
  ];
  // The two kinds of row a competition list holds: one you lifted at, and one
  // you are trying to get into.
  if (c.total.state === "made") {
    bits.push(`<strong>${c.total.total} total</strong>`);
  } else if (c.total.state === "bombed_out") {
    bits.push(`no total`);
  } else if (c.attempts_taken > 0) {
    bits.push(`${c.attempts_taken} attempt${c.attempts_taken === 1 ? "" : "s"} in`);
  }
  if (c.qualified) {
    // Met is met: what is left worth saying is by how much, and where.
    bits.push(
      `<span class="meta-ok">✓ qualified — best ${fmtKg(c.qualified.total)} at ${esc(c.qualified.meet)}${
        c.qualified.date ? ` (${fmtDay(c.qualified.date)})` : ""
      }</span>`,
    );
  } else if (c.standards > 0) {
    bits.push(`🎯 ${c.standards} mark${c.standards === 1 ? "" : "s"} to get in`);
  }
  if (!c.registered) {
    bits.push(`<span class="meta-warn">⚠ not registered</span>`);
  }
  return `<div class="workout">
            <a class="info tappable" href="#/comp/${encodeURIComponent(c.slug)}">
              <span class="name">🏆 ${esc(c.name)}</span>
              <span class="meta">${bits.join(" · ")}</span>
            </a>
          </div>`;
}

/** Totals to chase today. */
export function targetsSection(view: CompView): string {
  if (view.targets.length === 0) return "";
  return `
    <section class="view-part">
      <div class="view-part-head"><h2>Today</h2></div>
      ${view.targets
        .map((t) => markRow(esc(t.label) || `${fmtKg(t.total)} total`, t.total, t.status, t.reach, started(view), null))
        .join("")}
    </section>`;
}

/** Has the bar been touched yet? Before it has, "your next lifts" are the
 *  openers, and that is the word the screen should use. */
function started(view: CompView): boolean {
  const c = view.competition;
  return [...c.snatch.attempts, ...c.clean_jerk.attempts].some((a) => a && (a.result === "good" || a.result === "miss"));
}

export function totalPanel(view: CompView): string {
  const notes: string[] = [];
  if (view.next_total != null && view.total.state !== "bombed_out") {
    notes.push(
      started(view)
        ? `${fmtKg(view.next_total)} if you make your next lifts`
        : `${fmtKg(view.next_total)} with your openers`,
    );
  }
  if (view.best_possible_total != null && view.best_possible_total !== view.next_total) {
    notes.push(
      view.total.state === "made"
        ? `best possible ${fmtKg(view.best_possible_total)}`
        : `${fmtKg(view.best_possible_total)} if you make everything planned`,
    );
  }
  const possible = notes.map((n) => `<span class="comp-total-note">${n}</span>`).join("");
  return `
    <section class="comp-total ${view.total.state}">
      <span class="comp-total-label">Total</span>
      <span class="comp-total-value">${totalText(view.total)}</span>
      ${possible}
    </section>`;
}

function totalText(t: TotalState): string {
  if (t.state === "made") return fmtKg(t.total);
  // A bombed lift has no total and cannot get one; an open meet simply has not
  // got one yet. The screen must never show either as a zero.
  return t.state === "bombed_out" ? "no total" : "—";
}

/**
 * What each goal asks of one attempt, shown on the attempt itself: once a
 * mark comes down to a single number on a single lift, the attempt is where
 * you look, and "make this and you are in" is the whole message.
 */
export function attemptGoals(view: CompView, lift: Lift, attempt: number, written: number | null): string {
  const goals = [
    ...view.targets.map((t) => ({ label: esc(t.label) || `${fmtKg(t.total)} total`, status: t.status })),
    ...view.marks.map((m) => ({ label: `${esc(m.meet)}${m.label ? ` · ${esc(m.label)}` : ""}`, status: m.status })),
  ];
  return goals
    .map(({ label, status: s }) => {
      if (s.state !== "needs" || s.lift !== lift || s.attempt !== attempt) return "";
      if (written != null && written >= s.kg) {
        return `<div class="comp-goal">🎯 ${label} — make ${fmtKg(written)} and it is yours (needs ${fmtKg(s.kg)})</div>`;
      }
      const gap = written != null ? ` — ${fmtKg(s.kg - written)} more than written` : "";
      return `<div class="comp-goal">🎯 ${label} — needs ${fmtKg(s.kg)}${gap}</div>`;
    })
    .join("");
}

/** Qualifying marks at other meets that a total here could still win. */
export function marksSection(view: CompView): string {
  if (view.marks.length === 0) return "";
  // Lightest first: the next mark to clinch is the one worth reading first,
  // and the order is the order you will pass them in on the day.
  const marks: MarkView[] = [...view.marks].sort((a, b) => a.total - b.total);
  return `
    <section class="view-part">
      <div class="view-part-head"><h2>What this meet can win</h2></div>
      ${marks
        .map((m) =>
          markRow(
            `${esc(m.meet)}${m.label ? ` · ${esc(m.label)}` : ""}`,
            m.total,
            m.status,
            m.reach,
            started(view),
            group(m.age_group, m.category),
          ),
        )
        .join("")}
    </section>`;
}

/** `from`/`to` as a reader-facing range — either end may be open. */
export function windowText(q: Qualification): string | null {
  if (!q.from && !q.to) return null;
  if (q.from && q.to) return `Results from ${fmtDay(q.from)} to ${fmtDay(q.to)} count`;
  if (q.from) return `Results from ${fmtDay(q.from)} on count`;
  return `Results up to ${fmtDay(q.to!)} count`;
}

/** `label` arrives as HTML — every caller escapes its own text. */
function markRow(
  label: string,
  total: number,
  status: TargetStatus,
  reach: Reach | null,
  started: boolean,
  groupText: string | null,
): string {
  return `
    <div class="comp-mark st-${status.state}">
      <span class="comp-mark-total">${fmtKg(total)}</span>
      <span class="comp-mark-label">${label}${groupText ? ` <span class="muted">${esc(groupText)}</span>` : ""}</span>
      ${statusText(status)}
      ${reach ? reachText(reach, status, started) : ""}
    </div>`;
}

/** The plan against the mark — the line under the status that answers "and
 *  what I have written, does it get there?" */
function reachText(r: Reach, s: TargetStatus, started: boolean): string {
  switch (r.state) {
    case "next_lifts": {
      const what =
        s.state === "needs"
          ? `make ${s.lift === "snatch" ? "snatch" : "C&J"} ${s.attempt} as written`
          : started
            ? `make your next lifts (${fmtKg(r.total)})`
            : `make your openers (${fmtKg(r.total)})`;
      return `<span class="comp-mark-reach">reached if you ${what}</span>`;
    }
    case "plan":
      return `<span class="comp-mark-reach">needs more than your ${started ? "next lifts" : "openers"} — your plan gets it (${fmtKg(r.total)})</span>`;
    case "short":
      return `<span class="comp-mark-reach short">your plan (${fmtKg(r.total)}) is ${fmtKg(r.kg)} short</span>`;
  }
}

function statusText(s: TargetStatus): string {
  switch (s.state) {
    case "clinched":
      return `<span class="comp-mark-state clinched">✓ made</span>`;
    case "needs":
      return `<span class="comp-mark-state needs">need ${fmtKg(s.kg)} on ${
        s.lift === "snatch" ? "snatch" : "C&J"
      } ${s.attempt}</span>`;
    case "out_of_reach":
      return `<span class="comp-mark-state out">out of reach</span>`;
    default:
      // Both lifts still unfinished: what the bar needs depends on the other
      // one, so the target total is the only honest thing to show.
      return `<span class="comp-mark-state open">still possible</span>`;
  }
}

export function group(ageGroup: string | null, category: string | null): string | null {
  const parts = [ageGroup, category].filter(Boolean);
  return parts.length ? parts.join(" ") : null;
}

/** `2026-08-04` → `Tue, 4 Aug 2026`, split by hand so a bare date is not read
 *  as UTC and landed on the day before. */
export function fmtDay(date: string): string {
  const [y, m, d] = date.split("-").map(Number);
  return new Date(y, m - 1, d).toLocaleDateString([], {
    weekday: "short",
    day: "numeric",
    month: "short",
    year: "numeric",
  });
}
