import {
  api,
  effectiveRegistered,
  fmtKg,
  nextWarmupKg,
  todayStr,
  type Attempt,
  type AttemptResult,
  type Competition,
  type CompView,
  type LiftEntry,
  type ParseError,
  type WarmupSet,
} from "../api";
import { attemptGoals, fmtDay, group, marksSection, targetsSection, totalPanel, windowText } from "./comp";
import { blankStandard } from "./compedit";
import { armDelete, esc } from "./library";
import { bindOrgPicker, orgPicker, type OrgPickerState } from "./orgpicker";

type LiftKey = "snatch" | "clean_jerk";
const LIFTS: [LiftKey, string][] = [
  ["snatch", "Snatch"],
  ["clean_jerk", "Clean & Jerk"],
];

/**
 * A meet — the one screen for it, before, during and after. Condensed like a
 * read-only view, but everything on the lifts is tappable, because there is
 * no typing on the way to the common actions: between lifts you have one hand
 * and a few seconds.
 *
 * Nothing is open until it is tapped — a warmup set or an attempt, one per
 * lift. Most of the time spent on this screen is reading it, and a strip of
 * buttons under a set nobody asked about is noise.
 *
 * The meet's own details — name, date, who runs and sanctions it, the class
 * and group you entered, the entry standard — work the same way: each is a
 * chip or a row that opens its editor under it when tapped. There is no
 * separate edit screen for a meet that exists; the form is only for a new one.
 *
 * Every change saves itself, a moment after the last tap: there is no point on
 * the platform to come back and press Save, and saving once per tap of a
 * stepper would only race itself.
 */
