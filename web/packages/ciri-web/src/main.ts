// Production entry for the ciritty browser terminal.
//
// Unlike @ciri/demo (dev-time, talks to the @ciri/bridge on a hardcoded
// localhost port), this build is served by ciri-server's own `[web]`
// gateway. It therefore:
//   * connects SAME-ORIGIN to the gateway's `/ws` endpoint (no config),
//   * gates on a token entered in a login screen and kept in
//     sessionStorage — never placed in the page URL, so it can't leak
//     via history, Referer, or the service-worker cache.

import "@ciri/app/chrome.css";
import { CiriApp } from "@ciri/app";

// Server-side sentinel: "attach to the most-recent / a new session".
// Mirrors `AUTO_SESSION` in crates/ciri-server/src/daemon/connection.rs.
const AUTO_SESSION = "__auto__";

// Per-tab token store. sessionStorage (not localStorage) so closing the
// tab forgets the token; survives reloads within the tab.
const TOKEN_KEY = "ciri.web.token";

// Same-origin WS endpoint, derived RELATIVE to the directory the page is
// served from (not the origin root). Directly off ciri-server the page is
// at `/`, so this is `/ws`; behind a reverse proxy that mounts the bundle
// under a prefix (e.g. `/ciri/`, which the relative-asset `base: './'`
// build already supports), the page is at `/ciri/` and this becomes
// `/ciri/ws` — so the proxy strips the prefix and forwards `/ws` to the
// gateway. Strip any trailing filename (`…/index.html`) to get the dir.
const appBase = location.pathname.replace(/[^/]*$/, "");
const wsUrl = `${location.protocol === "https:" ? "wss:" : "ws:"}//${location.host}${appBase}ws`;

const params = new URLSearchParams(location.search);
const requestedSession = params.get("session");
const sessionName = requestedSession ?? AUTO_SESSION;

const statusEl = document.getElementById("status") as HTMLDivElement;
const stateEl = statusEl.querySelector(".state") as HTMLSpanElement;
const urlEl = statusEl.querySelector(".url") as HTMLSpanElement;
const errEl = statusEl.querySelector(".err") as HTMLSpanElement;
const rootEl = document.getElementById("app") as HTMLDivElement;
const loginEl = document.getElementById("login") as HTMLDivElement;
const loginForm = document.getElementById("login-form") as HTMLFormElement;
const loginInput = document.getElementById("login-token") as HTMLInputElement;
const loginErrEl = document.getElementById("login-err") as HTMLDivElement;

let app: CiriApp | null = null;
// Did THIS connect() ever reach `onOpen`? A close before the first open
// almost always means a rejected token (the browser WebSocket API hides
// the 401 body), which routes back to login.
let everOpened = false;
// Consecutive reconnect closes since the last successful open. A daemon
// restart with a rotated token rejects every reconnect handshake before
// `onOpen`; rather than spin in "reconnecting" forever, once enough pile up
// we return to login so the user can enter the new token. Reset on a
// successful (re)open, so a transient blip that recovers never trips it.
let failedReconnects = 0;
const MAX_RECONNECTS_BEFORE_RELOGIN = 5;

function setState(state: "connecting" | "connected" | "error", text: string): void {
  statusEl.classList.remove("connecting", "connected", "error");
  statusEl.classList.add(state);
  stateEl.textContent = text;
}

function renderUrl(name: string): void {
  urlEl.textContent = name === AUTO_SESSION ? "auto…" : name;
}

// Pin the resolved session into the URL so a reload/share reattaches to
// the same one instead of re-running auto-attach. replaceState avoids a
// history entry per attach. (Only the session name — never the token.)
function pinSessionInUrl(name: string): void {
  const url = new URL(location.href);
  if (url.searchParams.get("session") === name) return;
  url.searchParams.set("session", name);
  history.replaceState(null, "", url);
}

function showLogin(prefill: string, message: string): void {
  loginInput.value = prefill;
  loginErrEl.textContent = message;
  loginEl.hidden = false;
  statusEl.hidden = true;
  loginInput.focus();
  loginInput.select();
}

function teardown(): void {
  if (app !== null) {
    app.destroy();
    app = null;
  }
}

