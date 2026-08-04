import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import {
  api,
  fmtDuration,
  partsWithoutRestAfter,
  workoutTotalSecs,
  type Cue,
  type PhaseKind,
  type RunPlan,
  type Snapshot,
} from "../api";
import { initAudio, sounds, vibrate } from "../audio";
import { esc } from "./library";
import { loadRunDraft } from "./builder";

const COLORS: Record<PhaseKind, string> = {
  prepare: "#334155",
  work: "#16a34a",
  rest: "#2563eb",
  block_rest: "#475569",
};

const LABELS: Record<PhaseKind, string> = {
  prepare: "GET READY",
  work: "WORK",
  rest: "REST",
  block_rest: "BLOCK REST",
};

/**
 * `target` is a saved workout slug, `draft` (unsaved workout from the
 * builder), or `@<date>:<index>` (calendar entry).
 */
export async function renderRun(root: HTMLElement, target: string) {
  const draft = target === "draft" ? loadRunDraft() : null;
  const dayMatch = target.match(/^@(\d{4}-\d{2}-\d{2}):(\d+)$/);
  // A run left unfinished earlier today waits as a session; it is offered back
  // on its own start screen and only pointed at from any other one, so it
  // cannot be replaced without noticing. Sessions do not outlive their day.
  const session = await api.getSession();
  const resumable = session?.target === target ? session : null;
  root.innerHTML = `
    <div class="screen run" id="runscreen">
      <header class="topbar run-top">
        <button id="exit" class="btn">✕</button>
        <span id="wname" class="run-title"></span>
        <span id="phasecount" class="run-count"></span>
      </header>
      <div id="totalleft" class="total-left"></div>
      <main class="run-main">
        <div id="blockname" class="block-name"></div>
        <div id="phaselabel" class="phase-label"></div>
        <div id="time" class="time">·</div>
        <div id="intervalinfo" class="interval-info"></div>
        <div class="progress"><div id="progressfill" class="progress-fill"></div></div>
        <div id="nextup" class="next-up"></div>
      </main>
      <section id="desc" class="desc"></section>
      <footer class="run-controls">
        <button id="pause" class="btn big">Pause</button>
        <button id="skip" class="btn big">Skip ›</button>
      </footer>
      <div id="startoverlay" class="overlay">
        <h2 id="ovname"></h2>
        <div id="ovmeta" class="ov-meta"></div>
        <div id="ovwarn" class="ov-warn" hidden></div>
        <div id="ovresume" class="ov-resume"></div>
        <button id="start" class="btn start">START</button>
        <a class="btn" href="#/">‹ Back</a>
      </div>
      <div id="flash" class="flash-overlay"></div>
    </div>`;

  const el = (id: string) => root.querySelector<HTMLElement>(`#${id}`)!;
  const screen = el("runscreen");
  const flash = el("flash");
  const pauseBtn = el("pause") as HTMLButtonElement;

  let plan: RunPlan | null = null;
  let paused = false;
  let finished = false;
  let lastPanelIdx = -1;

  const upNextHead = (name: string) => `<div class="next-block">Up next: ${esc(name)}</div>`;

  function applyTick(s: Snapshot) {
    if (!plan || s.state === "idle") return;
    if (s.state === "finished") {
      showFinished();
      return;
    }
    const kind = s.phase_kind!;
    const block = plan.blocks[s.block_idx];
    const workColor = block?.color ?? COLORS.work;
    screen.style.background = kind === "work" ? workColor : COLORS[kind];
    el("phaselabel").textContent = LABELS[kind];
    el("time").textContent = fmtDuration(Math.ceil(s.remaining_ms / 1000));
    el("phasecount").textContent = `${s.phase_idx + 1}/${s.total_phases}`;
    el("blockname").textContent =
      kind === "work" || kind === "rest" ? block.name : "";
    el("intervalinfo").textContent =
      kind === "work" || kind === "rest"
        ? `interval ${s.interval_idx} / ${block.intervals}`
        : "";
    const frac = s.phase_secs > 0 ? 1 - s.remaining_ms / (s.phase_secs * 1000) : 0;
    el("progressfill").style.width = `${Math.min(100, Math.max(0, frac * 100))}%`;
    el("totalleft").innerHTML =
      `<span class="total-left-label">workout left</span> ${fmtDuration(Math.ceil(s.total_remaining_ms / 1000))}`;

    // The part that follows this one, shown through every rest and through the
    // last interval, which is when what comes after matters: the plates to
    // change are decided before the bar is racked, and a part with no rest
    // after it gives no later chance to read them at all. Ordinary work
    // intervals stay bare — nothing but the current cues belongs on screen
    // mid-set.
    const ahead =
      kind === "rest" || (kind === "work" && s.interval_idx === block.intervals)
        ? plan.blocks[s.block_idx + 1]
        : undefined;

    // Info panel: the cues for what is being done, under the cues for what is
    // coming. Rebuilt only when the phase changes — it is static within one,
    // and ticks land five times a second.
    if (s.phase_idx !== lastPanelIdx) {
      lastPanelIdx = s.phase_idx;
      const descIdx =
        kind === "work" || kind === "rest" ? s.block_idx : (s.next_block_idx ?? s.block_idx);
      const descBlock = plan.blocks[descIdx];
      // Between parts the heading stands on its own: which part is coming is
      // worth knowing even when it carries no cues. Mid-part, a block without
      // notes (e.g. a quick timer) still contributes nothing.
      const between = kind === "prepare" || kind === "block_rest";
      const notes = between
        ? descBlock
          ? upNextHead(descBlock.name) + descBlock.description_html
          : ""
        : (descBlock?.description_html ?? "");
      const aheadCue = ahead
        ? `<div class="up-next">${upNextHead(ahead.name)}${ahead.description_html}</div>`
        : "";
      el("desc").innerHTML = aheadCue + notes;
    }
    if (s.next_kind) {
      const nextBlock = plan.blocks[s.next_block_idx ?? 0];
      const nextName =
        s.next_kind === "work" ? (nextBlock?.name ?? "work") : LABELS[s.next_kind].toLowerCase();
      // A rest names what waits on the other side of it, so the last interval
      // says "then Bench Press" rather than just "block rest".
      el("nextup").textContent =
        ahead && s.next_kind !== "work"
          ? `next: ${nextName}, then ${ahead.name}`
          : `next: ${nextName}`;
    } else {
      el("nextup").textContent = "last one — finish strong!";
    }
  }

  function doFlash() {
    flash.classList.remove("on");
    // Force a reflow so re-adding the class restarts the animation.
    void flash.offsetWidth;
    flash.classList.add("on");
  }

  function onCue(cue: Cue) {
    switch (cue.kind) {
      case "pre_alert":
        sounds.prealert();
        vibrate(80);
        doFlash();
        break;
      case "phase_start":
        if (cue.phase === "work") {
          sounds.workStart();
          vibrate([150, 80, 150]);
        } else {
          sounds.restStart();
          vibrate(250);
        }
        break;
      case "finished":
        sounds.finished();
        vibrate([200, 100, 200, 100, 400]);
        break;
    }
  }

  function showFinished() {
    if (finished) return;
    finished = true;
    screen.style.background = "#0f172a";
    el("startoverlay").innerHTML = `
      <h2>Done! 💪</h2>
      <div class="ov-meta">${esc(plan?.workout_name ?? "")} — ${fmtDuration(plan?.total_secs ?? 0)}</div>
      <a class="btn start" href="#/">Finish</a>`;
    el("startoverlay").style.display = "flex";
  }

  const unlisteners: Promise<UnlistenFn>[] = [
    listen<Snapshot>("timer:tick", (e) => applyTick(e.payload)),
    listen<Cue>("timer:cue", (e) => onCue(e.payload)),
  ];

  /** Put a run on the clock — a fresh one or a resumed one — and get out of
   *  the overlay's way. */
  async function begin(load: () => Promise<RunPlan>) {
    initAudio();
    try {
      plan = await load();
    } catch (e) {
      el("ovmeta").textContent = String(e);
      return;
    }
    paused = false;
    pauseBtn.textContent = "Pause";
    screen.classList.remove("paused");
    el("startoverlay").style.display = "none";
    el("wname").textContent = plan.workout_name;
  }

  el("start").addEventListener("click", () => {
    void begin(() =>
      dayMatch
        ? api.startDayEntry(dayMatch[1], Number(dayMatch[2]))
        : draft
          ? api.startCustom(draft)
          : api.startWorkout(target),
    );
  });

  /** Say so when parts chain together, and start anyway — it is a heads-up
   *  about how the workout will run, not a reason to stop it. */
  function showNoRestWarning(count: number) {
    if (count === 0) return;
    const warn = el("ovwarn");
    warn.textContent =
      count === 1
        ? "⚠ one part runs straight into the next — no rest in between"
        : `⚠ ${count} parts run straight into the next — no rest in between`;
    warn.hidden = false;
  }

  // Show name/duration on the start overlay without starting the timer.
  if (dayMatch) {
    el("ovname").textContent = dayMatch[1];
    void api.getDay(dayMatch[1]).then(async (entries) => {
      const entry = entries[Number(dayMatch[2])];
      if (!entry) {
        // The workout travels with the session, so a paused run outlives the
        // calendar entry it came from and can still be finished.
        el("ovname").textContent = resumable?.workout_name ?? "Nothing to run";
        el("ovmeta").textContent = "this calendar entry no longer exists";
        return;
      }
      el("ovname").textContent = entry.name;
      const parsed = await api.parseFull(entry.markdown);
      if (parsed.status === "ok") {
        el("ovmeta").textContent = `total ${fmtDuration(workoutTotalSecs(parsed.workout))}`;
        showNoRestWarning(partsWithoutRestAfter(parsed.workout).length);
      }
    });
  } else if (target === "draft") {
    if (draft) {
      el("ovname").textContent = draft.name;
      el("ovmeta").textContent =
        draft.blocks
          .map((b) => `${b.intervals} × ${fmtDuration(b.work_secs)}`)
          .join("  ·  ") + `  ·  total ${fmtDuration(workoutTotalSecs(draft))}`;
      showNoRestWarning(partsWithoutRestAfter(draft).length);
    } else if (resumable) {
      el("ovname").textContent = resumable.workout_name;
      el("ovmeta").textContent = "the quick timer it was built from is gone";
    } else {
      el("ovname").textContent = "Nothing to run";
      el("ovmeta").textContent = "build a workout first";
    }
  } else {
    el("ovname").textContent = target;
    void api.listWorkouts().then((items) => {
      const w = items.find((i) => i.slug === target);
      if (w) {
        el("ovname").textContent = w.name;
        el("ovmeta").textContent = `${w.block_count} exercise${w.block_count === 1 ? "" : "s"} · ${fmtDuration(w.total_secs)}`;
        showNoRestWarning(w.parts_without_rest);
      }
    });
  }

  if (resumable) {
    el("ovresume").innerHTML = `
      <div class="ov-note">paused at ${resumable.phase_idx + 1}/${resumable.total_phases}
        · ${fmtDuration(resumable.remaining_secs)} left</div>
      <button id="resume" class="btn start">RESUME</button>`;
    // Starting fresh is still one tap away, just no longer the loud one.
    el("start").classList.remove("start");
    el("start").textContent = "Start over";
    el("resume").addEventListener("click", () => void begin(() => api.resumeSession()));
  } else if (session) {
    el("ovresume").innerHTML = `
      <a class="ov-note" href="#/run/${encodeURIComponent(session.target)}">
        ${esc(session.workout_name)} is paused — ${fmtDuration(session.remaining_secs)} left</a>`;
  }

  pauseBtn.addEventListener("click", async () => {
    if (finished) return;
    if (paused) {
      await api.resume();
    } else {
      await api.pause();
    }
    paused = !paused;
    pauseBtn.textContent = paused ? "Resume" : "Pause";
    screen.classList.toggle("paused", paused);
  });

  el("skip").addEventListener("click", () => {
    if (!finished) void api.skip();
  });

  el("exit").addEventListener("click", () => {
    location.hash = "#/";
  });

  return () => {
    // Leaving mid-workout keeps the run as a session rather than binning it.
    void api.suspend();
    for (const p of unlisteners) {
      void p.then((un) => un());
    }
  };
}
