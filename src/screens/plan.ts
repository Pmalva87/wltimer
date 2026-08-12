import { api, fmtDuration, type PlanDayView, type PlanView } from "../api";
import { copyText } from "../clipboard";
import { saveMarkdownFile } from "../files";
import { esc, syncSummary } from "./library";

/** `2026-08-12` → `Wed 12 Aug`. */
function shortDate(date: string): string {
  const d = new Date(`${date}T12:00:00`);
  return d.toLocaleDateString([], { weekday: "short", day: "numeric", month: "short" });
}

/**
 * What a plan holds, and what became of each of its days.
 *
 * A plan used to be write-only — a name, a day count and a date range in the
 * library, with Export the only way to read one. The status per day is the
 * part worth having: it says what a sync would do before you press it.
 */
export async function renderPlan(root: HTMLElement, slug: string) {
  let view: PlanView;
  let renaming = false;

  function showStatus(msg: string, ok: boolean) {
    const el = root.querySelector<HTMLElement>("#planstatus");
    if (!el) return;
    el.className = `editor-status ${ok ? "valid" : "invalid"}`;
    el.textContent = msg;
  }

  /** The one line that says what the next sync will do with this day. */
  function dayState(d: PlanDayView): string {
    if (d.status === null) return `<span class="meta-warn">not on the calendar</span>`;
    if (d.status === "done") return `✓ done — a sync never touches it`;
    if (d.edited) return `✎ edited here — a sync will leave it as it is`;
    return `⧗ planned`;
  }

  function dayRow(d: PlanDayView): string {
    // The entry can sit on another date: finishing a workout moves it to the
    // day it was done, and the plan still points at the day it asked for.
    const moved =
      d.entry_date && d.entry_date !== d.date
        ? ` · on ${shortDate(d.entry_date)}`
        : "";
    const link =
      d.entry_date !== null && d.entry_index !== null
        ? `#/view/@${d.entry_date}:${d.entry_index}/plan/${encodeURIComponent(slug)}`
        : null;
    const title = `<span class="name">${esc(d.name)}</span>
        <span class="meta">${shortDate(d.date)} · ${fmtDuration(d.total_secs)}${moved} · ${dayState(d)}</span>`;
    return `
      <div class="plan-day">
        ${link ? `<a class="info tappable" href="${link}">${title}</a>` : `<div class="info">${title}</div>`}
        ${d.id ? `<button class="btn danger day-remove" data-id="${esc(d.id)}">🗑 Remove</button>` : ""}
      </div>`;
  }

  /**
   * What is left to train, then what has been. A plan is read to answer
   * "what's next", and days already done are history that should not be in
   * the way of it — so they get their own section rather than a status chip
   * you have to scan for.
   */
  function daySections(): string {
    const todo = view.days.filter((d) => d.status !== "done");
    const done = view.days.filter((d) => d.status === "done");
    if (view.days.length === 0) {
      return `<div class="empty small">This plan has no days.</div>`;
    }
    const section = (title: string, days: PlanDayView[]) =>
      days.length === 0
        ? ""
        : `<div class="plan-section">${title} · ${days.length}</div>${days.map(dayRow).join("")}`;
    return (
      section("To do", todo) +
      (todo.length === 0 ? `<div class="empty small">Every day of this plan is done. 💪</div>` : "") +
      section("Done", done)
    );
  }

  async function render() {
    try {
      view = await api.viewPlan(slug);
    } catch (e) {
      root.innerHTML = `
        <div class="screen">
          <header class="topbar"><h1>Plan</h1><a class="btn" href="#/library">Back</a></header>
          <div class="empty">${esc(String(e))}</div>
        </div>`;
      return;
    }

    const dates = view.days.map((d) => d.date);
    const range = dates.length ? `${dates[0]} → ${dates[dates.length - 1]}` : "no days";
    root.innerHTML = `
      <div class="screen plan-view">
        <header class="topbar">
          <h1>${esc(view.name)}</h1>
          <a class="btn" href="#/library">Back</a>
        </header>
        <div class="plan-scroll">
          <div class="plan-meta">
            ${view.days.length} day${view.days.length === 1 ? "" : "s"} · ${range}
            ${view.updated ? `<br>last changed ${new Date(view.updated).toLocaleString()}` : ""}
          </div>
          ${view.error ? `<div class="editor-status invalid">${esc(view.error)}</div>` : ""}
          ${
            renaming
              ? `<div class="plan-rename">
                   <input type="text" id="planname" class="text-input" value="${esc(view.name)}">
                   <button class="btn primary" id="planrenameok">Save</button>
                   <button class="btn" id="planrenamecancel">Cancel</button>
                 </div>`
              : `<div class="plan-actions">
                   <button class="btn primary" id="plansync">⟳ Sync</button>
                   <button class="btn" id="planrename">✎ Rename</button>
                   <button class="btn" id="planexport">⇩ Export</button>
                   <button class="btn" id="plancopy">⧉ Copy</button>
                 </div>`
          }
          <div id="planstatus" class="editor-status"></div>
          ${daySections()}
        </div>
      </div>`;

    root.querySelector("#planrename")?.addEventListener("click", () => {
      renaming = true;
      void render();
    });
    root.querySelector("#planrenamecancel")?.addEventListener("click", () => {
      renaming = false;
      void render();
    });
    root.querySelector("#planrenameok")?.addEventListener("click", async () => {
      const name = root.querySelector<HTMLInputElement>("#planname")!.value;
      try {
        await api.renamePlan(slug, name);
        renaming = false;
        await render();
        showStatus("✓ renamed", true);
      } catch (e) {
        showStatus(String(e), false);
      }
    });

    root.querySelector("#plansync")?.addEventListener("click", async () => {
      try {
        showStatus(`✓ ${syncSummary(await api.syncPlan(slug))}`, true);
      } catch (e) {
        showStatus(String(e), false);
      }
    });
    root.querySelector("#planexport")?.addEventListener("click", async () => {
      saveMarkdownFile(`${slug}.md`, await api.getPlanSource(slug));
      showStatus("✓ exported — fix it and upload it back", true);
    });
    root.querySelector("#plancopy")?.addEventListener("click", async () => {
      const source = await api.getPlanSource(slug);
      showStatus((await copyText(source)) ? "✓ copied to clipboard" : "copy failed", true);
    });

    root.querySelectorAll<HTMLButtonElement>("button.day-remove").forEach((btn) => {
      btn.addEventListener("click", async () => {
        if (!btn.dataset.armed) {
          btn.dataset.armed = "1";
          btn.textContent = "Sure?";
          setTimeout(() => {
            delete btn.dataset.armed;
            btn.textContent = "🗑 Remove";
          }, 3000);
          return;
        }
        try {
          const report = await api.deletePlanDay(slug, btn.dataset.id!);
          await render();
          showStatus(`✓ day removed · ${syncSummary(report)}`, true);
        } catch (e) {
          showStatus(String(e), false);
        }
      });
    });
  }

  await render();
}
