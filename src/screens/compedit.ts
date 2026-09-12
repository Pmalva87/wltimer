import {
  api,
  fmtKg,
  newCompetition,
  todayStr,
  type Attempt,
  type AttemptResult,
  type Competition,
  type LiftEntry,
  type ParseError,
  type Qualification,
  type Standard,
} from "../api";
import { esc } from "./library";

/**
 * Add or change a competition.
 *
 * Two things are being written here and they are not the same thing: what
 * happened (or will happen) at *this* meet, and what it takes to get **into**
 * it. The second is the qualifying table, which lives on the meet it admits
 * you to — so adding "the marks I need for Europeans" means adding Europeans
 * as a competition and giving it a standard, not annotating the meet you are
 * lifting at next week.
 */
export async function renderCompEdit(root: HTMLElement, slug: string | null) {
  let c: Competition = newCompetition("", todayStr());
  let loadError: string | null = null;

  if (slug) {
    try {
      const parsed = await api.parseCompetition(await api.getCompetitionSource(slug));
      if (parsed.status === "ok") {
        c = parsed.competition;
      } else {
        loadError = `line ${parsed.errors[0].line}: ${parsed.errors[0].message}`;
      }
    } catch (e) {
      loadError = String(e);
    }
  }

  const backHash = slug ? `#/comp/${encodeURIComponent(slug)}` : "#/library";

  function render() {
    root.innerHTML = `
      <div class="screen editor">
        <header class="topbar">
          <a class="btn" href="${backHash}">‹ Back</a>
          <h1>${slug ? "Edit meet" : "New meet"}</h1>
          <button class="btn primary" id="save">Save</button>
        </header>
        <div class="view-scroll">
          ${loadError ? `<div class="editor-status invalid">${esc(loadError)}</div>` : ""}
          <div id="status" class="editor-status"></div>

          <section class="comp-fields">
            ${field("Name", `<input class="text-input" id="name" value="${esc(c.name)}" placeholder="Portuguese Nationals 2027">`)}
            ${field("Date", `<input class="text-input" id="date" type="date" value="${esc(c.date ?? "")}">`)}
            ${field(
              "Sanctioned by",
              `<input class="text-input" id="orgs" value="${esc(c.orgs.join(", "))}" placeholder="FPH, IWF">`,
              "Comma-separated. This is what decides whether a total here counts towards another meet's standard.",
            )}
            ${field("Weight class", `<input class="text-input" id="category" value="${esc(c.category ?? "")}" placeholder="89 kg">`)}
            ${field("Age group", `<input class="text-input" id="agegroup" value="${esc(c.age_group ?? "")}" placeholder="M40">`)}
            ${field(
              "Bodyweight",
              `<input class="text-input" id="bodyweight" type="number" step="0.1" inputmode="decimal" value="${
                c.bodyweight ?? ""
              }" placeholder="88.4">`,
            )}
          </section>

          ${liftSection("Snatch", "snatch", c.snatch)}
          ${liftSection("Clean & Jerk", "clean_jerk", c.clean_jerk)}

          <div class="section-head">
            <h2>Entry standard</h2>
            <div class="section-actions">
              <button class="btn" id="togglequal">${c.qualification ? "Remove" : "+ Add"}</button>
            </div>
          </div>
          <div class="empty small">
            What it takes to get <em>into</em> this meet: a total, a window it
            must be set in, and which federations' meets count. Add it here, on
            the competition you are trying to enter.
          </div>
          ${c.qualification ? qualSection(c.qualification) : ""}

          ${
            slug
              ? `<div class="comp-danger"><button class="btn danger" id="delete">🗑 Delete this meet</button></div>`
              : ""
          }
        </div>
      </div>`;
    bind();
  }

  function bind() {
    const on = (id: string, ev: string, fn: (el: HTMLInputElement) => void) => {
      const el = root.querySelector<HTMLInputElement>(`#${id}`);
      el?.addEventListener(ev, () => fn(el));
    };
    const text = (v: string): string | null => (v.trim() === "" ? null : v.trim());

    on("name", "input", (el) => (c.name = el.value));
    on("date", "change", (el) => (c.date = text(el.value)));
    on("orgs", "input", (el) => {
      c.orgs = el.value
        .split(",")
        .map((s) => s.trim())
        .filter((s) => s !== "");
    });
    on("category", "input", (el) => (c.category = text(el.value)));
    on("agegroup", "input", (el) => (c.age_group = text(el.value)));
    on("bodyweight", "input", (el) => {
      const n = Number(el.value);
      c.bodyweight = el.value.trim() === "" || Number.isNaN(n) ? null : n;
    });

    // --- attempts ---
    root.querySelectorAll<HTMLInputElement>("[data-attempt]").forEach((el) => {
      el.addEventListener("input", () => {
        const entry = liftOf(el.dataset.lift!);
        const i = Number(el.dataset.attempt);
        const kg = Number(el.value);
        entry.attempts[i] =
          el.value.trim() === "" || Number.isNaN(kg) || kg <= 0
            ? null
            : { kg, result: entry.attempts[i]?.result ?? "declared" };
        // The result button's label follows the slot, so it has to be redrawn
        // when a weight appears or disappears under it.
        const btn = root.querySelector<HTMLButtonElement>(
          `[data-result="${i}"][data-lift="${el.dataset.lift}"]`,
        );
        if (btn) setResultLabel(btn, entry.attempts[i]);
      });
    });
    root.querySelectorAll<HTMLButtonElement>("[data-result]").forEach((btn) => {
      btn.addEventListener("click", () => {
        const entry = liftOf(btn.dataset.lift!);
        const i = Number(btn.dataset.result);
        const a = entry.attempts[i];
        if (!a) return;
        // Round-trip rather than one-way: a mis-tap is undone by tapping on.
        const next: Record<AttemptResult, AttemptResult> = {
          declared: "good",
          good: "miss",
          miss: "declared",
        };
        a.result = next[a.result];
        setResultLabel(btn, a);
      });
    });

    // --- warmup ---
    root.querySelectorAll<HTMLButtonElement>("[data-warmadd]").forEach((btn) =>
      btn.addEventListener("click", () => {
        const entry = liftOf(btn.dataset.warmadd!);
        const last = entry.warmup[entry.warmup.length - 1];
        entry.warmup.push({ kg: last ? last.kg + 10 : 20, reps: last?.reps ?? 2, done: false });
        render();
      }),
    );
    root.querySelectorAll<HTMLElement>("[data-warmtick]").forEach((el) =>
      el.addEventListener("click", () => {
        const entry = liftOf(el.dataset.lift!);
        const set = entry.warmup[Number(el.dataset.warmtick)];
        set.done = !set.done;
        render();
      }),
    );
    root.querySelectorAll<HTMLButtonElement>("[data-warmdel]").forEach((btn) =>
      btn.addEventListener("click", () => {
        liftOf(btn.dataset.lift!).warmup.splice(Number(btn.dataset.warmdel), 1);
        render();
      }),
    );
    root.querySelectorAll<HTMLInputElement>("[data-warmkg]").forEach((el) =>
      el.addEventListener("input", () => {
        const set = liftOf(el.dataset.lift!).warmup[Number(el.dataset.warmkg)];
        const n = Number(el.value);
        if (!Number.isNaN(n) && n > 0) set.kg = n;
      }),
    );
    root.querySelectorAll<HTMLInputElement>("[data-warmreps]").forEach((el) =>
      el.addEventListener("input", () => {
        const set = liftOf(el.dataset.lift!).warmup[Number(el.dataset.warmreps)];
        const n = Number(el.value);
        if (!Number.isNaN(n) && n >= 1) set.reps = Math.round(n);
      }),
    );

    // --- qualification ---
    root.querySelector("#togglequal")?.addEventListener("click", () => {
      c.qualification = c.qualification
        ? null
        : { from: null, to: null, counts: [], standards: [blankStandard(c)] };
      render();
    });
    on("qualfrom", "change", (el) => (c.qualification!.from = text(el.value)));
    on("qualto", "change", (el) => (c.qualification!.to = text(el.value)));
    on("qualcounts", "input", (el) => {
      c.qualification!.counts = el.value
        .split(",")
        .map((s) => s.trim())
        .filter((s) => s !== "");
    });
    root.querySelector("#addmark")?.addEventListener("click", () => {
      c.qualification!.standards.push(blankStandard(c));
      render();
    });
    root.querySelectorAll<HTMLButtonElement>("[data-markdel]").forEach((btn) =>
      btn.addEventListener("click", () => {
        c.qualification!.standards.splice(Number(btn.dataset.markdel), 1);
        render();
      }),
    );
    root.querySelectorAll<HTMLInputElement>("[data-markfield]").forEach((el) =>
      el.addEventListener("input", () => {
        const s = c.qualification!.standards[Number(el.dataset.i)];
        const f = el.dataset.markfield!;
        if (f === "total") {
          const n = Number(el.value);
          s.total = Number.isNaN(n) ? 0 : n;
        } else if (f === "age") {
          s.age_group = el.value.trim() === "" ? null : el.value.trim();
        } else if (f === "category") {
          s.category = el.value.trim() === "" ? null : el.value.trim();
        } else {
          s.label = el.value.trim();
        }
      }),
    );

    root.querySelector("#save")?.addEventListener("click", () => void save());
    root.querySelector("#delete")?.addEventListener("click", (ev) => {
      const btn = ev.currentTarget as HTMLButtonElement;
      if (!btn.dataset.armed) {
        btn.dataset.armed = "1";
        btn.textContent = "Sure?";
        setTimeout(() => {
          delete btn.dataset.armed;
          btn.textContent = "🗑 Delete this meet";
        }, 3000);
        return;
      }
      void api.deleteCompetition(slug!).then(() => (location.hash = "#/library"));
    });
  }

  function liftOf(name: string): LiftEntry {
    return name === "snatch" ? c.snatch : c.clean_jerk;
  }

  function status(message: string, ok: boolean) {
    const el = root.querySelector<HTMLElement>("#status");
    if (!el) return;
    el.className = `editor-status ${ok ? "valid" : "invalid"}`;
    el.textContent = message;
  }

  async function save() {
    if (c.name.trim() === "") {
      status("Give the meet a name.", false);
      return;
    }
    // A mark with no number is a row that would fail to parse on the way back
    // in; drop it here rather than handing the user a line number.
    if (c.qualification) {
      c.qualification.standards = c.qualification.standards.filter((s) => s.total > 0);
      if (c.qualification.standards.length === 0) {
        status("An entry standard needs at least one mark, or remove it.", false);
        return;
      }
    }
    try {
      const source = await api.serializeCompetition(c);
      const saved = await api.saveCompetition(source, slug);
      location.hash = `#/comp/${encodeURIComponent(saved.slug)}`;
    } catch (e) {
      const errs = e as ParseError[];
      status(
        Array.isArray(errs) && errs[0] ? `line ${errs[0].line}: ${errs[0].message}` : String(e),
        false,
      );
    }
  }

  render();
}

