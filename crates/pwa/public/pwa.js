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

// Install affordance: capture Chrome's beforeinstallprompt and surface
// an in-page chip (dismissible, remembered). iOS never fires this
// event — its install path stays the manual share sheet, out of scope
// per principal (2026-11-10).
(() => {
  const KEY = "tafy-install-dismissed";
  const standalone = () =>
    window.matchMedia("(display-mode: standalone)").matches ||
    window.navigator.standalone === true;
  let deferred = null;
  const chip = () => document.getElementById("tafy-install-chip");

  const hide = () => {
    const c = chip();
    if (c) c.remove();
  };
  const show = () => {
    if (standalone() || localStorage.getItem(KEY) || chip()) return;
    const el = document.createElement("button");
    el.id = "tafy-install-chip";
    el.className = "install-chip";
    el.innerHTML = '\u2935 \u5b89\u88c5 tAfY <span class="install-x">\u2715</span>';
    el.addEventListener("click", async (e) => {
      if (e.target.closest(".install-x")) {
        localStorage.setItem(KEY, "1");
        hide();
        return;
      }
      if (!deferred) return;
      deferred.prompt();
      const { outcome } = await deferred.userChoice;
      if (outcome === "accepted") hide();
      // declined: chip stays dismissible but may re-show next visit
    });
    document.body.appendChild(el);
  };

  window.addEventListener("beforeinstallprompt", (e) => {
    e.preventDefault();
    deferred = e;
    show();
  });
  window.addEventListener("appinstalled", hide);
})();
