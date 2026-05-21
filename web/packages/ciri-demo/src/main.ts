// Demo entry point. Constructs a CiriApp against the dev bridge
// (ws://localhost:8090 by default) and wires it to the page's status
// strip so connection state + errors are visible without opening
// devtools.
//
// URL params:
//   ?ws=ws://host:port    override the WebSocket URL
//   ?session=name         attach to a named session. When omitted, the
//                         client sends the `__auto__` sentinel and the
//                         server picks the most-recently-attached session
//                         (or creates a fresh one) — same as bare
//                         `ciritty`. The resolved name is then written
//                         back into the URL so a reload reattaches to it.
//   ?token=...            optional auth token forwarded to the transport

import { CiriApp } from "@ciri/app";

// Server-side sentinel: "auto-attach to the most recent / a new session".
// Mirrors `AUTO_SESSION` in crates/ciri-server/src/daemon/connection.rs.
const AUTO_SESSION = "__auto__";

const params = new URLSearchParams(window.location.search);
const wsUrl = params.get("ws") ?? "ws://localhost:8090";
const requestedSession = params.get("session");
const sessionName = requestedSession ?? AUTO_SESSION;
const token = params.get("token");

const statusEl = document.getElementById("status") as HTMLDivElement;
const stateEl = statusEl.querySelector(".state") as HTMLSpanElement;
const urlEl = statusEl.querySelector(".url") as HTMLSpanElement;
const errEl = statusEl.querySelector(".err") as HTMLSpanElement;
const rootEl = document.getElementById("app") as HTMLDivElement;

// Show "auto…" rather than the raw `__auto__` sentinel until the
// server reports the resolved name via onSessionChange.
function renderUrl(name: string): void {
  urlEl.textContent = `${wsUrl}  ·  ${name === AUTO_SESSION ? "auto…" : name}`;
}
renderUrl(sessionName);

function setState(state: "connecting" | "connected" | "error", text: string): void {
  statusEl.classList.remove("connecting", "connected", "error");
  statusEl.classList.add(state);
  stateEl.textContent = text;
}

// Pin the URL to the concrete session the server attached us to, so a
// reload/share reattaches to the same one instead of re-running
// auto-attach (which could pick a different "most recent"). Uses
// replaceState so it doesn't add a history entry per attach.
function pinSessionInUrl(name: string): void {
  renderUrl(name);
  const url = new URL(window.location.href);
  if (url.searchParams.get("session") === name) return;
  url.searchParams.set("session", name);
  window.history.replaceState(null, "", url);
}

const app = new CiriApp(rootEl, {
  url: wsUrl,
  sessionName,
  ...(token !== null ? { token } : {}),
  onOpen: () => {
    setState("connected", "connected");
    errEl.textContent = "";
  },
  onClose: (reason, reconnecting) => {
    setState(
      reconnecting ? "connecting" : "error",
      reconnecting ? `reconnecting (${reason})…` : `disconnected: ${reason}`,
    );
  },
  onError: (err) => {
    setState("error", "error");
    errEl.textContent = err.message;
    // Also log to the console so the full stack is available.
    console.error("[ciri-demo]", err);
  },
  onSessionChange: (name) => {
    pinSessionInUrl(name);
  },
  onServerShutdown: () => {
    // Distinct from a transport drop: the daemon stopped on purpose,
    // so don't imply a reconnect is coming.
    setState("error", "server shut down");
    errEl.textContent = "";
  },
});

app.start();

// Re-measure cells once the web font has finished loading. The first
// measurement in the constructor used whatever fallback the browser
// had ready; with the proper monospace font the cell pixel size may
// shift slightly, and the server needs to know the real geometry to
// pick the right rows/cols.
if (document.fonts !== undefined && typeof document.fonts.ready?.then === "function") {
  document.fonts.ready.then(() => app.remeasureCells()).catch(() => {
    // Fonts API isn't critical — silently ignore failures.
  });
}

// Make the app handle available to the devtools console for debugging.
(window as unknown as { __ciri?: CiriApp }).__ciri = app;
