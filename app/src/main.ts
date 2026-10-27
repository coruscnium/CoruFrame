import { createApp } from "vue";
import App from "./App.vue";
import { router } from "./router";
import { applyTextScale } from "./stores/ui";
import "./style.css";

function preservesBrowserMenu(target: EventTarget | null) {
  if (!(target instanceof Element)) return false;
  return Boolean(
    target.closest("input, textarea, select, [contenteditable], .selectable, a[href]"),
  );
}

window.addEventListener("contextmenu", (event) => {
  if (preservesBrowserMenu(event.target)) return;
  if (window.getSelection()?.toString()) return;
  event.preventDefault();
});

// A dropped file would otherwise navigate the webview away from the app.
function allowsDrop(target: EventTarget | null) {
  return (
    target instanceof Element &&
    Boolean(target.closest("input, textarea, [contenteditable], .selectable"))
  );
}

window.addEventListener("dragover", (event) => {
  if (!allowsDrop(event.target)) event.preventDefault();
});

window.addEventListener("drop", (event) => {
  if (!allowsDrop(event.target)) event.preventDefault();
});

applyTextScale();
createApp(App).use(router).mount("#app");
