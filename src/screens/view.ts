import {
  api,
  fmtDuration,
  todayStr,
  PREPARE_SECS,
  type DayEntryInfo,
  type WorkoutView,
} from "../api";
import { esc } from "./library";

/**
 * Read-only view of a workout. `target` follows the builder's convention:
 * `<slug>` for a library workout, `@<date>:<index>` for a calendar entry.
 * Nothing here mutates anything — it is the screen to open when you want to
 * check what a workout is without risking an edit to it.
 */
export async function renderView(root: HTMLElement, target: string) {
  const dayMatch = target.match(/^@(\d{4}-\d{2}-\d{2}):(\d+)$/);
  const backHash = dayMatch ? `#/calendar/${dayMatch[1]}` : "#/library";
  const runHash = dayMatch ? `#/run/@${dayMatch[1]}:${dayMatch[2]}` : `#/run/${encodeURIComponent(target)}`;
  const editHash = dayMatch ? `#/edit/@${dayMatch[1]}:${dayMatch[2]}` : `#/edit/${encodeURIComponent(target)}`;

  let source: string;
  let entry: DayEntryInfo | undefined;
  try {
    if (dayMatch) {
      entry = (await api.getDay(dayMatch[1]))[Number(dayMatch[2])];
      source = entry?.markdown ?? "";
    } else {
      source = await api.getSource(target);
    }
  } catch (e) {
    root.innerHTML = errorScreen(backHash, editHash, String(e));
    return;
  }

  let view: WorkoutView;
  try {
    view = await api.viewWorkout(source);
  } catch (e) {
    const errs = e as { line: number; message: string }[];
    root.innerHTML = errorScreen(
      backHash,
      editHash,
      Array.isArray(errs) && errs[0] ? `line ${errs[0].line}: ${errs[0].message}` : String(e),
    );
    return;
  }

  // A finished workout is never re-run in place: Re-Run copies it onto today,
  // so the record of the day it was actually done stays untouched.
  const done = entry?.status === "done";
  root.innerHTML = `
    <div class="screen viewer">
      <header class="topbar">
        <a class="btn" href="${backHash}">‹ Back</a>
        <a class="btn" href="${editHash}">Edit</a>
        ${
          done
            ? `<button class="btn primary" id="rerun">↻ Re-Run</button>`
            : `<a class="btn primary" href="${runHash}">▶ Run</a>`
        }
      </header>
      <div class="view-scroll">
        <h1 class="view-title">${esc(view.name)}</h1>
        ${dayMatch ? dayLine(dayMatch[1], entry) : ""}
        <div class="view-id">
          <span class="view-id-label">ID</span>
          <code>${view.id ? esc(view.id) : "none yet — saving this workout adds one"}</code>
        </div>
        <div class="view-summary">
          <span>${view.blocks.length} part${view.blocks.length === 1 ? "" : "s"}</span>
          <span>${fmtDuration(view.total_secs)} total</span>
          <span class="muted">incl. ${PREPARE_SECS}s get-ready</span>
        </div>
        ${view.blocks.map(partCard).join("")}
      </div>
    </div>`;

  root.querySelector<HTMLButtonElement>("#rerun")?.addEventListener("click", async (ev) => {
    const btn = ev.currentTarget as HTMLButtonElement;
    btn.disabled = true;
    const today = todayStr();
    try {
      const idx = await api.repeatDayEntry(dayMatch![1], Number(dayMatch![2]), today);
      location.hash = `#/run/@${today}:${idx}`;
    } catch (e) {
      btn.disabled = false;
      const scroll = root.querySelector(".view-scroll");
      const msg = document.createElement("div");
      msg.className = "editor-status invalid";
      msg.textContent = `Could not schedule the repeat: ${String(e)}`;
      scroll?.prepend(msg);
    }
  });
}

/** The date the workout sits on, plus whether it has already been done. Only
 *  calendar entries have either — a library template is undated by nature. */
function dayLine(date: string, entry?: DayEntryInfo): string {
  const at = entry?.completed_at
    ? ` at ${new Date(entry.completed_at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`
    : "";
  const mark =
    entry?.status === "done"
      ? `<span class="view-mark done">✓ Done${at}</span>`
      : `<span class="view-mark planned">⧗ Planned</span>`;
  return `<div class="view-day"><span class="view-date">📅 ${fmtDay(date)}</span>${mark}</div>`;
}

/** `2026-08-04` → `Tue, 4 Aug 2026`. Split by hand rather than
 *  `new Date(date)`, which reads a bare date as UTC and can land on the day
 *  before in western timezones. */
function fmtDay(date: string): string {
  const [y, m, d] = date.split("-").map(Number);
  return new Date(y, m - 1, d).toLocaleDateString([], {
    weekday: "short",
    day: "numeric",
    month: "short",
    year: "numeric",
  });
}

function partCard(b: WorkoutView["blocks"][number], i: number): string {
  const rows = [
    ["Intervals", String(b.intervals)],
    ["Work", fmtDuration(b.work_secs)],
    ["Rest", b.rest_secs ? fmtDuration(b.rest_secs) : "none"],
  ];
  if (b.rest_after_secs) {
    rows.push(["Rest after part", fmtDuration(b.rest_after_secs)]);
  }
  return `
    <section class="view-part"${b.color ? ` style="border-left-color:${esc(b.color)}"` : ""}>
      <div class="view-part-head">
        <h2>${i + 1}. ${esc(b.name)}</h2>
        <span class="view-part-total">${fmtDuration(b.block_secs)}</span>
      </div>
      <dl class="view-rows">
        ${rows.map(([k, v]) => `<div class="view-row"><dt>${k}</dt><dd>${v}</dd></div>`).join("")}
      </dl>
      ${
        b.no_rest_after
          ? `<div class="part-warn">⚠ Runs straight into the next part — no rest in between</div>`
          : ""
      }
      ${b.description_html ? `<div class="view-notes">${b.description_html}</div>` : ""}
    </section>`;
}

/// The view is the only route to the editor, so a workout that will not parse
/// must still offer Edit here — otherwise a broken file could never be fixed.
function errorScreen(backHash: string, editHash: string, message: string): string {
  return `
    <div class="screen viewer">
      <header class="topbar">
        <a class="btn" href="${backHash}">‹ Back</a>
        <h1>Cannot show workout</h1>
        <a class="btn primary" href="${editHash}">Edit</a>
      </header>
      <div class="view-scroll">
        <div class="editor-status invalid">${esc(message)}</div>
      </div>
    </div>`;
}
