import {
  api,
  fmtDuration,
  partsWithoutRestAfter,
  workoutTotalSecs,
  PREPARE_SECS,
  type Block,
  type ParseError,
  type Workout,
} from "../api";
import { saveMarkdownFile } from "../files";
import { tabBar } from "../tabs";
import { esc } from "./library";

const RUN_DRAFT_KEY = "wltimer.rundraft";

function defaultBlock(): Block {
  return {
    name: "",
    description_md: "",
    intervals: 5,
    work_secs: 60,
    rest_secs: 30,
    rest_after_secs: 60,
    color: null,
  };
}

export function loadRunDraft(): Workout | null {
  try {
    const raw = JSON.parse(localStorage.getItem(RUN_DRAFT_KEY) ?? "null");
    if (raw && Array.isArray(raw.blocks) && raw.blocks.length > 0) {
      return raw as Workout;
    }
  } catch {
    // fall through
  }
  return null;
}

/**
 * `slug` targets: null = quick tab, `<slug>` = edit library workout,
 * `@<date>` = new calendar entry, `@<date>:<index>` = edit entry.
 * `asTab` renders the bottom tab bar instead of a back button.
 * Markdown is import/export only — all editing happens in the form.
 */
export async function renderBuilder(root: HTMLElement, slug: string | null, asTab = false) {
  const dayMatch = slug?.match(/^@(\d{4}-\d{2}-\d{2})(?::(\d+))?$/) ?? null;
  const dayDate = dayMatch?.[1] ?? null;
  let dayIndex = dayMatch?.[2] != null ? Number(dayMatch[2]) : null;
  const backHash = dayDate ? `#/calendar/${dayDate}` : "#/library";

  let w: Workout = { id: null, name: "", blocks: [defaultBlock()] };
  let loadErrors: ParseError[] = [];
  const openNotes = new Set<number>();

  async function loadFromSource(source: string) {
    const parsed = await api.parseFull(source);
    if (parsed.status === "ok") {
      w = parsed.workout;
    } else {
      loadErrors = parsed.errors;
    }
  }

  if (dayDate && dayIndex != null) {
    const entry = (await api.getDay(dayDate).catch(() => []))[dayIndex];
    if (entry) {
      await loadFromSource(entry.markdown);
    } else {
      dayIndex = null;
    }
  } else if (slug && !dayDate) {
    w.name = slug;
    await loadFromSource(await api.getSource(slug));
  }

  /** Save the current state as a calendar entry; returns its index. */
  async function saveDayEntry(source: string): Promise<number> {
    if (dayIndex != null) {
      await api.updateDayEntry(dayDate!, dayIndex, source);
      return dayIndex;
    }
    dayIndex = await api.addDayEntry(dayDate!, source);
    return dayIndex;
  }

  function showErrors(errors: ParseError[]) {
    const status = root.querySelector<HTMLElement>("#status");
    if (!status) return;
    status.className = "editor-status invalid";
    status.innerHTML = errors
      .map((e) => `<div>line ${e.line}: ${esc(e.message)}</div>`)
      .join("");
  }

  function showMessage(msg: string, ok = true) {
    const status = root.querySelector<HTMLElement>("#status");
    if (!status) return;
    status.className = `editor-status ${ok ? "valid" : "invalid"}`;
    status.textContent = msg;
  }

  /**
   * Copy of the workout ready to run/serialize: empty names get defaults and
   * "rest after" is dropped from the last part (nothing follows it).
   */
  function withDefaults(): Workout {
    const last = w.blocks.length - 1;
    return {
      // Carried through deliberately: without it every save would look like a
      // new workout and the backend would mint a fresh id each time.
      id: w.id,
      name: w.name.trim() || (dayDate ? "Workout" : "Quick Timer"),
      blocks: w.blocks.map((b, i) => ({
        ...b,
        name: b.name.trim() || `Part ${i + 1}`,
        rest_secs: b.rest_secs || null,
        rest_after_secs: i < last ? b.rest_after_secs || null : null,
      })),
    };
  }

  async function save() {
    if (!dayDate && !w.name.trim()) {
      showMessage("give the workout a name to save it", false);
      root.querySelector<HTMLInputElement>("#wname")?.focus();
      return;
    }
    try {
      const source = await api.serializeWorkout(withDefaults());
      if (dayDate) {
        await saveDayEntry(source);
      } else {
        await api.saveWorkout(source, slug);
      }
      location.hash = backHash;
    } catch (e) {
      showErrors(e as ParseError[]);
    }
  }

  async function startNow() {
    if (w.blocks.length === 0) {
      showMessage("add at least one part", false);
      return;
    }
    if (dayDate) {
      // Save (or update) the calendar entry first, then run it, so finishing
      // marks that entry done instead of recording a duplicate.
      try {
        const idx = await saveDayEntry(await api.serializeWorkout(withDefaults()));
        location.hash = `#/run/@${dayDate}:${idx}`;
      } catch (e) {
        showErrors(e as ParseError[]);
      }
      return;
    }
    localStorage.setItem(RUN_DRAFT_KEY, JSON.stringify(withDefaults()));
    location.hash = "#/run/draft";
  }

  type StepField = "intervals" | "work_secs" | "rest_secs" | "rest_after_secs";

  /** Text shown between a stepper's buttons: rests read "none" at zero. */
  function stepValue(b: Block, field: StepField): string {
    if (field === "intervals") return String(b.intervals);
    if (field === "work_secs") return fmtDuration(b.work_secs);
    const v = b[field];
    return v ? fmtDuration(v) : "none";
  }

  function stepper(i: number, field: StepField, step: number, minusLabel: string, plusLabel: string) {
    return `
      <div class="stepper" data-id="${i}.${field}" data-step="${step}">
        <button class="btn step">${minusLabel}</button>
        <span class="quick-value">${stepValue(w.blocks[i], field)}</span>
        <button class="btn step plus">${plusLabel}</button>
      </div>`;
  }

  /**
   * Flag the parts that run straight into the next one. A standing warning
   * rather than a block: chaining parts is allowed, it is just rarely meant.
   */
  function refreshWarnings() {
    const flagged = new Set(partsWithoutRestAfter(w));
    root.querySelectorAll<HTMLElement>(".part-warn").forEach((el) => {
      el.hidden = !flagged.has(Number(el.dataset.part));
    });
  }

  function refreshTotal() {
    const el = root.querySelector<HTMLElement>("#total");
    if (el) {
      el.textContent = `total ${fmtDuration(workoutTotalSecs(w))} (incl. ${PREPARE_SECS}s get-ready)`;
    }
  }

  /**
   * Full rebuild of the form. Only for structural changes (parts added or
   * removed) — value edits patch in place so the view never jumps. The scroll
   * offset is carried over so the user keeps looking at the same place.
   */
  function renderForm() {
    const multi = w.blocks.length > 1;
    const scrollTop = root.querySelector<HTMLElement>(".quick-form")?.scrollTop ?? 0;
    root.innerHTML = `
      <div class="screen builder">
        <header class="topbar">
          ${asTab ? "" : `<a class="btn" href="${backHash}">‹ Back</a>`}
          <h1>${dayDate ? `Workout · ${dayDate}` : asTab ? "Quick workout" : "Edit workout"}</h1>
          <button class="btn" id="save">Save</button>
          <button class="btn primary" id="go">Start</button>
        </header>
        <div class="quick-form">
          <input id="wname" class="text-input" placeholder="Workout name (needed to save)"
                 autocomplete="off" value="${esc(w.name)}">
          ${w.blocks
            .map(
              (b, i) => `
            <div class="quick-part">
              <div class="quick-part-head">
                <input class="text-input part-name" data-part="${i}"
                       placeholder="Part ${i + 1}" autocomplete="off" value="${esc(b.name)}">
                ${multi ? `<button class="btn danger remove" data-part="${i}">✕</button>` : ""}
              </div>
              <div class="quick-row">
                <span class="quick-label">Intervals</span>
                ${stepper(i, "intervals", 1, "−", "+")}
              </div>
              <div class="quick-row">
                <span class="quick-label">Work</span>
                ${stepper(i, "work_secs", 5, "−5s", "+5s")}
              </div>
              <div class="quick-row">
                <span class="quick-label">Rest</span>
                ${stepper(i, "rest_secs", 5, "−5s", "+5s")}
              </div>
              ${
                i < w.blocks.length - 1
                  ? `<div class="quick-row">
                       <span class="quick-label">Rest after part</span>
                       ${stepper(i, "rest_after_secs", 5, "−5s", "+5s")}
                     </div>
                     <div class="part-warn" data-part="${i}" ${b.rest_after_secs ? "hidden" : ""}>
                       ⚠ Runs straight into the next part — no rest in between
                     </div>`
                  : ""
              }
              <details class="notes" data-part="${i}" ${b.description_md || openNotes.has(i) ? "open" : ""}>
                <summary>Notes / cues</summary>
                <textarea class="notes-text" data-part="${i}" rows="4"
                          placeholder="Shown on screen during this part. Markdown works.">${esc(b.description_md)}</textarea>
              </details>
            </div>`,
            )
            .join("")}
          <button class="btn" id="addpart">+ Add part</button>
          <div class="builder-tools">
            <button class="btn" id="exportmd">⇩ Export .md</button>
          </div>
          <div id="status" class="editor-status"></div>
          <div id="total" class="quick-total">total ${fmtDuration(workoutTotalSecs(w))} (incl. ${PREPARE_SECS}s get-ready)</div>
        </div>
        ${asTab ? tabBar("quick") : ""}
      </div>`;

    root.querySelector<HTMLElement>(".quick-form")!.scrollTop = scrollTop;

    root.querySelector<HTMLInputElement>("#wname")!.addEventListener("input", (e) => {
      w.name = (e.target as HTMLInputElement).value;
    });

    root.querySelectorAll<HTMLInputElement>("input.part-name").forEach((inp) => {
      inp.addEventListener("input", () => {
        w.blocks[Number(inp.dataset.part)].name = inp.value;
      });
    });

    root.querySelectorAll<HTMLTextAreaElement>("textarea.notes-text").forEach((ta) => {
      ta.addEventListener("input", () => {
        w.blocks[Number(ta.dataset.part)].description_md = ta.value;
      });
    });

    root.querySelectorAll<HTMLElement>("details.notes").forEach((d) => {
      d.addEventListener("toggle", () => {
        const i = Number(d.dataset.part);
        if ((d as HTMLDetailsElement).open) {
          openNotes.add(i);
        } else {
          openNotes.delete(i);
        }
      });
    });

    root.querySelectorAll<HTMLElement>(".stepper").forEach((st) => {
      const [idx, field] = st.dataset.id!.split(".") as [string, StepField];
      const step = Number(st.dataset.step);
      const out = st.querySelector<HTMLElement>(".quick-value")!;
      st.querySelectorAll<HTMLButtonElement>("button.step").forEach((btn) => {
        btn.addEventListener("click", () => {
          const b = w.blocks[Number(idx)];
          const delta = step * (btn.classList.contains("plus") ? 1 : -1);
          const min = field === "intervals" ? 1 : field === "work_secs" ? 5 : 0;
          b[field] = Math.max(min, (b[field] ?? 0) + delta);
          // Patch just this value and the total: rebuilding the form here
          // would scroll the pane back to the top mid-edit.
          out.textContent = stepValue(b, field);
          refreshTotal();
          refreshWarnings();
        });
      });
    });

    root.querySelectorAll<HTMLButtonElement>("button.remove").forEach((btn) => {
      btn.addEventListener("click", () => {
        w.blocks.splice(Number(btn.dataset.part), 1);
        renderForm();
      });
    });

    root.querySelector("#addpart")!.addEventListener("click", () => {
      const last = w.blocks[w.blocks.length - 1];
      w.blocks.push(last ? { ...last, name: "", description_md: "" } : defaultBlock());
      renderForm();
    });

    root.querySelector("#exportmd")!.addEventListener("click", async () => {
      const workout = withDefaults();
      const source = await api.serializeWorkout(workout);
      saveMarkdownFile(
        `${workout.name.toLowerCase().replace(/[^a-z0-9]+/g, "-")}.md`,
        source,
      );
      showMessage("✓ exported as .md file");
    });

    root.querySelector("#save")!.addEventListener("click", () => void save());
    root.querySelector("#go")!.addEventListener("click", () => void startNow());
  }

  renderForm();
  if (loadErrors.length > 0) {
    showErrors(
      loadErrors.concat({
        line: 0,
        message: "stored file could not be loaded into the form — fix it externally or delete it",
      }),
    );
  }
}
