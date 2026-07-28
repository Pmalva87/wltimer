export type Tab = "quick" | "library" | "history";

const TAB_KEY = "wltimer.tab";
const DEFAULT_TAB = "#/quick";

/** Remember the active top-level tab so the app reopens on it. */
export function rememberTab(hash: string) {
  localStorage.setItem(TAB_KEY, hash);
}

export function lastTab(): string {
  const stored = localStorage.getItem(TAB_KEY);
  return stored === "#/quick" || stored === "#/library" || stored === "#/calendar"
    ? stored
    : DEFAULT_TAB;
}

export function tabBar(active: Tab): string {
  const tab = (id: Tab, hash: string, icon: string, label: string) =>
    `<a class="tab ${active === id ? "active" : ""}" href="${hash}">${icon}<span>${label}</span></a>`;
  return `
    <nav class="tabbar">
      ${tab("quick", "#/quick", "⚡", "Quick")}
      ${tab("library", "#/library", "📚", "Workouts")}
      ${tab("history", "#/calendar", "📅", "History")}
    </nav>`;
}
