import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { api, fmtDuration, type Cue, type PhaseKind, type RunPlan, type Snapshot } from "../api";
import { initAudio, sounds, vibrate } from "../audio";
import { esc } from "./library";

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

export async function renderRun(root: HTMLElement, slug: string) {
  root.innerHTML = `
    <div class="screen run" id="runscreen">
      <header class="topbar run-top">
        <button id="exit" class="btn">✕</button>
        <span id="wname" class="run-title"></span>
        <span id="phasecount" class="run-count"></span>
      </header>
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
  let lastDescIdx = -1;

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

    // Description: current block while working/resting, upcoming block otherwise.
    const descIdx =
      kind === "work" || kind === "rest" ? s.block_idx : (s.next_block_idx ?? s.block_idx);
    const descBlock = plan.blocks[descIdx];
    if (descIdx !== lastDescIdx && descBlock) {
      lastDescIdx = descIdx;
      el("desc").innerHTML =
        (kind === "prepare" || kind === "block_rest"
          ? `<div class="next-block">Up next: ${esc(descBlock.name)}</div>`
          : "") + descBlock.description_html;
    }
    if (s.next_kind) {
      const nextBlock = plan.blocks[s.next_block_idx ?? 0];
      const nextName =
        s.next_kind === "work" ? (nextBlock?.name ?? "work") : LABELS[s.next_kind].toLowerCase();
      el("nextup").textContent = `next: ${nextName}`;
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

  el("start").addEventListener("click", async () => {
    initAudio();
    try {
      plan = await api.startWorkout(slug);
    } catch (e) {
      el("ovmeta").textContent = String(e);
      return;
    }
    el("startoverlay").style.display = "none";
    el("wname").textContent = plan.workout_name;
  });

  el("ovname").textContent = slug;
  // Show real name/duration on the start overlay without starting the timer.
  void api.listWorkouts().then((items) => {
    const w = items.find((i) => i.slug === slug);
    if (w) {
      el("ovname").textContent = w.name;
      el("ovmeta").textContent = `${w.block_count} exercise${w.block_count === 1 ? "" : "s"} · ${fmtDuration(w.total_secs)}`;
    }
  });

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
    void api.stop();
    for (const p of unlisteners) {
      void p.then((un) => un());
    }
  };
}