export async function renderMeet(root: HTMLElement, slugArg: string) {
  // Renaming a meet renames its file, so the slug follows the saves.
  let slug = slugArg;
  let view: CompView;
  try {
    view = await api.viewCompetition(slug);
  } catch (e) {
    root.innerHTML = `
      <div class="screen viewer">
        <header class="topbar">
          <a class="btn" href="#/competitions">‹ Back</a>
          <h1>Cannot show this meet</h1>
          <a class="btn primary" href="#/compedit/${encodeURIComponent(slug)}">Rewrite</a>
        </header>
        <div class="view-scroll"><div class="editor-status invalid">${esc(String(e))}</div></div>
      </div>`;
    return;
  }
  // The local copy is the truth while you are tapping; the server's view is
  // only read back for what it computes (total, targets, marks), so a save
  // landing mid-tap can never roll a stepper back.
  const c = view.competition;
  // `undefined` means "follow the meet"; a number is one you tapped.
  // The warmup set and attempt whose controls are open, per lift — none until tapped.
  const warmFocus: Record<LiftKey, number | undefined> = { snatch: undefined, clean_jerk: undefined };
  const attFocus: Record<LiftKey, number | undefined> = { snatch: undefined, clean_jerk: undefined };
  // Weight for an attempt slot not declared yet — nothing is written until
  // one of its buttons says what it is.
  const drafts: Record<string, number> = {};
  // The plan a no-lift shifted down, keyed by the missed slot, so taking the
  // miss back puts it back instead of leaving the shift behind.
  const shifted: Record<string, (Attempt | null)[]> = {};
  // The attempts a warmup set raised, for the same reason: each slot's weight
  // before, and after. Keyed by the set itself, not its position — deleting
  // or adding a set moves the others, and an index would then hand one set's
  // undo to its neighbour.
  const raised = new Map<WarmupSet, { from: (number | null)[]; to: (number | null)[] }>();
  // Which detail has its editor open: a field name, or `mark:<i>` for a row
  // of the entry standard. One at a time, like an attempt.
  let detail: string | null = null;
  const orgs: OrgPickerState = { organizations: await api.listOrganizations(), adding: null };
  let saveStatus = "";
  let saveOk = true;
  let saveTimer: number | undefined;
  let saving = false;
  let dirtyAgain = false;

  const liftOf = (k: string): LiftEntry => (k === "snatch" ? c.snatch : c.clean_jerk);

  /** Mirrors `Competition::closed`: out of attempts, or a snatch overtaken
   *  by a clean & jerk already taken. */
  function liftOver(k: LiftKey): boolean {
    const taken = (e: LiftEntry) => e.attempts.filter((a) => a && takenResult(a)).length;
    return taken(liftOf(k)) === liftOf(k).attempts.length || (k === "snatch" && taken(c.clean_jerk) > 0);
  }

  function currentWarm(k: LiftKey): number | null {
    if (liftOver(k)) return null;
    const f = warmFocus[k];
    return f !== undefined && f < liftOf(k).warmup.length ? f : null;
  }

  function currentAtt(k: LiftKey): number | null {
    return attFocus[k] ?? null;
  }

  /** What an undeclared slot starts at: the usual next jump after a make, the
   *  same bar after a miss, and just above the warmup for an opener. */
  function suggested(k: LiftKey, i: number): number {
    const prev = liftOf(k).attempts.slice(0, i).reverse().find((a) => a !== null);
    if (prev) return prev.result === "good" ? prev.kg + 1 : prev.kg;
    const w = liftOf(k).warmup;
    return w.length ? w[w.length - 1].kg + 5 : 20;
  }

  /** Both lifts closed: the meet is a result now, and its details are read
   *  rather than filled in — they move next to what they describe. */
  function finished(): boolean {
    return liftOver("snatch") && liftOver("clean_jerk");
  }

  function render() {
    const done = finished();
    const scroll = root.querySelector(".view-scroll")?.scrollTop ?? 0;
    root.innerHTML = `
      <div class="screen viewer">
        <header class="topbar">
          <a class="btn" href="#/competitions">‹ Back</a>
          <span class="rc-save ${saveOk ? "" : "error"}">${esc(saveStatus)}</span>
        </header>
        <div class="view-scroll">
          <button class="view-title rc-title ${detail === "name" ? "focus" : ""}" data-act="detail" data-f="name">🏆 ${
            esc(c.name) || "(no name)"
          }${
            done && c.date
              ? ` <span class="rc-title-date ${detail === "date" ? "focus" : ""}" data-act="detail" data-f="date">${fmtDay(c.date)}</span>`
              : ""
          }</button>
          ${detail === "name" ? detailStrip("name") : ""}
          ${done ? stripFor(["date"]) : metaChips()}
          ${totalPanel(view, done ? entryChips() : "")}
          ${done ? stripFor(["category", "age", "bodyweight"]) : ""}
          ${targetsSection(view)}
          ${LIFTS.map(([k, name]) => liftCard(k, name)).join("")}
          ${marksSection(view)}
          ${standardSection()}
          ${done ? hostChips() : ""}
          <div class="comp-danger"><button class="btn danger" id="deletemeet">🗑 Delete this meet</button></div>
        </div>
      </div>`;
    const el = root.querySelector(".view-scroll");
    if (el) el.scrollTop = scroll;
    bindDetails();
  }

  // --- details ---

  /** The meet's details as chips: tap one to edit it under the row. A detail
   *  not written yet is a dashed `+` chip, so what is missing is visible
   *  without a form to scan. */
  function chip(f: string, value: string | null, empty: string): string {
    return `<button class="rc-chip rc-meta-chip ${value ? "" : "rc-add"} ${detail === f ? "focus" : ""}"
               data-act="detail" data-f="${f}">${value ?? empty}</button>`;
  }

  /** The editor for whichever of these details is open, if any. */
  function stripFor(fields: string[]): string {
    return detail && fields.includes(detail) ? detailStrip(detail) : "";
  }

  /** What you entered as, for the total of a finished meet: only what was
   *  written, since an empty slot there is not a question any more. */
  function entryChips(): string {
    const chips = [
      c.category ? chip("category", esc(c.category), "") : "",
      c.age_group ? chip("age", esc(c.age_group), "") : "",
      c.bodyweight != null ? chip("bodyweight", `${fmtKg(c.bodyweight)} kg bw`, "") : "",
    ].join("");
    return chips ? `<div class="rc-meta comp-total-entry">${chips}</div>` : "";
  }

  /** Who ran and sanctioned a finished meet, at the foot of the page. */
  function hostChips(): string {
    const chips = [
      c.organizer ? chip("organizer", `🏛 ${esc(c.organizer)}`, "") : "",
      c.orgs.length ? chip("orgs", esc(c.orgs.join(" · ")), "") : "",
    ].join("");
    return chips ? `<div class="rc-meta">${chips}</div>${stripFor(["organizer", "orgs"])}` : "";
  }

  function metaChips(): string {
    const registered = effectiveRegistered(c);
    // Registering is a question before the meet: once its date is past, or a
    // warmup is ticked or an attempt taken, you are evidently in.
    const begun = [c.snatch, c.clean_jerk].some(
      (e) => e.warmup.some((w) => w.done) || e.attempts.some((a) => a && takenResult(a)),
    );
    const over = begun || (c.date !== null && c.date < todayStr());
    const chips = [
      chip("date", c.date ? `📅 ${fmtDay(c.date)}` : null, "+ date"),
      chip("organizer", c.organizer ? `🏛 ${esc(c.organizer)}` : null, "+ federation"),
      chip("orgs", c.orgs.length ? esc(c.orgs.join(" · ")) : null, "+ sanctioned by"),
      chip("category", c.category ? esc(c.category) : null, "+ weight class"),
      chip("age", c.age_group ? esc(c.age_group) : null, "+ age group"),
      chip("bodyweight", c.bodyweight != null ? `${fmtKg(c.bodyweight)} kg bw` : null, "+ bodyweight"),
      // One tap flips it: there is nothing to type.
      over
        ? ""
        : `<button class="rc-chip rc-meta-chip ${registered ? "" : "warn"}" data-act="registered">${
            registered ? "✓ registered" : "⚠ not registered"
          }</button>`,
    ];
    const open = detail && detail !== "name" && !detail.startsWith("mark:") && detail !== "window" && detail !== "counts";
    return `<div class="rc-meta">${chips.join("")}</div>${open ? detailStrip(detail!) : ""}`;
  }

  function detailStrip(f: string): string {
    const input = (field: string, value: string, attrs = "") =>
      `<input class="text-input rc-grow" data-field="${field}" value="${esc(value)}" ${attrs}>`;
    let body: string;
    switch (f) {
      case "name":
        body = input("name", c.name, `placeholder="Meet name"`);
        break;
      case "date":
        body = input("date", c.date ?? "", `type="date"`);
        break;
      case "category":
        body = input("category", c.category ?? "", `placeholder="89 kg"`);
        break;
      case "age":
        body = input("age", c.age_group ?? "", `placeholder="M40"`);
        break;
      case "bodyweight":
        body = input("bodyweight", c.bodyweight != null ? String(c.bodyweight) : "", `type="number" step="0.1" inputmode="decimal" placeholder="88.4"`);
        break;
      case "organizer":
        body = `<div class="rc-grow">${orgPicker("organizer", "single", c.organizer ? [c.organizer] : [], orgs)}</div>`;
        break;
      case "orgs":
        body = `<div class="rc-grow">${orgPicker("orgs", "multi", c.orgs, orgs)}</div>`;
        break;
      case "window": {
        const q = c.qualification!;
        body = `<div class="rc-grow rc-col">
                  <span class="rc-strip-label">Results count from</span>${input("qualfrom", q.from ?? "", `type="date"`)}
                  <span class="rc-strip-label">until</span>${input("qualto", q.to ?? "", `type="date"`)}
                </div>`;
        break;
      }
      case "counts":
        body = `<div class="rc-grow rc-col">
                  ${orgPicker("counts", "multi", c.qualification!.counts, orgs)}
                  <span class="comp-hint">Leave empty and any meet counts.</span>
                </div>`;
        break;
      default: {
        const i = Number(f.slice("mark:".length));
        const s = c.qualification!.standards[i];
        const m = (k: string, value: string, attrs: string) =>
          `<input class="text-input rc-grow" data-field="mark" data-k="${k}" data-i="${i}" value="${esc(value)}" ${attrs}>`;
        body = `<div class="rc-grow rc-col">
                  ${m("total", s.total > 0 ? fmtKg(s.total) : "", `type="number" inputmode="decimal" placeholder="total"`)}
                  <div class="rc-row">
                    ${m("age", s.age_group ?? "", `placeholder="age group"`)}
                    ${m("category", s.category ?? "", `placeholder="class"`)}
                  </div>
                  ${m("label", s.label, `placeholder="name (optional)"`)}
                  <span class="comp-hint">Leave age group and class empty for a mark that applies to everyone.</span>
                  <button class="btn danger" data-act="markdel" data-i="${i}">🗑 Remove this mark</button>
                </div>`;
      }
    }
    return `
      <div class="rc-strip">
        <div class="rc-row">
          ${body}
          <button class="btn primary rc-done" data-act="detail" data-f="${f}">Done</button>
        </div>
      </div>`;
  }

  /** What it takes to get into this meet, edited the same way: the window and
   *  which meets count as chips, each mark as a row. Whether a mark is already
   *  met comes from the last save — it is the server that reads the other
   *  meets — so a row being typed into shows no verdict until it lands. */
  function standardSection(): string {
    const q = c.qualification;
    if (!q) {
      return `
        <section class="view-part">
          <div class="view-part-head"><h2>Entry standard</h2></div>
          <div class="comp-hint">What it takes to get <em>into</em> this meet — a total, the window it must
            be set in, and which federations' meets count.</div>
          <button class="comp-set rc-chip rc-add" data-act="qualadd">+ entry standard</button>
        </section>`;
    }
    const window = windowText(q);
    // Your row met is the whole answer, so it leads — the same rule the list
    // uses: the narrowest row for the group you said, or the only row there is.
    const said = c.age_group || c.category;
    const achieved =
      said || view.standards.length === 1 ? view.standards.find((s) => s.yours && s.met_by) : undefined;
    const banner = achieved
      ? `<div class="comp-qualified">✓ Minimum achieved — best ${fmtKg(achieved.met_total ?? 0)} at ${esc(achieved.met_by!)}${
          achieved.met_on ? ` (${fmtDay(achieved.met_on)})` : ""
        }${achieved.met_category ? ` · ${esc(achieved.met_category)}` : ""}</div>`
      : "";
    const chips = `
      <div class="rc-meta">
        <button class="rc-chip rc-meta-chip ${window ? "" : "rc-add"} ${detail === "window" ? "focus" : ""}"
                data-act="detail" data-f="window">${window ? `🗓 ${window}` : "+ qualifying window"}</button>
        <button class="rc-chip rc-meta-chip ${detail === "counts" ? "focus" : ""}" data-act="detail" data-f="counts">${
          q.counts.length ? `counts: ${esc(q.counts.join(" · "))}` : "any meet counts"
        }</button>
      </div>
      ${detail === "window" || detail === "counts" ? detailStrip(detail) : ""}`;
    const rows = q.standards
      .map((s, i) => {
        const sv = view.standards[i];
        const current = sv && sv.total === s.total ? sv : null;
        const label = [group(s.age_group, s.category), s.label].filter(Boolean).join(" · ");
        const met = !current
          ? ""
          : current.met_by
            ? `<span class="comp-mark-state clinched">✓ ${fmtKg(current.met_total ?? 0)} at ${esc(current.met_by)}${
                current.met_on ? ` (${fmtDay(current.met_on)})` : ""
              }${current.met_category ? ` · ${esc(current.met_category)}` : ""}</span>`
            : `<span class="comp-mark-state open">not yet</span>`;
        const yours = current?.yours ?? true;
        return `<button class="comp-mark rc-mark ${yours ? "yours" : "other"} ${detail === `mark:${i}` ? "focus" : ""}"
                        data-act="detail" data-f="mark:${i}">
                  <span class="comp-mark-total">${s.total > 0 ? fmtKg(s.total) : "—"}</span>
                  <span class="comp-mark-label">${esc(label) || "everyone"}${
                    said && yours && current ? ` <span class="comp-yours">yours</span>` : ""
                  }</span>
                  ${met}
                </button>${detail === `mark:${i}` ? detailStrip(detail) : ""}`;
      })
      .join("");
    return `
      <section class="view-part">
        <div class="view-part-head"><h2>Entry standard</h2></div>
        ${banner}
        ${chips}
        ${rows}
        <button class="comp-set rc-chip rc-add" data-act="markadd">+ mark</button>
      </section>`;
  }

  /** The org pickers bind per render, since they own their own events. */
  function bindDetails() {
    const orgField = (f: string, get: () => string[], set: (v: string[]) => void) =>
      bindOrgPicker(root, f, f === "organizer" ? "single" : "multi", orgs, get, (v) => {
        set(v);
        changed();
      }, render);
    if (detail === "organizer") orgField("organizer", () => (c.organizer ? [c.organizer] : []), (v) => (c.organizer = v[0] ?? null));
    if (detail === "orgs") orgField("orgs", () => c.orgs, (v) => (c.orgs = v));
    if (detail === "counts") orgField("counts", () => c.qualification!.counts, (v) => (c.qualification!.counts = v));
    const del = root.querySelector<HTMLButtonElement>("#deletemeet");
    if (del) {
      armDelete(del, del.textContent ?? "🗑 Delete this meet", async () => {
        window.clearTimeout(saveTimer);
        saveTimer = undefined;
        await api.deleteCompetition(slug);
        location.hash = "#/competitions";
      });
    }
  }

  function applyField(el: HTMLInputElement) {
    const v = el.value.trim();
    const text = v === "" ? null : v;
    switch (el.dataset.field) {
      case "name":
        // A meet needs a name to have a file; an emptied field keeps the old one.
        if (text) c.name = text;
        break;
      case "date":
        c.date = text;
        break;
      case "category":
        c.category = text;
        break;
      case "age":
        c.age_group = text;
        break;
      case "bodyweight": {
        const n = Number(v);
        c.bodyweight = text === null || Number.isNaN(n) || n <= 0 ? null : n;
        break;
      }
      case "qualfrom":
        c.qualification!.from = text;
        break;
      case "qualto":
        c.qualification!.to = text;
        break;
      case "mark": {
        const s = c.qualification!.standards[Number(el.dataset.i)];
        const k = el.dataset.k;
        if (k === "total") {
          const n = Number(v);
          s.total = Number.isNaN(n) || n < 0 ? 0 : n;
        } else if (k === "age") s.age_group = text;
        else if (k === "category") s.category = text;
        else s.label = v;
        break;
      }
    }
  }

  function liftCard(k: LiftKey, name: string): string {
    const entry = liftOf(k);
    const best = k === "snatch" ? view.snatch_best : view.clean_jerk_best;
    const goingDown = k === "snatch" ? view.snatch_going_down : view.clean_jerk_going_down;
    const notes = k === "snatch" ? view.snatch_notes_html : view.clean_jerk_notes_html;
    const wf = currentWarm(k);
    const af = currentAtt(k);

    const over = liftOver(k);
    // A finished lift's warmups are history: a small row to read, not sets to plan.
    const chips = entry.warmup
      .map((w, i) =>
        over
          ? `<span class="comp-set ${w.done ? "done" : ""}">${w.done ? "✓" : "○"} ${fmtKg(w.kg)}${
              w.reps > 1 ? ` × ${w.reps}` : ""
            }</span>`
          : `<button class="comp-set rc-chip ${w.done ? "done" : ""} ${i === wf ? "focus" : ""}"
                   data-act="warm" data-lift="${k}" data-i="${i}">${w.done ? "✓" : "○"} ${fmtKg(w.kg)}${
                     w.reps > 1 ? ` × ${w.reps}` : ""
                   }</button>`,
      )
      .join("");
    const warmStrip =
      wf === null
        ? ""
        : (() => {
            const w = entry.warmup[wf];
            return `
              <div class="rc-strip">
                ${stepper(k, "warmkg", wf, w.kg, [-5, -1, 1, 5])}
                <div class="rc-row">
                  <button class="btn rc-step rc-rstep" data-act="reps" data-d="-1" data-lift="${k}" data-i="${wf}" ${
                    w.reps <= 1 ? "disabled" : ""
                  }>−</button>
                  <span class="rc-reps">× ${w.reps}</span>
                  <button class="btn rc-step rc-rstep" data-act="reps" data-d="1" data-lift="${k}" data-i="${wf}">+</button>
                  <button class="btn ${w.done ? "" : "primary"} rc-grow" data-act="done" data-lift="${k}" data-i="${wf}">${
                    w.done ? "↺ Not done" : "✓ Done"
                  }</button>
                  <button class="btn danger" data-act="warmdel" data-lift="${k}" data-i="${wf}">✕</button>
                </div>
              </div>`;
          })();

    const attempts = entry.attempts
      .map((a, i) => {
        const cls = [a ? a.result : "empty", i === af ? "focus" : ""].join(" ");
        const mark = !a
          ? `<span class="comp-result declared">not declared</span>`
          : a.result === "good"
            ? `<span class="comp-result good">✓ good lift</span>`
            : a.result === "miss"
              ? `<span class="comp-result miss">✗ no lift</span>`
              : `<span class="comp-result declared">${a.result}</span>`;
        const warn = goingDown.includes(i + 1) ? `<span class="meta-warn">⚠ lighter than the one before</span>` : "";
        return `<button class="comp-attempt rc-attempt ${cls}" data-act="att" data-lift="${k}" data-i="${i}">
                  <span class="comp-attempt-n">${i + 1}</span>
                  <span class="comp-attempt-kg">${a ? fmtKg(a.kg) : "—"}</span>
                  ${mark}${warn}
                </button>${attemptGoals(view, k, i + 1, a ? a.kg : (drafts[`${k}:${i}`] ?? null))}`;
      })
      .join("");
    const attStrip =
      af === null
        ? ""
        : (() => {
            const a = entry.attempts[af];
            const kg = a ? a.kg : (drafts[`${k}:${af}`] ?? suggested(k, af));
            const res = (r: AttemptResult, label: string) =>
              `<button class="btn rc-grow rc-res ${r} ${a && a.result === r ? "on" : ""}"
                       data-act="result" data-r="${r}" data-lift="${k}" data-i="${af}">${label}</button>`;
            return `
              <div class="rc-strip">
                <div class="rc-strip-label">Attempt ${af + 1}${a ? "" : " — not declared yet"}</div>
                ${stepper(k, "attkg", af, kg, [-1, 1])}
                <div class="rc-row">
                  ${res("planned", a ? "Planned" : "Plan")}
                  ${res("declared", a ? "Declared" : "Declare")}
                </div>
                <div class="rc-row">
                  ${res("good", "✓ Good")}
                  ${res("miss", "✗ No lift")}
                </div>
              </div>`;
          })();

    return `
      <section class="view-part">
        <div class="view-part-head">
          <h2>${name}</h2>
          <span class="view-part-total">${best != null ? `best ${fmtKg(best)}` : "—"}</span>
        </div>
        <div class="comp-warmup ${over ? "over" : ""}">
          ${chips}
          ${over ? "" : `<button class="comp-set rc-chip rc-add" data-act="warmadd" data-lift="${k}">+ set</button>`}
        </div>
        ${warmStrip}
        <div class="comp-attempts">${attempts}</div>
        ${attStrip}
        ${notes ? `<div class="view-notes">${notes}</div>` : ""}
      </section>`;
  }

  function stepper(k: LiftKey, act: string, i: number, kg: number, steps: number[]): string {
    const btn = (d: number) =>
      `<button class="btn rc-step" data-act="${act}" data-d="${d}" data-lift="${k}" data-i="${i}">${
        d > 0 ? "+" : "−"
      }${Math.abs(d)}</button>`;
    return `
      <div class="rc-row">
        ${steps.filter((d) => d < 0).map(btn).join("")}
        <input class="text-input rc-kg" type="number" inputmode="decimal" step="0.5"
               data-act="${act}" data-lift="${k}" data-i="${i}" value="${fmtKg(kg)}">
        ${steps.filter((d) => d > 0).map(btn).join("")}
      </div>`;
  }

  // --- actions ---

  function setAttemptKg(k: LiftKey, i: number, kg: number) {
    if (!(kg > 0)) return;
    const a = liftOf(k).attempts[i];
    if (a) {
      a.kg = kg;
      changed();
    } else {
      drafts[`${k}:${i}`] = kg;
      render();
    }
  }

  /**
   * After a no-lift you go again at the same weight, so the rest of the plan
   * moves one attempt later: planned 100/103/106 and a miss at 100 becomes
   * 100✗/100/103. Only attempts not taken yet move, and whatever moves is
   * planned again — a weight declared before the miss is no longer the one
   * you will tell the table. A good lift changes nothing.
   */
  function shiftAfterMiss(k: LiftKey, i: number) {
    const atts = liftOf(k).attempts;
    const missed = atts[i]!;
    let end = i + 1;
    while (end < atts.length && !(atts[end] && (atts[end]!.result === "good" || atts[end]!.result === "miss"))) end++;
    if (end === i + 1) return;
    shifted[`${k}:${i}`] = atts.slice(i + 1, end).map((a) => (a ? { ...a } : null));
    const carried: (number | null)[] = [missed.kg, ...atts.slice(i + 1, end - 1).map((a) => (a ? a.kg : null))];
    carried.forEach((kg, j) => {
      atts[i + 1 + j] = kg === null ? null : { kg, result: "planned" };
    });
  }

  function unshift(k: LiftKey, i: number) {
    const before = shifted[`${k}:${i}`];
    if (!before) return;
    delete shifted[`${k}:${i}`];
    const atts = liftOf(k).attempts;
    before.forEach((a, j) => {
      const now = atts[i + 1 + j];
      // Something taken since is a fact, not part of the plan being put back.
      if (!(now && (now.result === "good" || now.result === "miss"))) atts[i + 1 + j] = a;
    });
  }

  /**
   * A warmup made at or above the opener says the opener is too light: at the
   * same weight it goes up by one, heavier and it becomes that weight. Only
   * while the opener is still yours to change — planned or declared, never
   * taken — and never downwards. Planned attempts after it that would now be
   * no heavier than the one before go to one above it, so the plan still
   * climbs; a declared one is left for you, since the table already has it.
   * Ticking the set off again puts every moved weight back, unless it has
   * been changed since.
   */
  function toggleWarmDone(k: LiftKey, i: number) {
    const set = liftOf(k).warmup[i];
    set.done = !set.done;
    const opener = liftOf(k).attempts[0];
    const atts = liftOf(k).attempts;
    const weights = () => atts.map((a) => (a ? a.kg : null));
    if (!set.done) {
      const r = raised.get(set);
      raised.delete(set);
      r?.to.forEach((to, j) => {
        const a = atts[j];
        if (a && !takenResult(a) && a.kg === to && r.from[j] !== null) a.kg = r.from[j]!;
      });
      return;
    }
    if (!opener || takenResult(opener) || set.kg < opener.kg) return;
    const from = weights();
    opener.kg = set.kg === opener.kg ? opener.kg + 1 : set.kg;
    for (let j = 1; j < atts.length; j++) {
      const a = atts[j];
      const prev = atts[j - 1];
      if (a && prev && a.result === "planned" && a.kg <= prev.kg) a.kg = prev.kg + 1;
    }
    raised.set(set, { from, to: weights() });
  }

  /**
   * The sets still to do climb, lightest first, in the slots the done ones
   * leave free. A done set stays where it is: that is the order you took
   * them in.
   */
  function sortPending(k: LiftKey) {
    const w = liftOf(k).warmup;
    const pending = w.filter((s) => !s.done).sort((a, b) => a.kg - b.kg);
    let p = 0;
    w.forEach((s, i) => {
      if (!s.done) w[i] = pending[p++];
    });
  }

  function takenResult(a: Attempt): boolean {
    return a.result === "good" || a.result === "miss";
  }

  function onClick(ev: Event) {
    const t = (ev.target as HTMLElement).closest<HTMLElement>("[data-act]");
    if (!t || t.tagName === "INPUT") return;
    const k = t.dataset.lift as LiftKey;
    const i = Number(t.dataset.i);
    const entry = k ? liftOf(k) : null;
    const d = Number(t.dataset.d);
    switch (t.dataset.act) {
      case "detail": {
        // Tapping the open one (or its Done) closes it. Any value still in an
        // input is taken first — "change" may not have fired yet on a phone.
        const before = JSON.stringify(c);
        root.querySelectorAll<HTMLInputElement>("input[data-field]").forEach(applyField);
        const f = t.dataset.f!;
        detail = detail === f ? null : f;
        orgs.adding = null;
        // Opening or closing is not an edit: a save would restamp `updated`,
        // which is what a restore compares on.
        if (JSON.stringify(c) !== before) changed();
        else render();
        break;
      }
      case "registered":
        c.registered_override = !effectiveRegistered(c);
        changed();
        break;
      case "qualadd":
        c.qualification = { from: null, to: null, counts: [], standards: [blankStandard(c)] };
        detail = "mark:0";
        changed();
        break;
      case "markadd":
        c.qualification!.standards.push(blankStandard(c));
        detail = `mark:${c.qualification!.standards.length - 1}`;
        changed();
        break;
      case "markdel":
        c.qualification!.standards.splice(i, 1);
        // The last mark gone is the standard gone: a window with nothing to
        // be set inside it is not a rule.
        if (c.qualification!.standards.length === 0) c.qualification = null;
        detail = null;
        changed();
        break;
      case "warm":
        // Tapping the open set closes it, as an attempt does; Done is its own
        // button, so a tap that only meant to look never ticks a set off.
        warmFocus[k] = currentWarm(k) === i ? undefined : i;
        render();
        break;
      case "done":
        toggleWarmDone(k, i);
        // Ticked off moves on to the next set still to do — the one you are
        // about to load — or closes when there is none. Undoing stays put.
        if (entry!.warmup[i].done) {
          const next = entry!.warmup.findIndex((w, j) => j > i && !w.done);
          warmFocus[k] = next < 0 ? undefined : next;
        } else {
          warmFocus[k] = i;
        }
        changed();
        break;
      case "warmkg":
        entry!.warmup[i].kg = Math.max(0.5, entry!.warmup[i].kg + d);
        warmFocus[k] = i;
        changed();
        break;
      case "reps":
        // Never below one: a set of none is a set to delete, which ✕ is for.
        entry!.warmup[i].reps = Math.max(1, entry!.warmup[i].reps + d);
        warmFocus[k] = i;
        changed();
        break;
      case "warmdel":
        entry!.warmup.splice(i, 1);
        sortPending(k);
        warmFocus[k] = undefined;
        changed();
        break;
      case "warmadd": {
        const set = { kg: nextWarmupKg(entry!), reps: 1, done: false };
        entry!.warmup.push(set);
        sortPending(k);
        // The new set is open wherever it sorted to, ready to be weighed.
        warmFocus[k] = entry!.warmup.indexOf(set);
        changed();
        break;
      }
      case "att":
        attFocus[k] = currentAtt(k) === i ? undefined : i;
        render();
        break;
      case "attkg": {
        const a = entry!.attempts[i];
        const kg = a ? a.kg : (drafts[`${k}:${i}`] ?? suggested(k, i));
        attFocus[k] = i;
        setAttemptKg(k, i, kg + d);
        break;
      }
      case "result": {
        const r = t.dataset.r as AttemptResult;
        const a = entry!.attempts[i];
        const was = a?.result;
        if (a) {
          a.result = r;
        } else {
          entry!.attempts[i] = { kg: drafts[`${k}:${i}`] ?? suggested(k, i), result: r };
          delete drafts[`${k}:${i}`];
        }
        if (r === "miss" && was !== "miss") shiftAfterMiss(k, i);
        else if (was === "miss" && r !== "miss") unshift(k, i);
        // A result closes the attempt; planning or declaring keeps it open, so
        // the weight can still be nudged before the bar is called.
        attFocus[k] = r === "good" || r === "miss" ? undefined : i;
        changed();
        break;
      }
    }
  }

  function onChange(ev: Event) {
    const el = ev.target as HTMLInputElement;
    if (el.tagName === "INPUT" && el.dataset.field) {
      // Saved without a redraw: the input already shows what was typed, and
      // redrawing under a finger on its way to the next field loses the tap.
      applyField(el);
      changed(false);
      return;
    }
    if (el.tagName !== "INPUT" || !el.dataset.act) return;
    const k = el.dataset.lift as LiftKey;
    const i = Number(el.dataset.i);
    const n = Number(el.value);
    if (Number.isNaN(n) || n <= 0) {
      render();
      return;
    }
    if (el.dataset.act === "warmkg") {
      liftOf(k).warmup[i].kg = n;
      warmFocus[k] = i;
      changed();
    } else if (el.dataset.act === "attkg") {
      attFocus[k] = i;
      setAttemptKg(k, i, n);
    }
  }

  // --- saving ---

  function changed(redraw = true) {
    saveStatus = "…";
    saveOk = true;
    if (redraw) render();
    else {
      const s = root.querySelector(".rc-save");
      if (s) s.textContent = saveStatus;
    }
    window.clearTimeout(saveTimer);
    saveTimer = window.setTimeout(() => void save(), 400);
  }

  async function save() {
    saveTimer = undefined;
    if (saving) {
      dirtyAgain = true;
      return;
    }
    saving = true;
    try {
      const sentName = c.name;
      const saved = await api.saveCompetition(await api.serializeCompetition(writable(c)), slug);
      if (saved.slug !== slug) {
        slug = saved.slug;
        // Not a navigation: the screen is already showing this meet.
        history.replaceState(null, "", `#/comp/${encodeURIComponent(slug)}`);
      }
      view = await api.viewCompetition(slug);
      // The store mints an id for a file that had none, and may add a counter
      // to a name another meet already has; take both back, or the next save
      // would mint yet another id and the meet would stop being itself.
      c.id = view.competition.id;
      if (c.name === sentName) c.name = view.competition.name;
      saveStatus = "✓ saved";
      saveOk = true;
    } catch (e) {
      const errs = e as ParseError[];
      saveStatus = Array.isArray(errs) && errs[0] ? `line ${errs[0].line}: ${errs[0].message}` : String(e);
      saveOk = false;
    }
    saving = false;
    if (dirtyAgain) {
      dirtyAgain = false;
      void save();
      return;
    }
    // Recomputed numbers, over the local copy the taps have been writing to —
    // unless a field is being typed into, which a redraw would take away.
    const typing = root.contains(document.activeElement) && document.activeElement?.tagName === "INPUT";
    if (saveTimer === undefined && !typing) render();
    else if (typing) {
      const s = root.querySelector(".rc-save");
      if (s) s.textContent = saveStatus;
    }
  }

  root.addEventListener("click", onClick);
  root.addEventListener("change", onChange);
  render();

  return () => {
    root.removeEventListener("click", onClick);
    root.removeEventListener("change", onChange);
    // Leaving mid-debounce must not drop the last tap.
    if (saveTimer !== undefined) {
      window.clearTimeout(saveTimer);
      void save();
    }
  };
}

/** The meet as it can be written: a mark still waiting for its number would
 *  not parse, so it stays on screen but out of the file until it has one. */
function writable(c: Competition): Competition {
  const q = c.qualification;
  if (!q || q.standards.every((s) => s.total > 0)) return c;
  const standards = q.standards.filter((s) => s.total > 0);
  return { ...c, qualification: standards.length ? { ...q, standards } : null };
}
