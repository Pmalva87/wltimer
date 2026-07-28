import {
  api,
  fmtDuration,
  workoutTotalSecs,
  PREPARE_SECS,
  type Block,
  type ParseError,
  type Workout,
} from "../api";
import { copyText } from "../clipboard";
import { esc } from "./library";

const DRAFT_KEY = "wltimer.draft";
const RUN_DRAFT_KEY = "wltimer.rundraft";
const OLD_QUICK_KEY = "wltimer.quick";

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

function loadDraft(): Workout {
  try {
    const raw = JSON.parse(localStorage.getItem(DRAFT_KEY) ?? "null");
    if (raw && Array.isArray(raw.blocks) && raw.blocks.length > 0) {
      return raw as Workout;
    }
    // Migrate the retired quick-timer settings into a draft.
    const quick = JSON.parse(localStorage.getItem(OLD_QUICK_KEY) ?? "null");
    if (quick && Array.isArray(quick.parts) && quick.parts.length > 0) {
      return {
        name: "",
        blocks: quick.parts.map(
          (p: { intervals?: number; workSecs?: number; restSecs?: number; restAfterSecs?: number }) => ({
            ...defaultBlock(),
            intervals: p.intervals ?? 5,
            work_secs: p.workSecs ?? 60,
            rest_secs: p.restSecs ?? 0,
            rest_after_secs: p.restAfterSecs ?? 60,
          }),
        ),
      };
    }
  } catch {
    // fall through
  }
  return { name: "", blocks: [defaultBlock()] };
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

export async function renderBuilder(root: HTMLElement, slug: string | null) {
  let w: Workout;
  let mode: "form" | "md" = "form";
  let mdText = "";
  let initialErrors: ParseError[] = [];
  const openNotes = new Set<number>();

  if (slug) {
    const source = await api.getSource(slug);
    const parsed = await api.parseFull(source);
    if (parsed.status === "ok") {
      w = parsed.workout;
    } else {
      // Stored file no longer parses (edited externally) — open as markdown.
      w = { name: slug, blocks: [] };
      mode = "md";
      mdText = source;
      initialErrors = parsed.errors;
    }
  } else {
    w = loadDraft();
  }

  function persist() {
    if (!slug) {
      localStorage.setItem(DRAFT_KEY, JSON.stringify(w));
    }
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

  async function save() {
    let source: string;
    if (mode === "md") {
      source = mdText;
    } else {
      if (!w.name.trim()) {
        showMessage("give the workout a name to save it", false);
        root.querySelector<HTMLInputElement>("#wname")?.focus();
        return;
      }
      source = await api.serializeWorkout(withDefaults());
    }
    try {
      await api.saveWorkout(source, slug);
      if (!slug) {
        localStorage.removeItem(DRAFT_KEY);
      }
      location.hash = "#/";
    } catch (e) {
      showErrors(e as ParseError[]);
    }
  }

  /** Copy of the workout with empty names replaced so it can run/serialize. */
  function withDefaults(): Workout {
    return {
      name: w.name.trim() || "Quick Timer",
      blocks: w.blocks.map((b, i) => ({
        ...b,
        name: b.name.trim() || `Part ${i + 1}`,
        rest_secs: b.rest_secs || null,
        rest_after_secs: b.rest_after_secs || null,
      })),
    };
  }

  function startNow() {
    if (w.blocks.length === 0) {
      showMessage("add at least one part", false);
      return;
    }
    localStorage.setItem(RUN_DRAFT_KEY, JSON.stringify(withDefaults()));
    location.hash = "#/run/draft";
  }

  function stepper(id: string, value: string, step: number, minusLabel: string, plusLabel: string) {
    return `
      <div class="stepper" data-id="${id}" data-step="${step}">
        <button class="btn step">${minusLabel}</button>
        <span class="quick-value">${value}</span>
        <button class="btn step plus">${plusLabel}</button>
      </div>`;
  }

  function renderForm() {
    const multi = w.blocks.length > 1;
    root.innerHTML = `
      <div class="screen builder">
        <header class="topbar">
          <a class="btn" href="#/">‹ Back</a>
          <h1>${slug ? "Edit" : "New"} workout</h1>
          <button class="btn primary" id="save">Save</button>
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
                ${stepper(`${i}.intervals`, String(b.intervals), 1, "−", "+")}
              </div>
              <div class="quick-row">
                <span class="quick-label">Work</span>
                ${stepper(`${i}.work_secs`, fmtDuration(b.work_secs), 15, "−15s", "+15s")}
              </div>
              <div class="quick-row">
                <span class="quick-label">Rest</span>
                ${stepper(`${i}.rest_secs`, b.rest_secs ? fmtDuration(b.rest_secs) : "none", 15, "−15s", "+15s")}
              </div>
              ${
                i < w.blocks.length - 1
                  ? `<div class="quick-row">
                       <span class="quick-label">Rest after part</span>
                       ${stepper(`${i}.rest_after_secs`, b.rest_after_secs ? fmtDuration(b.rest_after_secs) : "none", 15, "−15s", "+15s")}
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
            <button class="btn" id="upload">📂 Upload .md</button>
            <button class="btn" id="mdtoggle">‹/› Markdown</button>
          </div>
          <div id="status" class="editor-status"></div>
          <div class="quick-total">total ${fmtDuration(workoutTotalSecs(w))} (incl. ${PREPARE_SECS}s get-ready)</div>
          <button class="btn start" id="go">START</button>
        </div>
        <input type="file" id="file" accept=".md,.markdown,.txt" hidden>
      </div>`;

    root.querySelector<HTMLInputElement>("#wname")!.addEventListener("input", (e) => {
      w.name = (e.target as HTMLInputElement).value;
      persist();
    });

    root.querySelectorAll<HTMLInputElement>("input.part-name").forEach((inp) => {
      inp.addEventListener("input", () => {
        w.blocks[Number(inp.dataset.part)].name = inp.value;
        persist();
      });
    });

    root.querySelectorAll<HTMLTextAreaElement>("textarea.notes-text").forEach((ta) => {
      ta.addEventListener("input", () => {
        w.blocks[Number(ta.dataset.part)].description_md = ta.value;
        persist();
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
      const [idx, field] = st.dataset.id!.split(".") as [
        string,
        "intervals" | "work_secs" | "rest_secs" | "rest_after_secs",
      ];
      const step = Number(st.dataset.step);
      st.querySelectorAll<HTMLButtonElement>("button.step").forEach((btn) => {
        btn.addEventListener("click", () => {
          const b = w.blocks[Number(idx)];
          const delta = step * (btn.classList.contains("plus") ? 1 : -1);
          const min = field === "intervals" ? 1 : field === "work_secs" ? 15 : 0;
          b[field] = Math.max(min, (b[field] ?? 0) + delta);
          persist();
          renderForm();
        });
      });
    });

    root.querySelectorAll<HTMLButtonElement>("button.remove").forEach((btn) => {
      btn.addEventListener("click", () => {
        w.blocks.splice(Number(btn.dataset.part), 1);
        persist();
        renderForm();
      });
    });

    root.querySelector("#addpart")!.addEventListener("click", () => {
      const last = w.blocks[w.blocks.length - 1];
      w.blocks.push(last ? { ...last, name: "", description_md: "" } : defaultBlock());
      persist();
      renderForm();
    });

    const fileInput = root.querySelector<HTMLInputElement>("#file")!;
    root.querySelector("#upload")!.addEventListener("click", () => fileInput.click());
    fileInput.addEventListener("change", async () => {
      const file = fileInput.files?.[0];
      if (!file) return;
      const source = await file.text();
      const parsed = await api.parseFull(source);
      if (parsed.status === "ok") {
        w = parsed.workout;
        openNotes.clear();
        persist();
        renderForm();
        showMessage(`✓ imported "${parsed.workout.name}"`);
      } else {
        // Let the user fix the file's problems in the markdown view.
        mdText = source;
        mode = "md";
        renderMd(parsed.errors);
      }
    });

    root.querySelector("#mdtoggle")!.addEventListener("click", async () => {
      mdText = await api.serializeWorkout(withDefaults());
      mode = "md";
      renderMd();
    });

    root.querySelector("#save")!.addEventListener("click", () => void save());
    root.querySelector("#go")!.addEventListener("click", startNow);
  }

  function renderMd(errors: ParseError[] = []) {
    root.innerHTML = `
      <div class="screen editor">
        <header class="topbar">
          <a class="btn" href="#/">‹ Back</a>
          <h1>Markdown</h1>
          <button class="btn primary" id="save">Save</button>
        </header>
        <textarea id="src" spellcheck="false" autocapitalize="off" autocomplete="off"></textarea>
        <div id="status" class="editor-status"></div>
        <div class="builder-tools md-tools">
          <button class="btn" id="formview">‹/› Form view</button>
          <button class="btn" id="copy">⧉ Copy</button>
        </div>
      </div>`;

    const textarea = root.querySelector<HTMLTextAreaElement>("#src")!;
    textarea.value = mdText;

    let debounce: ReturnType<typeof setTimeout> | undefined;
    async function validate() {
      const preview = await api.parsePreview(mdText);
      if (preview.status === "ok") {
        showMessage(
          `✓ ${preview.name} — ${preview.block_count} part${preview.block_count === 1 ? "" : "s"}, ${fmtDuration(preview.total_secs)}`,
        );
      } else {
        showErrors(preview.errors);
      }
    }
    textarea.addEventListener("input", () => {
      mdText = textarea.value;
      clearTimeout(debounce);
      debounce = setTimeout(() => void validate(), 300);
    });
    if (errors.length > 0) {
      showErrors(errors);
    } else {
      void validate();
    }

    root.querySelector("#formview")!.addEventListener("click", async () => {
      const parsed = await api.parseFull(mdText);
      if (parsed.status === "ok") {
        w = parsed.workout;
        openNotes.clear();
        mode = "form";
        persist();
        renderForm();
      } else {
        showErrors(parsed.errors);
      }
    });

    root.querySelector("#copy")!.addEventListener("click", async () => {
      showMessage((await copyText(mdText)) ? "✓ copied to clipboard" : "copy failed", true);
    });

    root.querySelector("#save")!.addEventListener("click", () => void save());
  }

  if (mode === "md") {
    renderMd(initialErrors);
  } else {
    renderForm();
  }
}
