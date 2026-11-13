// The page's platform: the render engine and storage, from the desktop app (Tauri). The website
// was retired in 0.4.4 and its half of this file removed (Review 7, item 10): platform-desktop.js
// is the one there is.
//
// createPlatform() -> { kind: "desktop", store, makeEngine(catalog), fetch(path), modelFile(sha), openUrl(url), save(blob, name), api(cmd, args), … }
//   fetch: library and app files (data/, parts/) by relative path
//   modelFile: a model file's text by its content hash (the side viewer's Code tab)
//   save:  a save dialog, then the file written
//
// engine (from makeEngine)
//   render(model, values, onEvent) -> { promise, cancel }
//     model:   built model JSON: key, entry, files, fixed, parameters
//     values:  { name: value } for the model's settings
//     onEvent: { type: "stage", stage } | { type: "log", line }
//     promise: resolves { blob (STL), ms, logs, cached }; rejects Error with .logs or .cancelled
//   label: short text for the header ("OpenSCAD 2026.10.02, native")
//   concurrency: how many renders can run at once (batches use it)
//
// store
//   prefs.get(key, fallback) / prefs.set(key, value)   small UI preferences, synchronous
//   settings.list(modelKey) -> [{ id, model, name, values, created, updated }]
//   settings.get(id) / settings.save(record) / settings.remove(id)
//   Saved values hold only the settings that differ from the model's defaults.
export async function createPlatform() {
  if (!globalThis.__TAURI_INTERNALS__) {
    document.body.textContent = "SCAD Workshop runs in its desktop app.";
    throw new Error("not in the desktop app");
  }
  return (await import("./platform-desktop.js")).createPlatform();
}
