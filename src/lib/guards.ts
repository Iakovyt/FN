// WebView hardening: strip the default browser-ish behaviour that Tauri's
// WebView2 exposes (context menu, refresh/print/find hotkeys, text selection).
// This is the JS half; the Rust side (webview config) blocks the same things
// so a script that loses focus can't re-enable them.

function blockContextMenu(e: MouseEvent) {
  e.preventDefault();
  return false;
}

function isEditable(target: EventTarget | null): boolean {
  const el = target as HTMLElement | null;
  if (!el) return false;
  const tag = el.tagName;
  return tag === "INPUT" || tag === "TEXTAREA" || el.isContentEditable;
}

function blockHotkeys(e: KeyboardEvent) {
  const key = e.key;
  const ctrl = e.ctrlKey || e.metaKey;

  // F5 / Ctrl+R — reload
  if (key === "F5" || (ctrl && (key === "r" || key === "R"))) {
    e.preventDefault();
    return;
  }
  // Ctrl+P — print
  if (ctrl && (key === "p" || key === "P")) {
    e.preventDefault();
    return;
  }
  // Ctrl+F — in-page find (allow inside editable fields for normal typing of "f")
  if (ctrl && (key === "f" || key === "F") && !isEditable(e.target)) {
    e.preventDefault();
    return;
  }
  // F11 — fullscreen
  if (key === "F11") {
    e.preventDefault();
    return;
  }
  // F12 / Ctrl+Shift+I — devtools
  if (key === "F12" || (ctrl && e.shiftKey && (key === "i" || key === "I"))) {
    e.preventDefault();
    return;
  }
  // Ctrl+U — view source, Ctrl+G — find next
  if (ctrl && (key === "u" || key === "U" || key === "g" || key === "G")) {
    e.preventDefault();
    return;
  }
}

function blockDrag(e: DragEvent) {
  // Prevent dragging images / selections out of the window.
  if (!isEditable(e.target)) e.preventDefault();
}

export function installWebviewGuards() {
  document.addEventListener("contextmenu", blockContextMenu, { capture: true });
  document.addEventListener("keydown", blockHotkeys, { capture: true });
  document.addEventListener("dragstart", blockDrag, { capture: true });
  // Belt-and-suspenders: also set the inline handler the classic way.
  document.oncontextmenu = () => false;
}
