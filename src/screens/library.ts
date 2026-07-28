import { api, fmtDuration } from "../api";
import { copyText } from "../clipboard";

export async function renderLibrary(root: HTMLElement) {
  const items = await api.listWorkouts();
  root.innerHTML = `
    <div class="screen library">
      <header class="topbar">
        <h1>wltimer</h1>
        <a class="btn primary" href="#/edit">+ New</a>
      </header>
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
                         <div class="info">
                           <span class="name">${esc(w.name)}</span>
                           <span class="meta">${w.block_count} exercise${w.block_count === 1 ? "" : "s"} · ${fmtDuration(w.total_secs)}</span>
                         </div>
                         <div class="actions">
                           <a class="btn primary" href="#/run/${encodeURIComponent(w.slug)}">▶ Run</a>
                           <a class="btn" href="#/edit/${encodeURIComponent(w.slug)}">Edit</a>
                           <button class="btn copy" data-slug="${esc(w.slug)}">⧉ Copy</button>
                           <button class="btn danger delete" data-slug="${esc(w.slug)}">Delete</button>
                         </div>
                       </li>`,
                )
                .join("")
        }
      </ul>
    </div>`;

  // Export: copy the workout's markdown to the clipboard.
  root.querySelectorAll<HTMLButtonElement>("button.copy").forEach((btn) => {
    btn.addEventListener("click", async () => {
      const source = await api.getSource(btn.dataset.slug!);
      btn.textContent = (await copyText(source)) ? "✓ Copied" : "Copy failed";
      setTimeout(() => {
        btn.textContent = "⧉ Copy";
      }, 2000);
    });
  });

  // Two-tap delete: first tap arms the button, second within 3s confirms.
  root.querySelectorAll<HTMLButtonElement>("button.delete").forEach((btn) => {
    btn.addEventListener("click", async () => {
      if (btn.dataset.armed) {
        await api.deleteWorkout(btn.dataset.slug!);
        await renderLibrary(root);
      } else {
        btn.dataset.armed = "1";
        btn.textContent = "Sure?";
        setTimeout(() => {
          delete btn.dataset.armed;
          btn.textContent = "Delete";
        }, 3000);
      }
    });
  });
}

export function esc(s: string): string {
  return s.replace(/[&<>"']/g, (c) => `&#${c.charCodeAt(0)};`);
}
