import { api, type CompSummary } from "../api";
import { compRow } from "./comp";
import { esc } from "./library";
import { tabBar } from "../tabs";

/**
 * The Competitions tab: meets you have lifted at or are chasing, plus the
 * Organizations list that feeds the picker on the meet editor. Organizations
 * live here rather than on a screen of their own — this is the one place
 * anything reads them, so there is nothing to gain from a second menu.
 */
export async function renderCompetitions(root: HTMLElement) {
  const comps: CompSummary[] = await api.listCompetitions();
  let organizations: string[] = await api.listOrganizations();

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
            Federations, clubs and promoters to pick from when you set who
            organizes or sanctions a meet.
          </div>
          ${
            organizations.length
              ? `<div class="chip-list">${organizations
                  .map(
                    (o) =>
                      `<span class="chip">${esc(o)}<button class="chip-remove" data-orgdel="${esc(o)}" title="Delete">✕</button></span>`,
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
          </div>
          ${
            comps.length === 0
              ? `<div class="empty small">No competitions — add a meet to record its attempts, or one you are chasing to record what it takes to get in.</div>`
              : comps.map(compRow).join("")
          }
        </div>
        ${tabBar("competitions")}
      </div>`;
    bind();
  }

  function bind() {
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
  }

  render();
}
