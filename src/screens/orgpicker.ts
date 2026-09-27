import { api } from "../api";
import { esc } from "./library";

/** What an org picker needs to draw itself, shared by every picker on a
 *  screen: the organizations the app knows, and which picker (if any) is
 *  showing its "new organization" row — at most one, since it is one form. */
export interface OrgPickerState {
  organizations: string[];
  adding: string | null;
}

/**
 * An organization field: a dropdown of organizations the app knows about,
 * with a "+ New organization…" option that swaps in an add row rather than
 * navigating away — the meet you are editing is exactly where a new
 * organization is first needed, so this is where adding one belongs.
 *
 * `single` clears back to "— none —"; `multi` keeps chosen values as removable
 * chips above the dropdown and offers only the ones not already chosen.
 */
export function orgPicker(fieldId: string, mode: "single" | "multi", values: string[], st: OrgPickerState): string {
  if (st.adding === fieldId) {
    return `
      <div class="org-add-row">
        <input class="text-input" id="orgnew-${fieldId}" placeholder="Organization name" autofocus>
        <button class="btn primary" data-orgconfirm="${fieldId}">Add</button>
        <button class="btn" data-orgcancel="${fieldId}">Cancel</button>
      </div>`;
  }
  const current = values[0] ?? "";
  const avail =
    mode === "multi"
      ? st.organizations.filter((o) => !values.some((v) => v.toLowerCase() === o.toLowerCase()))
      : st.organizations;
  const chips =
    mode === "multi" && values.length
      ? `<div class="chip-list">${values
          .map((o, i) => `<span class="chip">${esc(o)}<button class="chip-remove" data-orgdel="${fieldId}:${i}">✕</button></span>`)
          .join("")}</div>`
      : "";
  const placeholder = mode === "single" ? "— none —" : "+ add organization…";
  return `
    ${chips}
    <select class="text-input" data-orgpick="${fieldId}">
      <option value="" ${current ? "" : "selected"}>${placeholder}</option>
      ${avail.map((o) => `<option value="${esc(o)}" ${o === current ? "selected" : ""}>${esc(o)}</option>`).join("")}
      <option value="__new__">+ New organization…</option>
    </select>`;
}

/**
 * One picker's worth of wiring: picking an existing organization (or clearing
 * a single-value field back to none), picking "+ New" to reveal the add row,
 * confirming or cancelling that row, and removing a chip from a multi-value
 * field. `get`/`set` reach into whichever field this picker owns; `set` is
 * where the caller saves, `render` only redraws.
 */
export function bindOrgPicker(
  root: HTMLElement,
  fieldId: string,
  mode: "single" | "multi",
  st: OrgPickerState,
  get: () => string[],
  set: (values: string[]) => void,
  render: () => void,
) {
  root.querySelector<HTMLSelectElement>(`[data-orgpick="${fieldId}"]`)?.addEventListener("change", (ev) => {
    const value = (ev.currentTarget as HTMLSelectElement).value;
    if (value === "__new__") {
      st.adding = fieldId;
      render();
      root.querySelector<HTMLInputElement>(`#orgnew-${fieldId}`)?.focus();
      return;
    }
    if (mode === "single") {
      set(value ? [value] : []);
    } else if (value && !get().some((v) => v.toLowerCase() === value.toLowerCase())) {
      set([...get(), value]);
    }
    render();
  });
  root.querySelector<HTMLButtonElement>(`[data-orgconfirm="${fieldId}"]`)?.addEventListener("click", () => {
    void (async () => {
      const input = root.querySelector<HTMLInputElement>(`#orgnew-${fieldId}`)!;
      const name = input.value.trim();
      if (name === "") return;
      st.organizations = await api.addOrganization(name);
      st.adding = null;
      set(mode === "single" ? [name] : [...get(), name]);
      render();
    })();
  });
  root.querySelector<HTMLButtonElement>(`[data-orgcancel="${fieldId}"]`)?.addEventListener("click", () => {
    st.adding = null;
    render();
  });
  root.querySelectorAll<HTMLButtonElement>(`[data-orgdel^="${fieldId}:"]`).forEach((btn) => {
    btn.addEventListener("click", () => {
      const i = Number(btn.dataset.orgdel!.split(":")[1]);
      set(get().filter((_, idx) => idx !== i));
      render();
    });
  });
}
