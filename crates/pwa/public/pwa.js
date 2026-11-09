// tafy-pwa PWA glue: registers the service worker. Runs as a plain
// script injected by dx (Dioxus.toml [web.resource] script) before the
// wasm boot.
if ("serviceWorker" in navigator) {
  window.addEventListener("load", () => {
    navigator.serviceWorker.register("./sw.js").catch((err) => {
      console.warn("sw registration failed", err);
    });
  });
}
