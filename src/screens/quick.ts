import { fmtDuration } from "../api";

const PREPARE_SECS = 10;
const STORAGE_KEY = "wltimer.quick";

export interface QuickPart {
  intervals: number;
  workSecs: number;
  restSecs: number;
  /** Rest after this part; only used when another part follows. */
  restAfterSecs: number;
}

export interface QuickSettings {
  parts: QuickPart[];
}

const DEFAULT_PART: QuickPart = { intervals: 5, workSecs: 60, restSecs: 30, restAfterSecs: 60 };

export function loadQuick(): QuickSettings {
  try {
    const raw = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}");
    if (Array.isArray(raw.parts) && raw.parts.length > 0) {
      // Older shapes had a single global restBetweenSecs instead of per-part.
      const fallbackRestAfter = raw.restBetweenSecs ?? DEFAULT_PART.restAfterSecs;
      return {
        parts: raw.parts.map((p: Partial<QuickPart>) => ({
          ...DEFAULT_PART,
          ...p,
          restAfterSecs: p.restAfterSecs ?? fallbackRestAfter,
        })),
      };
    }
  } catch {
    // fall through
  }
  return { parts: [{ ...DEFAULT_PART }] };
}

export function quickTotalSecs(s: QuickSettings): number {
  const last = s.parts.length - 1;
  return (
    PREPARE_SECS +
    s.parts.reduce(
      (sum, p, i) =>
        sum +
        p.intervals * p.workSecs +
        (p.intervals - 1) * p.restSecs +
        (i < last ? p.restAfterSecs : 0),
      0,
    )
  );
}

export async function renderQuick(root: HTMLElement) {
  const s = loadQuick();

  function save() {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(s));
  }

  function stepper(id: string, value: string, minus: string, plus: string) {
    return `
      <div class="stepper" data-id="${id}">
        <button class="btn step" data-d="${minus}">−${minus === "1" ? "" : minus + "s"}</button>
        <span class="quick-value">${value}</span>
        <button class="btn step plus" data-d="${plus}">+${plus === "1" ? "" : plus + "s"}</button>
      </div>`;
  }

  function render() {
    const multi = s.parts.length > 1;
    root.innerHTML = `
      <div class="screen quick">
        <header class="topbar">
          <a class="btn" href="#/">‹ Back</a>
          <h1>Quick timer</h1>
        </header>
        <div class="quick-form">
          ${s.parts
            .map(
              (p, i) => `
            <div class="quick-part" data-part="${i}">
              ${
                multi
                  ? `<div class="quick-part-head">
                       <span>Part ${i + 1}</span>
                       <button class="btn danger remove" data-part="${i}">✕</button>
                     </div>`
                  : ""
              }
              <div class="quick-row">
                <span class="quick-label">Intervals</span>
                ${stepper(`${i}.intervals`, String(p.intervals), "1", "1")}
              </div>
              <div class="quick-row">
                <span class="quick-label">Work</span>
                ${stepper(`${i}.workSecs`, fmtDuration(p.workSecs), "15", "15")}
              </div>
              <div class="quick-row">
                <span class="quick-label">Rest</span>
                ${stepper(`${i}.restSecs`, p.restSecs > 0 ? fmtDuration(p.restSecs) : "none", "15", "15")}
              </div>
              ${
                i < s.parts.length - 1
                  ? `<div class="quick-row">
                       <span class="quick-label">Rest after part</span>
                       ${stepper(`${i}.restAfterSecs`, p.restAfterSecs > 0 ? fmtDuration(p.restAfterSecs) : "none", "15", "15")}
                     </div>`
                  : ""
              }
            </div>`,
            )
            .join("")}
          <button class="btn" id="addpart">+ Add part</button>
          <div class="quick-total" id="total">total ${fmtDuration(quickTotalSecs(s))} (incl. ${PREPARE_SECS}s get-ready)</div>
          <button class="btn start" id="go">START</button>
        </div>
      </div>`;

    root.querySelectorAll<HTMLElement>(".stepper").forEach((st) => {
      const id = st.dataset.id!;
      st.querySelectorAll<HTMLButtonElement>("button.step").forEach((btn) => {
        btn.addEventListener("click", () => {
          const delta = Number(btn.dataset.d) * (btn.classList.contains("plus") ? 1 : -1);
          const [idx, field] = id.split(".") as [string, keyof QuickPart];
          const part = s.parts[Number(idx)];
          const min = field === "intervals" ? 1 : field === "workSecs" ? 15 : 0;
          part[field] = Math.max(min, part[field] + delta);
          save();
          render();
        });
      });
    });

    root.querySelectorAll<HTMLButtonElement>("button.remove").forEach((btn) => {
      btn.addEventListener("click", () => {
        s.parts.splice(Number(btn.dataset.part), 1);
        save();
        render();
      });
    });

    root.querySelector("#addpart")!.addEventListener("click", () => {
      s.parts.push({ ...(s.parts[s.parts.length - 1] ?? DEFAULT_PART) });
      save();
      render();
    });

    root.querySelector("#go")!.addEventListener("click", () => {
      location.hash = "#/run/quick";
    });
  }

  render();
}
