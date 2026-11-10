// tafy-pwa service worker v2: offline-capable shell with a
// self-refreshing entry. Policy: hashed assets (immutable by name)
// = cache-first; everything else (entry HTML, manifest, icons, root
// JS) = network-first with cache fallback. No manual version bump on
// deploy: the entry revalidates whenever the phone is online.
//
// v2 (PT-R fix, principal ruling 2026-11-10): GitHub Pages is a file
// server — SPA deep routes (/tafy/thread/...) have no file, so a
// navigation gets a 404. A 404 is a SUCCESSFUL fetch response, so v1
// passed it through and the phone showed GitHub's 404 page. v2: a
// navigation that does not reach 200 (server 404, or offline) is
// answered from the cached shell — the app always comes up and its
// own router owns the "unknown page" UX. Cold deep links (no SW yet)
// are covered deploy-side by 404.html = index.html copy (gh-pages
// root convention), NOT here.
const CACHE = "tafy-shell";

self.addEventListener("install", () => {
  self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    (async () => {
      const keys = await caches.keys();
      await Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k)));
      await self.clients.claim();
    })()
  );
});

// Hashed asset names produced by the build pipeline are immutable.
const IMMUTABLE = (url) =>
  /\/assets\/[^/]+-[A-Za-z0-9_-]+\.(js|wasm|css)$/.test(url.pathname);

self.addEventListener("fetch", (event) => {
  const req = event.request;
  if (req.method !== "GET") return;
  let url;
  try {
    url = new URL(req.url);
  } catch {
    return;
  }
  if (url.origin !== self.location.origin) return;

  if (IMMUTABLE(url)) {
    event.respondWith(
      (async () => {
        const cached = await caches.match(req);
        if (cached) return cached;
        const res = await fetch(req);
        if (res.ok) {
          (await caches.open(CACHE)).put(req, res.clone());
        }
        return res;
      })()
    );
  } else {
    // the cached app shell (entry HTML), for navigation fallbacks
    const shell = async () =>
      (await caches.match("./", { ignoreSearch: true })) ||
      (await caches.match("./index.html"));
    event.respondWith(
      (async () => {
        // navigations never fall back to the request's own cache key:
        // a deep route is never a cached entry, the shell is the answer
        const navigate = req.mode === "navigate";
        try {
          const res = await fetch(req);
          if (res.ok) {
            (await caches.open(CACHE)).put(req, res.clone());
            return res;
          }
          // non-200: pass through for non-navigations; a navigation gets
          // the shell (server 404 on a deep route = the known SPA case)
          if (navigate) return (await shell()) || res;
          return res;
        } catch {
          // offline: cached copy if we have it, else shell for navigations
          const cached = navigate ? await shell() : await caches.match(req);
          if (cached) return cached;
          throw new Error("offline and uncached");
        }
      })()
    );
  }
});
