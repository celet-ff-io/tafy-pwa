// tafy-pwa service worker v0: cache-first shell for offline launch.
// S1 scope: mock-data UI only. The cache version bumps on every
// deploy-triggered refresh of this file.
const CACHE = "tafy-shell-v0";

self.addEventListener("install", (event) => {
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

self.addEventListener("fetch", (event) => {
  const req = event.request;
  if (req.method !== "GET" || new URL(req.url).origin !== self.location.origin) return;
  event.respondWith(
    (async () => {
      const cached = await caches.match(req);
      if (cached) return cached;
      const res = await fetch(req);
      if (res.ok) {
        const clone = res.clone();
        const cache = await caches.open(CACHE);
        cache.put(req, clone);
      }
      return res;
    })()
  );
});
