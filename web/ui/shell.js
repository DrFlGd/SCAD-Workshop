// Mounts the interface's lasting parts into the page: the top bar, sidebar, tabs, browser, status
// bar, palette, quick look, dialogs and side viewer. app.js mounts the pages it routes to (a model's,
// a parts pack's, Settings, Licenses, Library settings).
import { html, render, useEffect } from "../lib/html.js";
import { ui, initState } from "./state.js";
import { setContext, ctx } from "./context.js";
import { Sidebar } from "./sidebar.js";
import { Browser } from "./browser.js";
import { Palette } from "./palette.js";
import { QuickLook } from "./quicklook.js";
import { TopSearch, TopButtons, Tabs, StatusBar } from "./chrome.js";
import { useStore } from "../lib/store.js";
import { AddProject, MergeConflicts, makeThumbnails, dailyUpdateCheck, rescanLocal, scanOnFocus } from "./library.js";
import { MetaEditor, undoNow } from "./metaedit.js";
import { FlagDialog, SavePartDialog } from "./actions.js";
import { PinDialog, FormEditor } from "./forms.js";
import { SideView } from "./sideview.js";
import { DropDialog, DropHint, onDropped } from "./drop.js";
import { CollectDialog, refreshCollections } from "./collections.js";
import { PictureDialog, ContentDialog } from "./content.js";

/** Modal dialogs (desktop): adding a project, editing details, flagging as broken, saving as a part, merge differences, pinning, the form editor. */
function Dialogs() {
  const d = useStore(ui, (s) => s.dialog);
  // every dialog behaves the same (Review 2, UI-14): the cursor starts in its first field, Escape
  // closes it from anywhere, and a click outside does nothing (it used to throw away what you'd typed)
  useEffect(() => {
    if (!d) return;
    const box = document.querySelector("#dialog-root .dialog");
    const first = box?.querySelector("[autofocus], input:not([type=hidden]):not([disabled]), select:not([disabled]), textarea:not([disabled])");
    if (first && !box.contains(document.activeElement)) first.focus();
    else if (box && !box.contains(document.activeElement)) { box.tabIndex = -1; box.focus(); }
    const key = (e) => { if (e.key === "Escape" && !e.defaultPrevented) ui.set({ dialog: null }); };
    document.addEventListener("keydown", key);
    return () => document.removeEventListener("keydown", key);
  }, [d]);
  if (!d) return html`<${DropHint} />`;
  const body = d.type === "drop" ? html`<${DropDialog} spec=${d} key=${JSON.stringify(d.info)} />` : d.type === "add-project" ? html`<${AddProject} />` : d.type === "edit" ? html`<${MetaEditor} spec=${d} key=${JSON.stringify(d)} />`
    : d.type === "flag" ? html`<${FlagDialog} spec=${d} />` : d.type === "save-part" ? html`<${SavePartDialog} spec=${d} />` : d.type === "merge" ? html`<${MergeConflicts} result=${d.result} />`
    : d.type === "pin" ? html`<${PinDialog} spec=${d} key=${d.key} />` : d.type === "form" ? html`<${FormEditor} spec=${d} key=${d.key} />`
    : d.type === "collect" ? html`<${CollectDialog} spec=${d} />` : d.type === "picture" ? html`<${PictureDialog} spec=${d} />`
    : d.type === "content" ? html`<${ContentDialog} spec=${d} />` : null;
  return html`<div class="dialog-backdrop">${body}</div>`;
}

export function mountShell(context) {
  setContext(context);
  window.__workshop = ctx; // for tests (tests/desktop_page.py reads the index and catalog)
  initState(context.platform.store);
  const mount = (id, C) => { const el = document.getElementById(id); if (el) render(html`<${C} />`, el); };
  mount("topsearch", TopSearch);
  mount("topbuttons", TopButtons);
  mount("sidebar-root", Sidebar);
  mount("tabs-root", Tabs);
  mount("view-browse", Browser);
  mount("statusbar", StatusBar);
  mount("palette-root", Palette);
  mount("quicklook-root", QuickLook);
  mount("dialog-root", Dialogs);
  mount("sideview-root", SideView);
  // files, folders and ZIPs dropped on the window (milestone 5)
  context.platform.onDrop?.(onDropped, (dragging) => ui.set({ dragging }));
  // Ctrl+Z undoes the last edit of details (outside text fields)
  document.addEventListener("keydown", (e) => {
    if ((e.ctrlKey || e.metaKey) && !e.shiftKey && e.key.toLowerCase() === "z" && !e.target.closest?.("input, textarea, select, [contenteditable]") && ui.get().undo.length) {
      e.preventDefault();
      undoNow();
    }
  });
  refreshCollections();
  setTimeout(async () => { await rescanLocal(); makeThumbnails(); dailyUpdateCheck(); scanOnFocus(); }, 1500);
}

export { ui };
