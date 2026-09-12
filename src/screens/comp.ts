import {
  api,
  fmtKg,
  type CompSummary,
  type CompView,
  type LiftEntry,
  type MarkView,
  type StandardView,
  type TargetStatus,
  type TotalState,
} from "../api";
import { esc } from "./library";

/** One competition as a list row, shared by the Competitions screen. */
export function compRow(c: CompSummary): string {
  if (c.error) {
    return `<div class="workout broken">
              <a class="info tappable" href="#/compedit/${encodeURIComponent(c.slug)}">
                <span class="name">🏆 ${esc(c.name)}</span>
                <span class="meta error">${esc(c.error)}</span>
              </a>
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
  if (c.standards > 0) {
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
            <div class="actions compact">
              <a class="btn primary" href="#/comp/${encodeURIComponent(c.slug)}">👁 View</a>
              <a class="btn" href="#/compedit/${encodeURIComponent(c.slug)}">✎ Edit</a>
            </div>
          </div>`;
}

/**
 * A competition, read-only. Everything on this screen is either written in the
 * document or computed from it — what the meet totals, what it could still
 * total, and which qualifying marks it is able to answer.
 */
export async function renderComp(root: HTMLElement, slug: string) {
  let view: CompView;
  try {
    view = await api.viewCompetition(slug);
  } catch (e) {
    root.innerHTML = `
      <div class="screen viewer">
        <header class="topbar">
          <a class="btn" href="#/library">‹ Back</a>
          <h1>Cannot show competition</h1>
          <a class="btn primary" href="#/compedit/${encodeURIComponent(slug)}">Edit</a>
        </header>
        <div class="view-scroll"><div class="editor-status invalid">${esc(String(e))}</div></div>
      </div>`;
    return;
  }

  const c = view.competition;
  const meta = [
    c.date ? `📅 ${fmtDay(c.date)}` : null,
    c.organizer ? `organized by ${esc(c.organizer)}` : null,
    c.orgs.length ? esc(c.orgs.join(" · ")) : null,
    c.category ? esc(c.category) : null,
    c.age_group ? esc(c.age_group) : null,
    c.bodyweight != null ? `${fmtKg(c.bodyweight)} kg bw` : null,
    view.registered ? null : `<span class="meta-warn">⚠ not registered</span>`,
  ].filter(Boolean);

  root.innerHTML = `
    <div class="screen viewer">
      <header class="topbar">
        <a class="btn" href="#/library">‹ Back</a>
        <a class="btn primary" href="#/compedit/${encodeURIComponent(slug)}">Edit</a>
      </header>
      <div class="view-scroll">
        <h1 class="view-title">🏆 ${esc(c.name)}</h1>
        ${meta.length ? `<div class="comp-meta">${meta.map((m) => `<span>${m}</span>`).join("")}</div>` : ""}
        ${totalPanel(view)}
        ${hasData(c.snatch) ? liftCard("Snatch", c.snatch, view.snatch_best, view.snatch_going_down, view.snatch_notes_html) : ""}
        ${hasData(c.clean_jerk) ? liftCard("Clean & Jerk", c.clean_jerk, view.clean_jerk_best, view.clean_jerk_going_down, view.clean_jerk_notes_html) : ""}
        ${
          view.targets.length
            ? `<section class="view-part">
                 <div class="view-part-head"><h2>Today</h2></div>
                 ${view.targets
                   .map((t) => markRow(esc(t.label) || `${fmtKg(t.total)} total`, t.total, t.status, null))
                   .join("")}
               </section>`
            : ""
        }
        ${marksSection(view.marks)}
        ${standardsSection(view.standards, c.age_group, c.category)}
      </div>
    </div>`;
}

/** Is there anything on this lift worth a card — an attempt, or a warmup? */
function hasData(entry: LiftEntry): boolean {
  return entry.attempts.some((a) => a !== null) || entry.warmup.length > 0;
}

function totalPanel(view: CompView): string {
  const possible =
    view.best_possible_total != null && view.total.state !== "made"
      ? `<span class="comp-total-note">${fmtKg(view.best_possible_total)} if you make what is declared</span>`
      : view.best_possible_total != null && view.total.state === "made"
        ? `<span class="comp-total-note">best possible ${fmtKg(view.best_possible_total)}</span>`
        : "";
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

function liftCard(
  name: string,
  entry: LiftEntry,
  best: number | null,
  goingDown: number[],
  notesHtml: string,
): string {
  const attempts = entry.attempts
    .map((a, i) => {
      const n = i + 1;
      if (!a) {
        return `<div class="comp-attempt empty"><span class="comp-attempt-n">${n}</span><span class="comp-attempt-kg">—</span></div>`;
      }
      const mark =
        a.result === "good"
          ? `<span class="comp-result good">✓ good lift</span>`
          : a.result === "miss"
            ? `<span class="comp-result miss">✗ no lift</span>`
            : `<span class="comp-result declared">declared</span>`;
      const warn = goingDown.includes(n) ? `<span class="meta-warn">⚠ lighter than the one before</span>` : "";
      return `<div class="comp-attempt ${a.result}">
                <span class="comp-attempt-n">${n}</span>
                <span class="comp-attempt-kg">${fmtKg(a.kg)}</span>
                ${mark}${warn}
              </div>`;
    })
    .join("");
  const warmup = entry.warmup.length
    ? `<div class="comp-warmup">${entry.warmup
        .map(
          (w) =>
            `<span class="comp-set ${w.done ? "done" : ""}">${w.done ? "✓" : "○"} ${fmtKg(w.kg)}${
              w.reps > 1 ? ` × ${w.reps}` : ""
            }</span>`,
        )
        .join("")}</div>`
    : "";
  return `
    <section class="view-part">
      <div class="view-part-head">
        <h2>${name}</h2>
        <span class="view-part-total">${best != null ? `best ${fmtKg(best)}` : "—"}</span>
      </div>
      ${warmup}
      <div class="comp-attempts">${attempts}</div>
      ${notesHtml ? `<div class="view-notes">${notesHtml}</div>` : ""}
    </section>`;
}

/** Qualifying marks at other meets that a total here could still win. */
function marksSection(marks: MarkView[]): string {
  if (marks.length === 0) return "";
  return `
    <section class="view-part">
      <div class="view-part-head"><h2>What this meet can win</h2></div>
      ${marks
        .map((m) =>
          markRow(
            `${esc(m.meet)}${m.label ? ` · ${esc(m.label)}` : ""}`,
            m.total,
            m.status,
            group(m.age_group, m.category),
          ),
        )
        .join("")}
    </section>`;
}

/** This meet's own entry standards, and what has already answered them. */
function standardsSection(
  standards: StandardView[],
  ageGroup: string | null,
  category: string | null,
): string {
  if (standards.length === 0) return "";
  const said = ageGroup || category;
  return `
    <section class="view-part">
      <div class="view-part-head"><h2>Entry standard</h2></div>
      ${standards
        .map((s) => {
          const label = [group(s.age_group, s.category), s.label].filter(Boolean).join(" · ");
          const met = s.met_by
            ? `<span class="comp-mark-state clinched">✓ ${fmtKg(s.met_total ?? 0)} at ${esc(s.met_by)}${
                s.met_on ? ` (${fmtDay(s.met_on)})` : ""
              }${s.met_category ? ` · ${esc(s.met_category)}` : ""}</span>`
            : `<span class="comp-mark-state open">not yet</span>`;
          return `<div class="comp-mark ${s.yours ? "yours" : "other"}">
                    <span class="comp-mark-total">${fmtKg(s.total)}</span>
                    <span class="comp-mark-label">${esc(label) || "everyone"}${
                      said && s.yours ? ` <span class="comp-yours">yours</span>` : ""
                    }</span>
                    ${met}
                  </div>`;
        })
        .join("")}
    </section>`;
}

/** `label` arrives as HTML — every caller escapes its own text. */
function markRow(label: string, total: number, status: TargetStatus, groupText: string | null): string {
  return `
    <div class="comp-mark">
      <span class="comp-mark-total">${fmtKg(total)}</span>
      <span class="comp-mark-label">${label}${groupText ? ` <span class="muted">${esc(groupText)}</span>` : ""}</span>
      ${statusText(status)}
    </div>`;
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
      return `<span class="comp-mark-state open">still open</span>`;
  }
}

function group(ageGroup: string | null, category: string | null): string | null {
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
