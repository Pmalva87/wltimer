import { copyFileSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";

const root = path.dirname(path.dirname(fileURLToPath(import.meta.url)));
const { version } = JSON.parse(
  readFileSync(path.join(root, "package.json"), "utf8"),
);

const apkDir = path.join(
  root,
  "src-tauri/gen/android/app/build/outputs/apk/universal/release",
);
const src = path.join(apkDir, "app-universal-release.apk");
const dest = path.join(apkDir, `wltimer-${version}.apk`);

copyFileSync(src, dest);
console.log(`Copied to ${dest}`);
