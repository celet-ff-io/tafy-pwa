// tafy-pwa service worker v1: offline-capable shell with a
// self-refreshing entry. Policy: hashed assets (immutable by name)
// = cache-first; everything else (entry HTML, manifest, icons, root
// JS) = network-first with cache fallback. No manual version bump on
// deploy: the entry revalidates whenever the phone is online.
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
    event.respondWith(
      (async () => {
        try {
          const res = await fetch(req);
          if (res.ok) {
            (await caches.open(CACHE)).put(req, res.clone());
          }
          return res;
        } catch {
          const cached = await caches.match(req);
          if (cached) return cached;
          throw new Error("offline and uncached");
        }
      })()
    );
  }
});
