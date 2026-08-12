import "./styles.css";
import { renderLibrary } from "./screens/library";
import { renderBuilder } from "./screens/builder";
import { renderRun } from "./screens/run";
import { renderCalendar } from "./screens/calendar";
import { renderPlanFrom } from "./screens/planfrom";
import { renderPlan } from "./screens/plan";
import { renderView } from "./screens/view";
import { lastTab, rememberTab } from "./tabs";

type Cleanup = (() => void) | void;

const app = document.getElementById("app")!;
let cleanup: Cleanup;

async function route() {
  if (typeof cleanup === "function") {
    cleanup();
    cleanup = undefined;
  }
  const hash = location.hash || "#/";
  const [, screen, arg, ...rest] = hash.split("/");
  const slug = arg ? decodeURIComponent(arg) : null;
  // Trailing segments say where a screen was opened from, so its Back button
  // can return there — `#/view/@2026-08-01:0/plan/531-cycle-1` reads as "this
  // entry, reached from that plan". Survives a reload, unlike a remembered
  // history entry.
  const from = rest.length ? `#/${rest.map(decodeURIComponent).join("/")}` : null;
  if (screen === "quick" || (screen === "edit" && !slug)) {
    rememberTab("#/quick");
    cleanup = await renderBuilder(app, null, true);
  } else if (screen === "edit") {
    cleanup = await renderBuilder(app, slug, false);
  } else if (screen === "library") {
    rememberTab("#/library");
    cleanup = await renderLibrary(app);
  } else if (screen === "plan" && slug) {
    cleanup = await renderPlan(app, slug);
  } else if (screen === "planfrom") {
    cleanup = await renderPlanFrom(app);
  } else if (screen === "calendar") {
    rememberTab("#/calendar");
    cleanup = await renderCalendar(app, slug);
  } else if (screen === "view" && slug) {
    cleanup = await renderView(app, slug, from);
  } else if (screen === "run" && slug) {
    cleanup = await renderRun(app, slug);
  } else {
    // "#/" and anything unknown: reopen the last-used tab (default: quick).
    location.hash = lastTab();
  }
}

window.addEventListener("hashchange", () => void route());
void route();
