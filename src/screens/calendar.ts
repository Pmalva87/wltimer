import { api, fmtDuration, todayStr, type DayEntryInfo, type ParseError } from "../api";
import { copyText } from "../clipboard";
import { tabBar } from "../tabs";
import { esc } from "./library";

const MONTHS = [
  "January", "February", "March", "April", "May", "June",
  "July", "August", "September", "October", "November", "December",
];
const WEEKDAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

function pad(n: number): string {
  return String(n).padStart(2, "0");
}

export async function renderCalendar(root: HTMLElement, dateArg: string | null) {
  const today = todayStr();
  const selected = dateArg && /^\d{4}-\d{2}-\d{2}$/.test(dateArg) ? dateArg : today;
  let [viewYear, viewMonth] = selected.split("-").map(Number);
  let movingIdx: number | null = null;
  let showLibraryPicker = false;

  function select(date: string) {
    const target = `#/calendar/${date}`;
    if (location.hash === target) {
      void render();
    } else {
      location.hash = target;
    }
  }

  function showStatus(msg: string, ok: boolean) {
    const el = root.querySelector<HTMLElement>("#daystatus");
    if (!el) return;
    el.className = `editor-status ${ok ? "valid" : "invalid"}`;
    el.textContent = msg;
  }

  function entryCard(e: DayEntryInfo, i: number, totalSecs: number | null): string {
    const done = e.status === "done";
    const when = e.completed_at
      ? ` · ${new Date(e.completed_at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}`
      : "";
    const duration = totalSecs != null ? ` · ${fmtDuration(totalSecs)}` : "";
    return `
      <div class="day-entry ${done ? "done" : ""}">
        <a class="info tappable" href="#/view/@${selected}:${i}">
          <span class="name">${esc(e.name)}</span>
          <span class="meta">${done ? `✓ done${when}` : "⧗ planned"}${duration}${e.source_slug ? ` · from library` : ""}${e.source_plan ? ` · plan: ${esc(e.source_plan)}` : ""}</span>
        </a>
        ${
          movingIdx === i
            ? `<div class="move-row">
                 <input type="date" id="movedate" class="text-input" value="${selected}">
                 <button class="btn primary" id="moveok" data-i="${i}">Move</button>
                 <button class="btn" id="movecancel">Cancel</button>
               </div>`
            : `<div class="actions">
                 <a class="btn primary" href="#/run/@${selected}:${i}">▶ Run</a>
                 <a class="btn" href="#/view/@${selected}:${i}">👁 View</a>
                 <a class="btn" href="#/edit/@${selected}:${i}">Edit</a>
                 <button class="btn copy" data-i="${i}">⧉ Copy to clipboard</button>
                 <button class="btn promote" data-i="${i}">☆ Save to library</button>
                 <button class="btn move" data-i="${i}">⇄ Move</button>
                 <button class="btn danger delete" data-i="${i}">Remove</button>
               </div>`
        }
      </div>`;
  }

  async function render() {
    const summaries = await api.getMonth(viewYear, viewMonth);
    const byDate = new Map(summaries.map((s) => [s.date, s.entries]));
    const entries: DayEntryInfo[] = await api.getDay(selected).catch(() => []);
    const entryTotals = await Promise.all(
      entries.map((e) =>
        api.parsePreview(e.markdown).then((p) => (p.status === "ok" ? p.total_secs : null)),
      ),
    );
    const plannedTotal = entries.reduce(
      (sum, e, i) => (e.status === "planned" && entryTotals[i] != null ? sum + entryTotals[i]! : sum),
      0,
    );
    const plannedCount = entries.filter((e) => e.status === "planned").length;

    const firstWeekday = (new Date(viewYear, viewMonth - 1, 1).getDay() + 6) % 7;
    const daysInMonth = new Date(viewYear, viewMonth, 0).getDate();
    let cells = "";
    for (let i = 0; i < firstWeekday; i++) {
      cells += `<div class="cal-cell blank"></div>`;
    }
    for (let d = 1; d <= daysInMonth; d++) {
      const date = `${viewYear}-${pad(viewMonth)}-${pad(d)}`;
      const dayEntries = byDate.get(date) ?? [];
      // One icon per status present: 🏋 = worked out, 📋 = planned.
      const icons =
        (dayEntries.some((e) => e.status === "done") ? `<span class="cal-ico">🏋</span>` : "") +
        (dayEntries.some((e) => e.status === "planned") ? `<span class="cal-ico">📋</span>` : "");
      cells += `
        <button class="cal-cell day ${date === today ? "today" : ""} ${date === selected ? "selected" : ""}"
                data-date="${date}">
          <span class="num">${d}</span>
          <span class="dots">${icons}</span>
        </button>`;
    }

    const selDate = new Date(`${selected}T12:00:00`);
    const selLabel = `${WEEKDAYS[(selDate.getDay() + 6) % 7]}, ${selDate.getDate()} ${MONTHS[selDate.getMonth()]}`;

    root.innerHTML = `
      <div class="screen calendar">
        <header class="topbar">
          <h1>${MONTHS[viewMonth - 1]} ${viewYear}</h1>
          <button class="btn" id="prev">‹</button>
          <button class="btn" id="next">›</button>
        </header>
        <div class="cal-scroll">
        <div class="cal-weekdays">${WEEKDAYS.map((w) => `<span>${w}</span>`).join("")}</div>
        <div class="cal-grid">${cells}</div>
        <div class="day-panel">
          <div class="day-panel-head">
            <h2>${selLabel}</h2>
            <button class="btn" id="gotoday">Today</button>
          </div>
          ${plannedCount > 1 ? `<div class="day-total">Planned total: ${fmtDuration(plannedTotal)}</div>` : ""}
          ${
            entries.length === 0
              ? `<div class="empty small">Nothing on this day.</div>`
              : entries.map((e, i) => entryCard(e, i, entryTotals[i])).join("")
          }
          <div id="daystatus" class="editor-status"></div>
          ${
            showLibraryPicker
              ? `<div class="lib-picker" id="libpicker"><div class="empty small">loading…</div></div>`
              : `<div class="day-add">
                   <button class="btn" id="upload">📂 Upload .md</button>
                   <button class="btn" id="fromlib">📚 From library</button>
                   <a class="btn" href="#/edit/@${selected}">＋ Build</a>
                 </div>`
          }
        </div>
        </div>
        <input type="file" id="file" accept=".md,.markdown,.txt" hidden>
        ${tabBar("history")}
      </div>`;

    root.querySelectorAll<HTMLButtonElement>(".cal-cell.day").forEach((cell) => {
      cell.addEventListener("click", () => select(cell.dataset.date!));
    });
    root.querySelector("#prev")!.addEventListener("click", () => {
      viewMonth--;
      if (viewMonth === 0) {
        viewMonth = 12;
        viewYear--;
      }
      void render();
    });
    root.querySelector("#next")!.addEventListener("click", () => {
      viewMonth++;
      if (viewMonth === 13) {
        viewMonth = 1;
        viewYear++;
      }
      void render();
    });
    root.querySelector("#gotoday")!.addEventListener("click", () => select(today));

    root.querySelectorAll<HTMLButtonElement>("button.copy").forEach((btn) => {
      btn.addEventListener("click", async () => {
        const e = entries[Number(btn.dataset.i)];
        showStatus((await copyText(e.markdown)) ? "✓ copied to clipboard" : "copy failed", true);
      });
    });

    root.querySelectorAll<HTMLButtonElement>("button.promote").forEach((btn) => {
      btn.addEventListener("click", async () => {
        try {
          const sum = await api.promoteDayEntry(selected, Number(btn.dataset.i));
          showStatus(`✓ saved "${sum.name}" to library`, true);
        } catch (e) {
          const errs = e as ParseError[];
          showStatus(`cannot save: ${errs[0]?.message ?? e}`, false);
        }
      });
    });

    root.querySelectorAll<HTMLButtonElement>("button.move").forEach((btn) => {
      btn.addEventListener("click", () => {
        movingIdx = Number(btn.dataset.i);
        void render();
      });
    });
    root.querySelector("#moveok")?.addEventListener("click", async (ev) => {
      const i = Number((ev.currentTarget as HTMLButtonElement).dataset.i);
      const to = root.querySelector<HTMLInputElement>("#movedate")!.value;
      if (!/^\d{4}-\d{2}-\d{2}$/.test(to)) {
        showStatus("pick a date first", false);
        return;
      }
      movingIdx = null;
      try {
        await api.moveDayEntry(selected, i, to);
        select(to);
      } catch (e) {
        showStatus(String(e), false);
        void render();
      }
    });
    root.querySelector("#movecancel")?.addEventListener("click", () => {
      movingIdx = null;
      void render();
    });

    root.querySelectorAll<HTMLButtonElement>("button.delete").forEach((btn) => {
      btn.addEventListener("click", async () => {
        if (btn.dataset.armed) {
          await api.deleteDayEntry(selected, Number(btn.dataset.i));
          void render();
        } else {
          btn.dataset.armed = "1";
          btn.textContent = "Sure?";
          setTimeout(() => {
            delete btn.dataset.armed;
            btn.textContent = "Remove";
          }, 3000);
        }
      });
    });

    const fileInput = root.querySelector<HTMLInputElement>("#file")!;
    root.querySelector("#upload")?.addEventListener("click", () => fileInput.click());
    fileInput.addEventListener("change", async () => {
      const file = fileInput.files?.[0];
      if (!file) return;
      const text = await file.text();
      const planCheck = await api.parsePlanPreview(text);
      if (planCheck.status === "ok") {
        // Plans carry their own dates; import as a plan instead of pinning
        // the whole thing to one day.
        await api.savePlan(text, null).catch(() => undefined);
        location.hash = "#/library";
        return;
      }
      try {
        await api.addDayEntry(selected, text);
        void render();
      } catch (e) {
        const errs = e as ParseError[];
        showStatus(
          errs[0] ? `line ${errs[0].line}: ${errs[0].message}` : String(e),
          false,
        );
      }
    });

    root.querySelector("#fromlib")?.addEventListener("click", async () => {
      showLibraryPicker = true;
      await render();
      const picker = root.querySelector<HTMLElement>("#libpicker")!;
      const workouts = (await api.listWorkouts()).filter((w) => !w.error);
      picker.innerHTML =
        workouts.length === 0
          ? `<div class="empty small">library is empty</div><button class="btn" id="pickcancel">Cancel</button>`
          : workouts
              .map(
                (w) =>
                  `<button class="btn pick" data-slug="${esc(w.slug)}">${esc(w.name)}</button>`,
              )
              .join("") + `<button class="btn" id="pickcancel">Cancel</button>`;
      picker.querySelectorAll<HTMLButtonElement>("button.pick").forEach((b) => {
        b.addEventListener("click", async () => {
          showLibraryPicker = false;
          await api.addDayFromLibrary(selected, b.dataset.slug!);
          void render();
        });
      });
      picker.querySelector("#pickcancel")?.addEventListener("click", () => {
        showLibraryPicker = false;
        void render();
      });
    });
  }

  await render();
}
