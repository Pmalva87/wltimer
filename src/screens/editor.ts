import { api, fmtDuration, type ParseError } from "../api";
import { esc } from "./library";

const TEMPLATE = `# My Workout

## First Exercise
- intervals: 3
- work: 1:00
- rest: 0:30
- rest after: 2:00

Notes and cues shown on screen during this exercise.
**Markdown** works here.

## Second Exercise
- intervals: 5
- work: 0:45
`;

export async function renderEditor(root: HTMLElement, slug: string | null) {
  const source = slug ? await api.getSource(slug) : TEMPLATE;
  root.innerHTML = `
    <div class="screen editor">
      <header class="topbar">
        <a class="btn" href="#/">‹ Back</a>
        <h1>${slug ? "Edit" : "New"} workout</h1>
        <button class="btn primary" id="save">Save</button>
      </header>
      <textarea id="src" spellcheck="false" autocapitalize="off" autocomplete="off"></textarea>
      <div id="status" class="editor-status"></div>
    </div>`;

  const textarea = root.querySelector<HTMLTextAreaElement>("#src")!;
  const status = root.querySelector<HTMLElement>("#status")!;
  const saveBtn = root.querySelector<HTMLButtonElement>("#save")!;
  textarea.value = source;

  function showErrors(errors: ParseError[]) {
    status.className = "editor-status invalid";
    status.innerHTML = errors
      .map((e) => `<div>line ${e.line}: ${esc(e.message)}</div>`)
      .join("");
  }

  let debounce: ReturnType<typeof setTimeout> | undefined;
  async function validate() {
    const preview = await api.parsePreview(textarea.value);
    if (preview.status === "ok") {
      status.className = "editor-status valid";
      status.textContent = `✓ ${preview.name} — ${preview.block_count} exercise${preview.block_count === 1 ? "" : "s"}, ${fmtDuration(preview.total_secs)}`;
    } else {
      showErrors(preview.errors);
    }
  }
  textarea.addEventListener("input", () => {
    clearTimeout(debounce);
    debounce = setTimeout(() => void validate(), 300);
  });
  void validate();

  saveBtn.addEventListener("click", async () => {
    try {
      await api.saveWorkout(textarea.value, slug);
      location.hash = "#/";
    } catch (e) {
      showErrors(e as ParseError[]);
    }
  });

  return () => clearTimeout(debounce);
}
