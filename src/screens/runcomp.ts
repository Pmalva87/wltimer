import { api, fmtKg, nextWarmupKg, type Attempt, type AttemptResult, type CompView, type LiftEntry, type ParseError } from "../api";
import { attemptGoals, marksSection, standardsSection, targetsSection, totalPanel } from "./comp";
import { esc } from "./library";

type LiftKey = "snatch" | "clean_jerk";
const LIFTS: [LiftKey, string][] = [
  ["snatch", "Snatch"],
  ["clean_jerk", "Clean & Jerk"],
];

/**
 * The platform console: the view screen's condensed layout, made tappable.
 * There is no typing on the way to the common actions — between lifts you have
 * one hand and a few seconds — so each lift has a single *focused* warmup set
 * and attempt, and one strip of big buttons acts on it. Focus follows the
 * meet on its own (the next set not done, the next attempt without a result);
 * tapping anything else moves it there, for the corrections.
 *
 * Every change saves itself, a moment after the last tap: there is no point on
 * the platform to come back and press Save, and saving once per tap of a
 * stepper would only race itself.
 */
export async function renderRunComp(root: HTMLElement, slug: string) {
  let view: CompView;
  try {
    view = await api.viewCompetition(slug);
  } catch (e) {
    root.innerHTML = `
      <div class="screen viewer">
        <header class="topbar">
          <a class="btn" href="#/comp/${encodeURIComponent(slug)}">‹ Back</a>
          <h1>Cannot run this meet</h1>
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
  const warmFocus: Record<LiftKey, number | undefined> = { snatch: undefined, clean_jerk: undefined };
  const attFocus: Record<LiftKey, number | undefined> = { snatch: undefined, clean_jerk: undefined };
  // Weight for an attempt slot not declared yet — nothing is written until
  // one of its buttons says what it is.
  const drafts: Record<string, number> = {};
  // The plan a no-lift shifted down, keyed by the missed slot, so taking the
  // miss back puts it back instead of leaving the shift behind.
  const shifted: Record<string, (Attempt | null)[]> = {};
  // The attempts a warmup set raised, keyed by that set, for the same reason:
  // each slot's weight before, and after.
  const raised: Record<string, { from: (number | null)[]; to: (number | null)[] }> = {};
  let saveStatus = "";
  let saveOk = true;
  let saveTimer: number | undefined;
  let saving = false;
  let dirtyAgain = false;

  const liftOf = (k: string): LiftEntry => (k === "snatch" ? c.snatch : c.clean_jerk);

  function currentWarm(k: LiftKey): number | null {
    const f = warmFocus[k];
    if (f !== undefined && f < liftOf(k).warmup.length) return f;
    const i = liftOf(k).warmup.findIndex((w) => !w.done);
    return i < 0 ? null : i;
  }

  function currentAtt(k: LiftKey): number | null {
    const f = attFocus[k];
    if (f !== undefined) return f;
    const i = liftOf(k).attempts.findIndex((a) => a === null || a.result === "planned" || a.result === "declared");
    return i < 0 ? null : i;
  }

  /** What an undeclared slot starts at: the usual next jump after a make, the
   *  same bar after a miss, and just above the warmup for an opener. */
  function suggested(k: LiftKey, i: number): number {
    const prev = liftOf(k).attempts.slice(0, i).reverse().find((a) => a !== null);
    if (prev) return prev.result === "good" ? prev.kg + 1 : prev.kg;
    const w = liftOf(k).warmup;
    return w.length ? w[w.length - 1].kg + 5 : 20;
  }

  function render() {
    const scroll = root.querySelector(".view-scroll")?.scrollTop ?? 0;
    root.innerHTML = `
      <div class="screen viewer">
        <header class="topbar">
          <a class="btn" href="#/comp/${encodeURIComponent(slug)}">‹ Back</a>
          <h1>${esc(c.name)}</h1>
          <span class="rc-save ${saveOk ? "" : "error"}">${esc(saveStatus)}</span>
        </header>
        <div class="view-scroll">
          ${totalPanel(view)}
          ${targetsSection(view)}
          ${LIFTS.map(([k, name]) => liftCard(k, name)).join("")}
          ${marksSection(view)}
          ${standardsSection(view.standards, c.age_group, c.category, c.qualification)}
          <div class="comp-hint rc-foot">Notes, reps and the rest of the meet are under
            <a href="#/compedit/${encodeURIComponent(slug)}">Edit</a>.</div>
        </div>
      </div>`;
    const el = root.querySelector(".view-scroll");
    if (el) el.scrollTop = scroll;
  }

  function liftCard(k: LiftKey, name: string): string {
    const entry = liftOf(k);
    const best = k === "snatch" ? view.snatch_best : view.clean_jerk_best;
    const goingDown = k === "snatch" ? view.snatch_going_down : view.clean_jerk_going_down;
    const notes = k === "snatch" ? view.snatch_notes_html : view.clean_jerk_notes_html;
    const wf = currentWarm(k);
    const af = currentAtt(k);

    const chips = entry.warmup
      .map(
        (w, i) =>
          `<button class="comp-set rc-chip ${w.done ? "done" : ""} ${i === wf ? "focus" : ""}"
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
                  <button class="btn rc-reps" data-act="reps" data-lift="${k}" data-i="${wf}">× ${w.reps}</button>
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
        <div class="comp-warmup">
          ${chips}
          <button class="comp-set rc-chip rc-add" data-act="warmadd" data-lift="${k}">+ set</button>
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
    const key = `${k}:${i}`;
    const opener = liftOf(k).attempts[0];
    const atts = liftOf(k).attempts;
    const weights = () => atts.map((a) => (a ? a.kg : null));
    if (!set.done) {
      const r = raised[key];
      delete raised[key];
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
    raised[key] = { from, to: weights() };
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
      case "warm":
        // A second tap on the focused set is the shortcut for Done.
        if (currentWarm(k) === i) {
          toggleWarmDone(k, i);
          warmFocus[k] = undefined;
          changed();
        } else {
          warmFocus[k] = i;
          render();
        }
        break;
      case "done":
        toggleWarmDone(k, i);
        warmFocus[k] = entry!.warmup[i].done ? undefined : i;
        changed();
        break;
      case "warmkg":
        entry!.warmup[i].kg = Math.max(0.5, entry!.warmup[i].kg + d);
        warmFocus[k] = i;
        changed();
        break;
      case "reps":
        // 1 → 5 and round again: more than five is not a competition warmup.
        entry!.warmup[i].reps = (entry!.warmup[i].reps % 5) + 1;
        warmFocus[k] = i;
        changed();
        break;
      case "warmdel":
        entry!.warmup.splice(i, 1);
        warmFocus[k] = undefined;
        changed();
        break;
      case "warmadd": {
        entry!.warmup.push({ kg: nextWarmupKg(entry!), reps: 1, done: false });
        warmFocus[k] = entry!.warmup.length - 1;
        changed();
        break;
      }
      case "att":
        attFocus[k] = currentAtt(k) === i ? undefined : i;
        // Tapping the focused one again hands focus back to the meet — unless
        // that lands right back on it, in which case it simply stays.
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
        // A result moves on to the next attempt; planning or declaring stays
        // put, so the weight can still be nudged before the bar is called.
        attFocus[k] = r === "good" || r === "miss" ? undefined : i;
        changed();
        break;
      }
    }
  }

  function onChange(ev: Event) {
    const el = ev.target as HTMLInputElement;
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

  function changed() {
    saveStatus = "…";
    saveOk = true;
    render();
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
      await api.saveCompetition(await api.serializeCompetition(c), slug);
      view = await api.viewCompetition(slug);
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
    // Recomputed numbers, over the local copy the taps have been writing to.
    if (saveTimer === undefined) render();
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