function blankStandard(c: Competition): Standard {
  // Pre-filled with the group this meet says it is entering, since that is
  // almost always the row you came here to write.
  return { total: 0, label: "", age_group: c.age_group, category: c.category };
}

function field(label: string, input: string, hint?: string): string {
  return `
    <label class="comp-field">
      <span class="quick-label">${label}</span>
      ${input}
      ${hint ? `<span class="comp-hint">${hint}</span>` : ""}
    </label>`;
}

function liftSection(title: string, key: string, entry: LiftEntry): string {
  const attempts = entry.attempts
    .map(
      (a, i) => `
      <div class="comp-edit-attempt">
        <span class="comp-attempt-n">${i + 1}</span>
        <input class="text-input" type="number" inputmode="decimal" step="0.5"
               data-attempt="${i}" data-lift="${key}" value="${a ? fmtKg(a.kg) : ""}" placeholder="kg">
        <button class="btn comp-result-btn" data-result="${i}" data-lift="${key}">${resultLabel(a)}</button>
      </div>`,
    )
    .join("");
  const warmup = entry.warmup
    .map(
      (w, i) => `
      <div class="comp-edit-set">
        <button class="btn comp-tick ${w.done ? "done" : ""}" data-warmtick="${i}" data-lift="${key}">${
          w.done ? "✓" : "○"
        }</button>
        <input class="text-input" type="number" inputmode="decimal" step="0.5"
               data-warmkg="${i}" data-lift="${key}" value="${fmtKg(w.kg)}">
        <span class="comp-x">×</span>
        <input class="text-input" type="number" inputmode="numeric" min="1"
               data-warmreps="${i}" data-lift="${key}" value="${w.reps}">
        <button class="btn danger" data-warmdel="${i}" data-lift="${key}">✕</button>
      </div>`,
    )
    .join("");
  return `
    <div class="section-head">
      <h2>${title}</h2>
      <div class="section-actions">
        <button class="btn" data-warmadd="${key}">+ Warmup set</button>
      </div>
    </div>
    ${warmup}
    <div class="comp-edit-attempts">${attempts}</div>`;
}

