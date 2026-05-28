// Minimal app-shell service worker for the ciritty web demo.
//
// Purpose: make the page installable (PWA) and open instantly by
// caching its static shell (HTML + content-hashed JS/CSS). It does NOT
// touch the live terminal — that runs over a WebSocket, which a service
// worker never intercepts — so going offline just means the shell loads
// but the connection (expectedly) fails.
//
// Strategy:
//   * navigations  → network-first (pick up new deploys), cache fallback
//   * static GETs  → cache-first (Vite asset URLs are content-hashed and
//                    immutable, so a hit is always correct)
// Cross-origin and non-GET requests are passed straight through.

const CACHE = "ciritty-shell-v1";

self.addEventListener("install", (event) => {
  // Pre-cache the app shell so an offline launch works even on the
  // first run after this worker takes control — the navigation
  // fallback below depends on "./" being in the cache. A precache miss
  // (e.g. installed while flaky) must not fail the install.
  event.waitUntil(
    caches
      .open(CACHE)
      .then((c) => c.add("./"))
      .catch(() => {}),
  );
  // Activate this worker as soon as it finishes installing instead of
  // waiting for every old tab to close.
  self.skipWaiting();
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) =>
        Promise.all(keys.filter((k) => k !== CACHE).map((k) => caches.delete(k))),
      )
      .then(() => self.clients.claim()),
  );
});

self.addEventListener("fetch", (event) => {
  const req = event.request;
  if (req.method !== "GET") return;
  const url = new URL(req.url);
  if (url.origin !== self.location.origin) return;

  if (req.mode === "navigate") {
    event.respondWith(
      fetch(req)
        .then((res) => {
          // Only cache a successful response — a transient 500/404 HTML
          // page must not poison the offline shell fallback.
          if (res.ok) {
            const copy = res.clone();
            caches.open(CACHE).then((c) => c.put(req, copy));
          }
          return res;
        })
        .catch(() =>
          caches.match(req).then((hit) => hit ?? caches.match("./")),
        ),
    );
    return;
  }

  event.respondWith(
    caches.match(req).then(
      (hit) =>
        hit ??
        fetch(req).then((res) => {
          if (res.ok) {
            const copy = res.clone();
            caches.open(CACHE).then((c) => c.put(req, copy));
          }
          return res;
        }),
    ),
  );
});
