import { fmtDuration } from "../api";

const PREPARE_SECS = 10;
const STORAGE_KEY = "wltimer.quick";

export interface QuickPart {
  intervals: number;
  workSecs: number;
  restSecs: number;
}

export interface QuickSettings {
  parts: QuickPart[];
  restBetweenSecs: number;
}

const DEFAULT_PART: QuickPart = { intervals: 5, workSecs: 60, restSecs: 30 };

export function loadQuick(): QuickSettings {
  try {
    const raw = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "{}");
    if (Array.isArray(raw.parts) && raw.parts.length > 0) {
      return { parts: raw.parts, restBetweenSecs: raw.restBetweenSecs ?? 60 };
    }
    // Migrate the old single-part shape (or fall through to defaults).
    if (typeof raw.intervals === "number") {
      return {
        parts: [{ intervals: raw.intervals, workSecs: raw.workSecs, restSecs: raw.restSecs }],
        restBetweenSecs: 60,
      };
    }
  } catch {
    // fall through
  }
  return { parts: [{ ...DEFAULT_PART }], restBetweenSecs: 60 };
}

export function quickTotalSecs(s: QuickSettings): number {
  const partsSecs = s.parts.reduce(
    (sum, p) => sum + p.intervals * p.workSecs + (p.intervals - 1) * p.restSecs,
    0,
  );
  return PREPARE_SECS + partsSecs + (s.parts.length - 1) * s.restBetweenSecs;
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
            </div>`,
            )
            .join("")}
          <button class="btn" id="addpart">+ Add part</button>
          ${
            multi
              ? `<div class="quick-row">
                   <span class="quick-label">Rest between parts</span>
                   ${stepper("restBetweenSecs", s.restBetweenSecs > 0 ? fmtDuration(s.restBetweenSecs) : "none", "15", "15")}
                 </div>`
              : ""
          }
          <div class="quick-total" id="total">total ${fmtDuration(quickTotalSecs(s))} (incl. ${PREPARE_SECS}s get-ready)</div>
          <button class="btn start" id="go">START</button>
        </div>
      </div>`;

    root.querySelectorAll<HTMLElement>(".stepper").forEach((st) => {
      const id = st.dataset.id!;
      st.querySelectorAll<HTMLButtonElement>("button.step").forEach((btn) => {
        btn.addEventListener("click", () => {
          const delta = Number(btn.dataset.d) * (btn.classList.contains("plus") ? 1 : -1);
          if (id === "restBetweenSecs") {
            s.restBetweenSecs = Math.max(0, s.restBetweenSecs + delta);
          } else {
            const [idx, field] = id.split(".") as [string, keyof QuickPart];
            const part = s.parts[Number(idx)];
            const min = field === "intervals" ? 1 : field === "workSecs" ? 15 : 0;
            part[field] = Math.max(min, part[field] + delta);
          }
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
