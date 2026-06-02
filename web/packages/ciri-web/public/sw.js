// Minimal app-shell service worker for the ciritty web client.
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

self.addEventListener("message", (event) => {
  // The page hands us the asset URLs it loaded BEFORE this worker took
  // control (the SW registers after the initial JS/CSS were already
  // fetched, so they bypassed the `fetch` handler and never got cached).
  // Caching them here makes a first-load-then-offline launch actually open
  // the shell instead of failing on missing chunks.
  const data = event.data;
  if (!data || data.type !== "cache-assets" || !Array.isArray(data.urls)) return;
  // Same safety as the fetch handler: only same-origin, query-less URLs, so
  // a request that ever carried a `?token=`-style secret can't be persisted.
  const safe = data.urls.filter((u) => {
    try {
      const url = new URL(u);
      return url.origin === self.location.origin && url.search === "";
    } catch {
      return false;
    }
  });
  if (safe.length === 0) return;
  event.waitUntil(caches.open(CACHE).then((c) => c.addAll(safe).catch(() => {})));
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
          // page must not poison the offline shell fallback. Cache under
          // the canonical "./" key, NOT `req`: the app shell is the same
          // regardless of query string, and keying by the full request
          // would persist the `?token=` bearer in Cache Storage.
          if (res.ok) {
            const copy = res.clone();
            caches.open(CACHE).then((c) => c.put("./", copy));
          }
          return res;
        })
        .catch(() => caches.match("./")),
    );
    return;
  }

  event.respondWith(
    caches.match(req).then(
      (hit) =>
        hit ??
        fetch(req).then((res) => {
          // Cache only clean (query-less) asset URLs. Vite's hashed assets
          // carry no query string, so this loses nothing — and it
          // guarantees a request that ever carried a `?token=`-style secret
          // in its query can never be persisted to Cache Storage.
          if (res.ok && url.search === "") {
            const copy = res.clone();
            caches.open(CACHE).then((c) => c.put(req, copy));
          }
          return res;
        }),
    ),
  );
});
