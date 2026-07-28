import "./styles.css";
import { renderLibrary } from "./screens/library";
import { renderEditor } from "./screens/editor";
import { renderRun } from "./screens/run";
import { renderQuick } from "./screens/quick";

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
    cleanup = await renderEditor(app, slug);
  } else if (screen === "quick") {
    cleanup = await renderQuick(app);
  } else if (screen === "run" && slug) {
    cleanup = await renderRun(app, slug);
  } else {
    cleanup = await renderLibrary(app);
  }
}

window.addEventListener("hashchange", () => void route());
void route();
