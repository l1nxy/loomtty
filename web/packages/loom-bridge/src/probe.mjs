#!/usr/bin/env node
// loom-bridge probe — sanity-checks the full wire path without a
// browser. Opens a WebSocket to the bridge, sends a real
// ClientHello via @loom/client's encoder, waits for the first
// frame back from loom-server.
//
// Use this to confirm the three-piece dev setup (loom-server with
// `[remote] enabled = true` + bridge + reachable host) is correctly
// wired before opening the browser demo.
//
// Usage:
//   node packages/loom-bridge/src/probe.mjs \
//       [--url ws://localhost:8090] [--session default] [--timeout 5000]
//
// Exit codes:
//   0 — handshake completed, at least one server byte stream received
//   1 — timed out waiting for a server frame
//   2 — connection / protocol error

import { WebSocket } from "ws";
import { parseArgs } from "node:util";
import { encodeClientHello } from "@loom/client";

const { values } = parseArgs({
  options: {
    url: { type: "string", default: "ws://localhost:8090" },
    session: { type: "string", default: "default" },
    timeout: { type: "string", default: "5000" },
  },
});

const url = values.url;
const sessionName = values.session;
const timeoutMs = Number(values.timeout);

const ws = new WebSocket(url);
ws.binaryType = "nodebuffer";

let sawAnyMessage = false;

const timeout = setTimeout(() => {
  if (sawAnyMessage) return;
  process.stderr.write(
    `probe: timed out after ${timeoutMs}ms waiting for a server frame\n`,
  );
  try {
    ws.close();
  } catch {
    // ignore
  }
  process.exit(1);
}, timeoutMs);

ws.on("open", () => {
  process.stdout.write(`probe: connected to ${url}\n`);
  const hello = encodeClientHello({
    sessionName,
    width: 800,
    height: 600,
    cellWidth: 9,
    cellHeight: 18,
  });
  ws.send(hello, { binary: true });
});

ws.on("message", (data) => {
  // Don't try to parse — the bridge transports raw loom-protocol
  // bytes, and the first response is the ServerHello preamble
  // (8 bytes), not a framed message. Receiving anything at all is
  // proof the path works end-to-end.
  if (!sawAnyMessage) {
    sawAnyMessage = true;
    process.stdout.write(
      `probe: handshake OK — first response was ${data.length} bytes\n`,
    );
    clearTimeout(timeout);
    ws.close();
    process.exit(0);
  }
});

ws.on("error", (err) => {
  process.stderr.write(`probe: ws error: ${err.message}\n`);
  clearTimeout(timeout);
  process.exit(2);
});

ws.on("close", (code, reason) => {
  if (!sawAnyMessage) {
    const why = reason?.toString() ?? "no reason";
    process.stderr.write(`probe: socket closed (code=${code}, ${why})\n`);
    clearTimeout(timeout);
    process.exit(2);
  }
});