function resultLabel(a: Attempt | null): string {
  if (!a) return "—";
  return a.result === "good" ? "✓ good" : a.result === "miss" ? "✗ no lift" : "declared";
}

function setResultLabel(btn: HTMLButtonElement, a: Attempt | null) {
  btn.textContent = resultLabel(a);
  btn.className = `btn comp-result-btn ${a ? a.result : ""}`;
}

function qualSection(q: Qualification): string {
  const rows = q.standards
    .map(
      (s, i) => `
      <div class="comp-edit-mark">
        <input class="text-input" type="number" inputmode="decimal" data-markfield="total" data-i="${i}"
               value="${s.total > 0 ? fmtKg(s.total) : ""}" placeholder="total">
        <input class="text-input" data-markfield="age" data-i="${i}" value="${esc(s.age_group ?? "")}" placeholder="age group">
        <input class="text-input" data-markfield="category" data-i="${i}" value="${esc(s.category ?? "")}" placeholder="class">
        <input class="text-input" data-markfield="label" data-i="${i}" value="${esc(s.label)}" placeholder="name">
        <button class="btn danger" data-markdel="${i}">✕</button>
      </div>`,
    )
    .join("");
  return `
    <section class="comp-fields">
      ${field("Results count from", `<input class="text-input" id="qualfrom" type="date" value="${esc(q.from ?? "")}">`)}
      ${field("until", `<input class="text-input" id="qualto" type="date" value="${esc(q.to ?? "")}">`)}
      ${field(
        "Meets that count",
        `<input class="text-input" id="qualcounts" value="${esc(q.counts.join(", "))}" placeholder="BWL, IWF">`,
        "Comma-separated federations. Leave empty and any meet counts.",
      )}
      <div class="comp-marks-head">
        <span class="quick-label">Marks</span>
        <button class="btn" id="addmark">+ Add mark</button>
      </div>
      <div class="comp-hint">One row per group: the total, then the age group and class it is for. Leave both empty for a mark that applies to everyone.</div>
      ${rows}
    </section>`;
}
