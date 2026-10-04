import { api, todayStr, type CompSummary, type ParseError } from "../api";
import { compRow } from "./comp";
import { armDelete, esc } from "./library";
import { tabBar } from "../tabs";
import { saveMarkdownFile } from "../files";
import { COMP_FORMAT_GUIDE } from "../format";

/**
 * The Competitions tab: meets you have lifted at or are chasing, plus the
 * Organizations list that feeds the picker on the meet editor. Organizations
 * live here rather than on a screen of their own — this is the one place
 * anything reads them, so there is nothing to gain from a second menu.
 */
export async function renderCompetitions(root: HTMLElement) {
  let comps: CompSummary[] = await api.listCompetitions();
  let organizations: string[] = await api.listOrganizations();
  // A name a meet uses is offered from the meet itself, so it has no ✕ here:
  // deleting it would only bring it straight back.
  const inUse = new Set((await api.organizationsInUse()).map((n) => n.toLowerCase()));

  /**
   * The list arrives already ordered around today; this only labels the split.
   * An undated meet is not history, so it stays with what is to come.
   */
  function meetSections(): string {
    const today = todayStr();
    const done = comps.filter((c) => c.date !== null && c.date < today);
    const upcoming = comps.filter((c) => !done.includes(c));
    const section = (title: string, list: CompSummary[]) =>
      list.length === 0
        ? ""
        : `<div class="plan-section">${title} · ${list.length}</div>${list.map(compRow).join("")}`;
    return section("Upcoming", upcoming) + section("Done", done);
  }

  function render() {
    root.innerHTML = `
      <div class="screen library">
        <header class="topbar">
          <h1>Competitions</h1>
          <a class="btn primary" href="#/compedit">+ New meet</a>
        </header>
        <div class="library-scroll">
          <div class="section-head">
            <h2>Organizations</h2>
          </div>
          <div class="empty small">
            Federations, clubs and promoters to pick from when you set whose
            meet it is or who sanctions it. Every one your meets name is here
            already; the ones without ✕ are in use by a meet.
          </div>
          ${
            organizations.length
              ? `<div class="chip-list">${organizations
                  .map(
                    (o) =>
                      inUse.has(o.toLowerCase())
                        ? `<span class="chip" title="Used by a meet">${esc(o)}</span>`
                        : `<span class="chip">${esc(o)}<button class="chip-remove" data-orgdel="${esc(o)}" title="Delete">✕</button></span>`,
                  )
                  .join("")}</div>`
              : ""
          }
          <div class="org-add-row">
            <input class="text-input" id="neworg" placeholder="Organization name">
            <button class="btn" id="addorg">+ Add</button>
          </div>

          <div class="section-head">
            <h2>Meets</h2>
            <div class="section-actions">
              <button class="btn" id="compupload">📂 Upload</button>
              <button class="btn" id="compformat">📄 Format .md</button>
            </div>
          </div>
          <div id="compstatus" class="editor-status"></div>
          ${
            comps.length === 0
              ? `<div class="empty small">No competitions — add a meet to record its attempts, or one you are chasing to record what it takes to get in.</div>`
              : meetSections()
          }
        </div>
        ${tabBar("competitions")}
        <input type="file" id="compfile" accept=".md,.markdown,.txt" hidden>
      </div>`;
    bind();
  }

  function showStatus(msg: string, ok: boolean) {
    const el = root.querySelector<HTMLElement>("#compstatus");
    if (!el) return;
    el.className = `editor-status ${ok ? "valid" : "invalid"}`;
    el.textContent = msg;
  }

  function bind() {
    root.querySelector("#compformat")?.addEventListener("click", () => {
      saveMarkdownFile("wltimer-competition-format.md", COMP_FORMAT_GUIDE);
      showStatus("✓ competition format guide exported — give it to Claude to write a meet", true);
    });
    const file = root.querySelector<HTMLInputElement>("#compfile")!;
    root.querySelector("#compupload")?.addEventListener("click", () => file.click());
    file.addEventListener("change", async () => {
      const f = file.files?.[0];
      if (!f) return;
      const text = await f.text();
      // Only meets land here. A plan or workout has its own upload under
      // Workouts, and guessing would file it somewhere you are not looking.
      if (!(await api.isCompetition(text))) {
        showStatus("not a meet — it needs '- kind: competition' under its title (plans and workouts upload under Workouts)", false);
        return;
      }
      try {
        const sum = await api.importCompetition(text);
        comps = await api.listCompetitions();
        render();
        showStatus(`✓ "${sum.name}" imported`, true);
      } catch (e) {
        const errs = e as ParseError[];
        showStatus(Array.isArray(errs) && errs[0] ? `line ${errs[0].line}: ${errs[0].message}` : String(e), false);
      }
    });
    const addOrg = async () => {
      const input = root.querySelector<HTMLInputElement>("#neworg")!;
      const name = input.value.trim();
      if (name === "") return;
      organizations = await api.addOrganization(name);
      render();
    };
    root.querySelector("#addorg")?.addEventListener("click", () => void addOrg());
    root.querySelector<HTMLInputElement>("#neworg")?.addEventListener("keydown", (ev) => {
      if ((ev as KeyboardEvent).key === "Enter") void addOrg();
    });
    root.querySelectorAll<HTMLButtonElement>("[data-orgdel]").forEach((btn) => {
      btn.addEventListener("click", async () => {
        organizations = await api.deleteOrganization(btn.dataset.orgdel!);
        render();
      });
    });
    root.querySelectorAll<HTMLButtonElement>("button.compdelete").forEach((btn) => {
      const label = btn.textContent ?? "🗑";
      armDelete(btn, label, async () => {
        await api.deleteCompetition(btn.dataset.slug!);
        await renderCompetitions(root);
      });
    });
  }

  render();
}
