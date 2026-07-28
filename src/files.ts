declare global {
  interface Window {
    /** Android bridge installed by MainActivity (system "save file" dialog). */
    WltimerFiles?: { save(filename: string, content: string): void };
  }
}

/** Save text as a .md file: native picker on Android, download elsewhere. */
export function saveMarkdownFile(filename: string, content: string) {
  if (window.WltimerFiles) {
    window.WltimerFiles.save(filename, content);
    return;
  }
  const url = URL.createObjectURL(new Blob([content], { type: "text/markdown" }));
  const a = document.createElement("a");
  a.href = url;
  a.download = filename;
  document.body.appendChild(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(url), 5000);
}
