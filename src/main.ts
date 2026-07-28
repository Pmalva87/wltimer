import "./styles.css";
import { renderLibrary } from "./screens/library";
import { renderBuilder } from "./screens/builder";
import { renderRun } from "./screens/run";

type Cleanup = (() => void) | void;

const app = document.getElementById("app")!;
let cleanup: Cleanup;

async function route() {
  if (typeof cleanup === "function") {
    cleanup();
    cleanup = undefined;
  }
  const hash = location.hash || "#/";
  const [, screen, arg] = hash.split("/");
  const slug = arg ? decodeURIComponent(arg) : null;
  if (screen === "edit") {
    cleanup = await renderBuilder(app, slug);
  } else if (screen === "run" && slug) {
    cleanup = await renderRun(app, slug);
  } else {
    cleanup = await renderLibrary(app);
  }
}

window.addEventListener("hashchange", () => void route());
void route();
