import {
  api,
  effectiveRegistered,
  fmtKg,
  newCompetition,
  todayStr,
  type Competition,
  type ParseError,
  type Qualification,
  type Standard,
} from "../api";
import { esc } from "./library";
import { bindOrgPicker, orgPicker, type OrgPickerState } from "./orgpicker";

/**
 * The form for a new meet — every detail open at once, because a meet that
 * does not exist yet has nothing to tap. Once saved, the meet screen is where
 * each detail is changed, one tap at a time; this screen is only reached again
 * for a meet whose file no longer reads, where starting over is the fix.
 *
 * Two things are being written here and they are not the same thing: what
 * this meet is, and what it takes to get **into** it. The second is the
 * qualifying table, which lives on the meet it admits you to — so adding "the
 * marks I need for Europeans" means adding Europeans as a competition and
 * giving it a standard, not annotating the meet you are lifting at next week.
 */
export async function renderCompEdit(root: HTMLElement, slug: string | null) {
  const c: Competition = newCompetition("", todayStr());
  const orgs: OrgPickerState = { organizations: await api.listOrganizations(), adding: null };
  let loadError: string | null = null;
  if (slug) {
    try {
      const parsed = await api.parseCompetition(await api.getCompetitionSource(slug));
      if (parsed.status === "ok") {
        // Readable after all — it belongs on the meet screen.
        location.hash = `#/comp/${encodeURIComponent(slug)}`;
        return;
      }
      loadError = `line ${parsed.errors[0].line}: ${parsed.errors[0].message}`;
    } catch (e) {
      loadError = String(e);
    }
  }

  function render() {
    root.innerHTML = `
      <div class="screen editor">
        <header class="topbar">
          <a class="btn" href="#/competitions">‹ Back</a>
          <h1>${slug ? "Rewrite meet" : "New meet"}</h1>
          <button class="btn primary" id="save">Save</button>
        </header>
        <div class="view-scroll">
          ${loadError ? `<div class="editor-status invalid">${esc(loadError)} — saving replaces the file.</div>` : ""}
          <div id="status" class="editor-status"></div>

          <section class="comp-fields">
            ${field("Name", `<input class="text-input" id="name" value="${esc(c.name)}" placeholder="Portuguese Nationals 2027">`)}
            ${field("Date", `<input class="text-input" id="date" type="date" value="${esc(c.date ?? "")}">`)}
            ${field("Organizer", orgPicker("organizer", "single", c.organizer ? [c.organizer] : [], orgs), "Who is running the meet.")}
            ${field(
              "Sanctioned by",
              orgPicker("orgs", "multi", c.orgs, orgs),
              "This is what decides whether a total here counts towards another meet's standard.",
            )}
            ${field("Weight class", `<input class="text-input" id="category" value="${esc(c.category ?? "")}" placeholder="89 kg">`)}
            ${field("Age group", `<input class="text-input" id="agegroup" value="${esc(c.age_group ?? "")}" placeholder="M40">`)}
            ${field(
              "Bodyweight",
              `<input class="text-input" id="bodyweight" type="number" step="0.1" inputmode="decimal" value="${
                c.bodyweight ?? ""
              }" placeholder="88.4">`,
            )}
            ${field(
              "Registered",
              `<label class="comp-checkbox">
                 <input type="checkbox" id="registered" ${effectiveRegistered(c) ? "checked" : ""}>
                 ${effectiveRegistered(c) ? "Signed up for this meet" : "Not signed up yet"}
               </label>`,
              c.qualification && c.qualification.standards.length
                ? "Unchecked by default while there is a mark to hit — check it once you have actually entered."
                : "Checked by default — nothing here is gating entry.",
            )}
          </section>
          <div class="comp-hint">Warmups and attempts are planned on the meet itself, once it is saved.</div>

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
          ${c.qualification ? qualSection(c.qualification, orgs) : ""}
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
    bindOrgPicker(
      root,
      "organizer",
      "single",
      orgs,
      () => (c.organizer ? [c.organizer] : []),
      (v) => (c.organizer = v[0] ?? null),
      render,
    );
    bindOrgPicker(root, "orgs", "multi", orgs, () => c.orgs, (v) => (c.orgs = v), render);
    on("category", "input", (el) => (c.category = text(el.value)));
    on("agegroup", "input", (el) => (c.age_group = text(el.value)));
    on("bodyweight", "input", (el) => {
      const n = Number(el.value);
      c.bodyweight = el.value.trim() === "" || Number.isNaN(n) ? null : n;
    });
    // Touching the box always writes an explicit answer — there is no way
    // back to "let the app decide" from here, matching the checkbox itself
    // only ever being checked or not.
    root.querySelector<HTMLInputElement>("#registered")?.addEventListener("change", (ev) => {
      c.registered_override = (ev.currentTarget as HTMLInputElement).checked;
    });

    root.querySelector("#togglequal")?.addEventListener("click", () => {
      c.qualification = c.qualification ? null : { from: null, to: null, counts: [], standards: [blankStandard(c)] };
      render();
    });
    on("qualfrom", "change", (el) => (c.qualification!.from = text(el.value)));
    on("qualto", "change", (el) => (c.qualification!.to = text(el.value)));
    bindOrgPicker(
      root,
      "counts",
      "multi",
      orgs,
      () => c.qualification!.counts,
      (v) => (c.qualification!.counts = v),
      render,
    );
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
          s.age_group = text(el.value);
        } else if (f === "category") {
          s.category = text(el.value);
        } else {
          s.label = el.value.trim();
        }
      }),
    );

    root.querySelector("#save")?.addEventListener("click", () => void save());
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
      const saved = await api.saveCompetition(await api.serializeCompetition(c), slug);
      location.hash = `#/comp/${encodeURIComponent(saved.slug)}`;
    } catch (e) {
      const errs = e as ParseError[];
      status(Array.isArray(errs) && errs[0] ? `line ${errs[0].line}: ${errs[0].message}` : String(e), false);
    }
  }

  render();
}

export function blankStandard(c: Competition): Standard {
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

function qualSection(q: Qualification, orgs: OrgPickerState): string {
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
      ${field("Meets that count", orgPicker("counts", "multi", q.counts, orgs), "Leave empty and any meet counts.")}
      <div class="comp-marks-head">
        <span class="quick-label">Marks</span>
        <button class="btn" id="addmark">+ Add mark</button>
      </div>
      <div class="comp-hint">One row per group: the total, then the age group and class it is for. Leave both empty for a mark that applies to everyone.</div>
      ${rows}
    </section>`;
}