function connect(token: string): void {
  everOpened = false;
  failedReconnects = 0;
  loginEl.hidden = true;
  statusEl.hidden = false;
  setState("connecting", "connecting…");
  renderUrl(sessionName);

  app = new CiriApp(rootEl, {
    url: wsUrl,
    sessionName,
    token,
    onOpen: () => {
      everOpened = true;
      failedReconnects = 0;
      setState("connected", "connected");
      errEl.textContent = "";
    },
    onClose: (reason, reconnecting) => {
      if (!everOpened) {
        // Never reached onOpen → almost certainly a rejected/invalid token.
        // Tear the app down (this also cancels its reconnect loop), forget
        // the token, and re-show the login screen with the attempted value
        // pre-filled so a transient blip is a one-click retry.
        teardown();
        sessionStorage.removeItem(TOKEN_KEY);
        showLogin(token, "Connection refused — check the token and try again.");
        return;
      }
      if (reconnecting) {
        // A drop after a working connection. Auto-reconnect rides out a
        // transient blip (the counter resets on the next onOpen), but if
        // handshakes keep failing before re-opening — e.g. the daemon
        // restarted with a rotated token — stop the endless spinner and
        // return to login so the user can enter the new token.
        failedReconnects += 1;
        if (failedReconnects >= MAX_RECONNECTS_BEFORE_RELOGIN) {
          teardown();
          sessionStorage.removeItem(TOKEN_KEY);
          showLogin(token, "Reconnect keeps failing — the token may have changed. Log in again.");
          return;
        }
        setState("connecting", `reconnecting (${reason})…`);
      } else {
        setState("error", `disconnected: ${reason}`);
      }
    },
    onError: (err) => {
      setState("error", "error");
      errEl.textContent = err.message;
      console.error("[ciri-web]", err);
    },
    onSessionChange: (name) => {
      renderUrl(name);
      pinSessionInUrl(name);
    },
    onServerShutdown: () => {
      setState("error", "server shut down");
      errEl.textContent = "";
    },
  });
  app.start();

  // Re-measure cells once the web font is ready; the first measurement
  // used a fallback face whose cell size may differ slightly.
  if (document.fonts !== undefined && typeof document.fonts.ready?.then === "function") {
    document.fonts.ready.then(() => app?.remeasureCells()).catch(() => {
      /* Fonts API isn't critical — ignore. */
    });
  }

  // Expose the live handle for devtools debugging.
  (window as unknown as { __ciri?: CiriApp | null }).__ciri = app;
}

loginForm.addEventListener("submit", (e) => {
  e.preventDefault();
  const token = loginInput.value.trim();
  if (token.length === 0) {
    loginErrEl.textContent = "Token is required.";
    return;
  }
  sessionStorage.setItem(TOKEN_KEY, token);
  connect(token);
});

// Auto-connect when this tab already has a token; otherwise show login.
const stored = sessionStorage.getItem(TOKEN_KEY);
if (stored !== null && stored.length > 0) {
  connect(stored);
} else {
  showLogin("", "");
}

// Register the app-shell service worker — production builds only. In
// `vite dev` its cache-first strategy fights HMR. Registration failure
// (insecure origin, unsupported browser) is non-fatal.
if (import.meta.env.PROD && "serviceWorker" in navigator) {
  window.addEventListener("load", () => {
    navigator.serviceWorker.register("./sw.js").catch(() => {
      /* SW is optional; ignore. */
    });
    // The SW registers after this page already fetched its content-hashed
    // JS/CSS, so those bypassed the worker's fetch handler and aren't
    // cached. Hand the active worker the list of same-origin assets this
    // load used, so a first-load-then-offline still opens the shell.
    // Best-effort; later loads are cached by the SW's cache-first handler.
    navigator.serviceWorker.ready
      .then((reg) => {
        const urls = performance
          .getEntriesByType("resource")
          .map((e) => e.name)
          .filter((n) => {
            if (!n.startsWith(location.origin)) return false;
            const path = n.split("?")[0] ?? n;
            return path.endsWith(".js") || path.endsWith(".css");
          });
        reg.active?.postMessage({ type: "cache-assets", urls });
      })
      .catch(() => {
        /* precache hint is best-effort */
      });
  });
}
