import { api, fmtDuration, type ParseError } from "../api";
import { copyText } from "../clipboard";
import { saveMarkdownFile } from "../files";
import { FORMAT_GUIDE, PLAN_FORMAT_GUIDE } from "../format";
import { tabBar } from "../tabs";

export function esc(s: string): string {
  return s.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
}

/** Two-tap confirmation for destructive buttons. */
function armDelete(btn: HTMLButtonElement, label: string, action: () => Promise<void>) {
  btn.addEventListener("click", async () => {
    if (btn.dataset.armed) {
      await action();
    } else {
      btn.dataset.armed = "1";
      btn.textContent = "Sure?";
      setTimeout(() => {
        delete btn.dataset.armed;
        btn.textContent = label;
      }, 3000);
    }
  });
}

export async function renderLibrary(root: HTMLElement) {
  const [items, plans] = await Promise.all([api.listWorkouts(), api.listPlans()]);
  root.innerHTML = `
    <div class="screen library">
      <header class="topbar">
        <h1>Workouts</h1>
        <a class="btn primary" href="#/quick">+ New</a>
      </header>
      <div class="library-scroll">
        <div class="section-head">
          <h2>Plans</h2>
          <div class="section-actions">
            <button class="btn" id="uploadplan">📂 Upload</button>
            <button class="btn" id="planformat">📄 Format .md</button>
          </div>
        </div>
        ${
          plans.length === 0
            ? `<div class="empty small">No plans — upload a multi-day .md to schedule weeks at once.</div>`
            : plans
                .map((p, i) =>
                  p.error
                    ? `<div class="workout broken">
                         <div class="info">
                           <span class="name">${esc(p.name)}</span>
                           <span class="meta error">${esc(p.error)}</span>
                         </div>
                         <div class="actions">
                           <button class="btn danger plan-delete" data-slug="${esc(p.slug)}">Delete</button>
                         </div>
                       </div>`
                    : `<div class="workout plan" data-i="${i}">
                         <div class="info">
                           <span class="name">📋 ${esc(p.name)}</span>
                           <span class="meta">${p.day_count} day${p.day_count === 1 ? "" : "s"} · ${p.first_date} → ${p.last_date}</span>
                         </div>
                         <div class="actions compact">
                           <button class="btn primary plan-sync" data-slug="${esc(p.slug)}">⟳ Sync</button>
                           <button class="btn plan-newver" data-slug="${esc(p.slug)}">⇧ Version</button>
                           <button class="btn plan-export" data-slug="${esc(p.slug)}">⇩ Export</button>
                           <button class="btn plan-copy" data-slug="${esc(p.slug)}">⧉ Copy</button>
                           <button class="btn danger plan-delete" data-slug="${esc(p.slug)}">Delete</button>
                         </div>
                       </div>`,
                )
                .join("")
        }
        <div id="libstatus" class="editor-status"></div>
        <div class="section-head">
          <h2>Single workouts</h2>
          <div class="section-actions">
            <button class="btn" id="uploadworkout">📂 Upload</button>
            <button class="btn" id="workoutformat">📄 Format .md</button>
          </div>
        </div>
        <ul class="workout-list">
          ${
            items.length === 0
              ? `<li class="empty">No workouts yet — create one!</li>`
              : items
                  .map((w, i) =>
                    w.error
                      ? `<li class="workout broken" data-i="${i}">
                           <div class="info">
                             <span class="name">${esc(w.name)}</span>
                             <span class="meta error">${esc(w.error)}</span>
                           </div>
                           <div class="actions">
                             <a class="btn" href="#/edit/${encodeURIComponent(w.slug)}">Edit</a>
                             <button class="btn danger delete" data-slug="${esc(w.slug)}">Delete</button>
                           </div>
                         </li>`
                      : `<li class="workout" data-i="${i}">
                           <a class="info tappable" href="#/view/${encodeURIComponent(w.slug)}">
                             <span class="name">${esc(w.name)}</span>
                             <span class="meta">${w.block_count} exercise${w.block_count === 1 ? "" : "s"} · ${fmtDuration(w.total_secs)}</span>
                           </a>
                           <div class="actions dense">
                             <a class="btn primary" href="#/run/${encodeURIComponent(w.slug)}">▶ Run</a>
                             <a class="btn" href="#/view/${encodeURIComponent(w.slug)}">👁 View</a>
                             <button class="btn danger delete" data-slug="${esc(w.slug)}">🗑 Delete</button>
                             <button class="btn copy" data-slug="${esc(w.slug)}">⧉ Copy</button>
                             <button class="btn dup" data-slug="${esc(w.slug)}">⊕ Duplicate</button>
                           </div>
                         </li>`,
                  )
                  .join("")
          }
        </ul>
      </div>
      <input type="file" id="planfile" accept=".md,.markdown,.txt" hidden>
      <input type="file" id="workoutfile" accept=".md,.markdown,.txt" hidden>
      ${tabBar("library")}
    </div>`;

  function showStatus(msg: string, ok: boolean) {
    const el = root.querySelector<HTMLElement>("#libstatus");
    if (!el) return;
    el.className = `editor-status ${ok ? "valid" : "invalid"}`;
    el.textContent = msg;
  }

  function showParseErrors(e: unknown) {
    const errs = e as ParseError[];
    showStatus(
      Array.isArray(errs) && errs[0]
        ? `line ${errs[0].line}: ${errs[0].message}`
        : String(e),
      false,
    );
  }

  // ---- plans ----

  const planFile = root.querySelector<HTMLInputElement>("#planfile")!;
  let newVersionSlug: string | null = null;

  root.querySelector("#uploadplan")!.addEventListener("click", () => {
    newVersionSlug = null;
    planFile.click();
  });
  root.querySelector("#planformat")!.addEventListener("click", () => {
    saveMarkdownFile("wltimer-plan-format.md", PLAN_FORMAT_GUIDE);
    showStatus("✓ plan format guide exported — give it to Claude to write a plan", true);
  });
  root.querySelectorAll<HTMLButtonElement>("button.plan-newver").forEach((btn) => {
    btn.addEventListener("click", () => {
      newVersionSlug = btn.dataset.slug!;
      planFile.click();
    });
  });
  planFile.addEventListener("change", async () => {
    const file = planFile.files?.[0];
    if (!file) return;
    try {
      const sum = await api.savePlan(await file.text(), newVersionSlug);
      showStatus(`✓ "${sum.name}" saved — ${sum.day_count} days synced to the calendar`, true);
      await renderLibrary(root);
    } catch (e) {
      showParseErrors(e);
    }
  });

  root.querySelectorAll<HTMLButtonElement>("button.plan-sync").forEach((btn) => {
    btn.addEventListener("click", async () => {
      try {
        const n = await api.syncPlan(btn.dataset.slug!);
        showStatus(`✓ synced — ${n} upcoming day${n === 1 ? "" : "s"} scheduled`, true);
      } catch (e) {
        showStatus(String(e), false);
      }
    });
  });

  root.querySelectorAll<HTMLButtonElement>("button.plan-copy").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const source = await api.getPlanSource(btn.dataset.slug!);
      showStatus((await copyText(source)) ? "✓ plan copied to clipboard" : "copy failed", true);
    });
  });

  root.querySelectorAll<HTMLButtonElement>("button.plan-export").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const source = await api.getPlanSource(btn.dataset.slug!);
      saveMarkdownFile(`${btn.dataset.slug}.md`, source);
      showStatus("✓ plan exported as .md file", true);
    });
  });

  root.querySelectorAll<HTMLButtonElement>("button.plan-delete").forEach((btn) => {
    armDelete(btn, "Delete", async () => {
      await api.deletePlan(btn.dataset.slug!);
      await renderLibrary(root);
    });
  });

  // ---- single workouts ----

  const workoutFile = root.querySelector<HTMLInputElement>("#workoutfile")!;

  root.querySelector("#uploadworkout")!.addEventListener("click", () => {
    workoutFile.click();
  });
  root.querySelector("#workoutformat")!.addEventListener("click", () => {
    saveMarkdownFile("wltimer-format.md", FORMAT_GUIDE);
    showStatus("✓ format guide exported — give it to Claude to write workouts", true);
  });
  workoutFile.addEventListener("change", async () => {
    const file = workoutFile.files?.[0];
    if (!file) return;
    const text = await file.text();
    // Plan files (dated ## headings) import as plans, not single workouts.
    const planCheck = await api.parsePlanPreview(text);
    try {
      if (planCheck.status === "ok") {
        const sum = await api.savePlan(text, null);
        showStatus(`✓ "${sum.name}" saved — ${sum.day_count} days synced to the calendar`, true);
      } else {
        const sum = await api.saveWorkout(text, null);
        showStatus(`✓ "${sum.name}" imported`, true);
      }
      await renderLibrary(root);
    } catch (e) {
      showParseErrors(e);
    }
  });

  root.querySelectorAll<HTMLButtonElement>("button.copy").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const source = await api.getSource(btn.dataset.slug!);
      btn.textContent = (await copyText(source)) ? "✓ Copied" : "Copy failed";
      setTimeout(() => {
        btn.textContent = "⧉ Copy";
      }, 2000);
    });
  });

  // Duplicate mints a new id backend-side: re-saving the source as-is would
  // match the original by id and update it instead of copying it.
  root.querySelectorAll<HTMLButtonElement>("button.dup").forEach((btn) => {
    btn.addEventListener("click", async () => {
      try {
        await api.duplicateWorkout(btn.dataset.slug!);
        await renderLibrary(root);
      } catch (e) {
        showParseErrors(e);
      }
    });
  });

  root.querySelectorAll<HTMLButtonElement>("button.delete").forEach((btn) => {
    armDelete(btn, "🗑 Delete", async () => {
      await api.deleteWorkout(btn.dataset.slug!);
      await renderLibrary(root);
    });
  });
}
