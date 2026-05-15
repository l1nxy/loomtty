// Demo entry point. Constructs a CiriApp against the dev bridge
// (ws://localhost:8090 by default) and wires it to the page's status
// strip so connection state + errors are visible without opening
// devtools.
//
// URL params:
//   ?ws=ws://host:port    override the WebSocket URL
//   ?session=name         override the session name (default: "default")
//   ?token=...            optional auth token forwarded to the transport

import { CiriApp } from "@ciri/app";

const params = new URLSearchParams(window.location.search);
const wsUrl = params.get("ws") ?? "ws://localhost:8090";
const sessionName = params.get("session") ?? "default";
const token = params.get("token");

const statusEl = document.getElementById("status") as HTMLDivElement;
const stateEl = statusEl.querySelector(".state") as HTMLSpanElement;
const urlEl = statusEl.querySelector(".url") as HTMLSpanElement;
const errEl = statusEl.querySelector(".err") as HTMLSpanElement;
const rootEl = document.getElementById("app") as HTMLDivElement;

urlEl.textContent = `${wsUrl}  ·  ${sessionName}`;

function setState(state: "connecting" | "connected" | "error", text: string): void {
  statusEl.classList.remove("connecting", "connected", "error");
  statusEl.classList.add(state);
  stateEl.textContent = text;
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
