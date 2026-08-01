import { api, fmtDuration, PREPARE_SECS, type WorkoutView } from "../api";
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
  try {
    source = dayMatch
      ? ((await api.getDay(dayMatch[1]))[Number(dayMatch[2])]?.markdown ?? "")
      : await api.getSource(target);
  } catch (e) {
    root.innerHTML = errorScreen(backHash, String(e));
    return;
  }

  let view: WorkoutView;
  try {
    view = await api.viewWorkout(source);
  } catch (e) {
    const errs = e as { line: number; message: string }[];
    root.innerHTML = errorScreen(
      backHash,
      Array.isArray(errs) && errs[0] ? `line ${errs[0].line}: ${errs[0].message}` : String(e),
    );
    return;
  }

  root.innerHTML = `
    <div class="screen viewer">
      <header class="topbar">
        <a class="btn" href="${backHash}">‹ Back</a>
        <h1>${esc(view.name)}</h1>
        <a class="btn" href="${editHash}">Edit</a>
        <a class="btn primary" href="${runHash}">▶ Run</a>
      </header>
      <div class="view-scroll">
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
      ${b.description_html ? `<div class="view-notes">${b.description_html}</div>` : ""}
    </section>`;
}

function errorScreen(backHash: string, message: string): string {
  return `
    <div class="screen viewer">
      <header class="topbar">
        <a class="btn" href="${backHash}">‹ Back</a>
        <h1>Cannot show workout</h1>
      </header>
      <div class="view-scroll">
        <div class="editor-status invalid">${esc(message)}</div>
      </div>
    </div>`;
}
