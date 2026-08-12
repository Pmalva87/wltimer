import { api, todayStr, type DayPick } from "../api";
import { esc } from "./library";

const WEEKDAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
const MONTHS = [
  "January", "February", "March", "April", "May", "June",
  "July", "August", "September", "October", "November", "December",
];

/** `2026-08-12` → `Wed, 12 August`. */
function dayLabel(date: string): string {
  const d = new Date(`${date}T12:00:00`);
  return `${WEEKDAYS[(d.getDay() + 6) % 7]}, ${d.getDate()} ${MONTHS[d.getMonth()]}`;
}

/** The same date a month back, clamped by the browser's own date maths. */
function monthAgo(date: string): string {
  const d = new Date(`${date}T12:00:00`);
  d.setMonth(d.getMonth() - 1);
  return d.toISOString().slice(0, 10);
}

/**
 * Build a plan out of workouts already on the calendar — done or planned.
 *
 * The picker exists because a plan and a calendar day disagree about what a
 * date holds: pick exactly the workouts you want carried over, rather than
 * having a rule decide for you. The generated plan keeps each entry's id, so
 * fixing a day in the plan later updates the entry it came from.
 */
export async function renderPlanFrom(root: HTMLElement) {
  const today = todayStr();
  let from = monthAgo(today);
  let to = today;
  let entries: DayPick[] = [];
  /** Keys (`date:index`) of the entries that will go into the plan. */
  let picked = new Set<string>();

  const key = (e: DayPick) => `${e.date}:${e.index}`;

  function showStatus(msg: string, ok: boolean) {
    const el = root.querySelector<HTMLElement>("#pfstatus");
    if (!el) return;
    el.className = `editor-status ${ok ? "valid" : "invalid"}`;
    el.textContent = msg;
  }

  async function load() {
    try {
      entries = await api.listDayEntries(from, to);
      // Everything in range starts selected: trimming a list is less work than
      // building one, and the common case is "these weeks, as they happened".
      picked = new Set(entries.map(key));
    } catch (e) {
      entries = [];
      picked = new Set();
      showStatus(String(e), false);
    }
  }

  function entryRows(): string {
    if (entries.length === 0) {
      return `<div class="empty small">No workouts between those dates.</div>`;
    }
    let out = "";
    let day = "";
    for (const e of entries) {
      if (e.date !== day) {
        day = e.date;
        out += `<div class="pf-day">${dayLabel(e.date)}</div>`;
      }
      const done = e.status === "done";
      const when = e.completed_at
        ? ` · ${new Date(e.completed_at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`
        : "";
      out += `
        <label class="pf-row">
          <input type="checkbox" data-key="${key(e)}" ${picked.has(key(e)) ? "checked" : ""}>
          <span class="info">
            <span class="name">${esc(e.name)}</span>
            <span class="meta">${done ? `✓ done${when}` : "⧗ planned"}</span>
          </span>
        </label>`;
    }
    return out;
  }

  /** Redraw without losing what has been typed into the name field. */
  function rerender() {
    const name = root.querySelector<HTMLInputElement>("#pfname")?.value;
    render();
    if (name !== undefined) {
      root.querySelector<HTMLInputElement>("#pfname")!.value = name;
    }
  }

  function render() {
    root.innerHTML = `
      <div class="screen planfrom">
        <header class="topbar">
          <h1>Plan from calendar</h1>
          <a class="btn" href="#/library">Cancel</a>
        </header>
        <div class="pf-scroll">
          <input type="text" id="pfname" class="text-input" placeholder="Plan name"
                 value="${esc(`Plan from ${from}`)}">
          <div class="pf-range">
            <input type="date" id="pffrom" class="text-input" value="${from}">
            <span>→</span>
            <input type="date" id="pfto" class="text-input" value="${to}">
          </div>
          <div class="pf-actions">
            <button class="btn" id="pfall">Select all</button>
            <button class="btn" id="pfnone">Select none</button>
          </div>
          <div id="pfstatus" class="editor-status"></div>
          <div class="pf-list">${entryRows()}</div>
        </div>
        <div class="pf-footer">
          <button class="btn primary" id="pfcreate">Create plan (${picked.size})</button>
        </div>
      </div>`;

    root.querySelectorAll<HTMLInputElement>(".pf-row input").forEach((box) => {
      box.addEventListener("change", () => {
        if (box.checked) {
          picked.add(box.dataset.key!);
        } else {
          picked.delete(box.dataset.key!);
        }
        // Only the count changes; re-rendering would cost the name field.
        const create = root.querySelector<HTMLButtonElement>("#pfcreate");
        if (create) create.textContent = `Create plan (${picked.size})`;
      });
    });

    root.querySelector("#pffrom")!.addEventListener("change", (ev) => {
      from = (ev.target as HTMLInputElement).value;
      void load().then(rerender);
    });
    root.querySelector("#pfto")!.addEventListener("change", (ev) => {
      to = (ev.target as HTMLInputElement).value;
      void load().then(rerender);
    });

    root.querySelector("#pfall")!.addEventListener("click", () => {
      picked = new Set(entries.map(key));
      rerender();
    });
    root.querySelector("#pfnone")!.addEventListener("click", () => {
      picked = new Set();
      rerender();
    });

    root.querySelector("#pfcreate")!.addEventListener("click", async () => {
      const name = root.querySelector<HTMLInputElement>("#pfname")!.value;
      // Send them in calendar order, which is the order they will read in.
      const picks = entries
        .filter((e) => picked.has(key(e)))
        .map((e) => ({ date: e.date, index: e.index }));
      try {
        await api.createPlanFromDays(name, picks);
        // The new plan is at the top of the Plans list, which is where you
        // would go next to export or sync it.
        location.hash = "#/library";
      } catch (e) {
        const errs = e as { line: number; message: string }[];
        showStatus(Array.isArray(errs) && errs[0] ? errs[0].message : String(e), false);
      }
    });
  }

  await load();
  render();
}
