import "./app.css";
import App from "./App.svelte";
import { installWebviewGuards } from "./lib/guards";

// Harden the WebView (disable context menu, browser hotkeys, text selection)
// before the app mounts so no default behaviour flashes on first paint.
installWebviewGuards();

const app = new App({
  target: document.getElementById("app")!,
});

export default app;
