#!/usr/bin/env node
// ciri-bridge — dev-time WebSocket ↔ TCP bridge.
//
// ciri-server speaks the ciri-protocol (length-prefixed frames) over
// raw TCP when `[remote] enabled = true`. Browsers can't speak raw
// TCP, so this script bridges:
//
//   browser ──ws://localhost:8090── bridge ──tcp://127.0.0.1:7890── ciri-server
//
// Each WebSocket connection opens its own TCP socket; bytes flow
// unmodified in both directions. The ws library hands us Buffer
// chunks; we forward them as-is. From the ciri-server's side this
// looks indistinguishable from a native TCP client.
//
// NOT FOR PRODUCTION:
//   - no auth (matching ciri-server's "intended for SSH tunnel use only" stance)
//   - no TLS (use `wss://` via a real reverse proxy if exposed)
//   - no rate limiting, no resource caps
//   - binds to 127.0.0.1 by default for the same reason
//
// CLI:
//   ciri-bridge [--ws-port 8090] [--ws-host 127.0.0.1]
//               [--tcp-port 7890] [--tcp-host 127.0.0.1]

import { WebSocketServer } from "ws";
import { createConnection } from "node:net";
import { parseArgs } from "node:util";

const { values } = parseArgs({
  options: {
    "ws-port": { type: "string", default: "8090" },
    "ws-host": { type: "string", default: "127.0.0.1" },
    "tcp-port": { type: "string", default: "7890" },
    "tcp-host": { type: "string", default: "127.0.0.1" },
    help: { type: "boolean", short: "h", default: false },
  },
});

if (values.help) {
  process.stdout.write(
    `ciri-bridge — WebSocket ↔ TCP bridge for the web client\n` +
      `\n` +
      `Usage: ciri-bridge [options]\n` +
      `\n` +
      `  --ws-port <N>     WebSocket listen port (default 8090)\n` +
      `  --ws-host <addr>  WebSocket bind host (default 127.0.0.1)\n` +
      `  --tcp-port <N>    ciri-server TCP port (default 7890)\n` +
      `  --tcp-host <addr> ciri-server TCP host (default 127.0.0.1)\n` +
      `  -h, --help        Show this help\n`,
  );
  process.exit(0);
}

const wsPort = Number(values["ws-port"]);
const wsHost = values["ws-host"];
const tcpPort = Number(values["tcp-port"]);
const tcpHost = values["tcp-host"];

if (!Number.isFinite(wsPort) || wsPort < 1 || wsPort > 65535) {
  process.stderr.write(`invalid --ws-port: ${values["ws-port"]}\n`);
  process.exit(2);
}
if (!Number.isFinite(tcpPort) || tcpPort < 1 || tcpPort > 65535) {
  process.stderr.write(`invalid --tcp-port: ${values["tcp-port"]}\n`);
  process.exit(2);
}

const wss = new WebSocketServer({ host: wsHost, port: wsPort });

wss.on("listening", () => {
  process.stdout.write(
    `ciri-bridge: ws://${wsHost}:${wsPort} → tcp://${tcpHost}:${tcpPort}\n`,
  );
});

wss.on("error", (err) => {
  process.stderr.write(`ciri-bridge: ws server error: ${err.message}\n`);
});

let connectionSeq = 0;

wss.on("connection", (ws, req) => {
  const id = ++connectionSeq;
  const peer = req.socket.remoteAddress ?? "?";
  process.stdout.write(`[${id}] ws connect from ${peer}\n`);

  // Buffer messages received before the TCP socket has connected so
  // the initial ClientHello isn't dropped on the floor.
  const earlyBuffer = [];
  let tcpReady = false;
  let closed = false;

  const tcp = createConnection({ host: tcpHost, port: tcpPort });
  // `noDelay = true` so the small initial handshake bytes get flushed
  // immediately — same setting ciri-server applies on its side.
  tcp.setNoDelay(true);

  const closeBoth = (reason) => {
    if (closed) return;
    closed = true;
    process.stdout.write(`[${id}] closing: ${reason}\n`);
    try {
      ws.close();
    } catch {
      // ignore
    }
    try {
      tcp.destroy();
    } catch {
      // ignore
    }
  };

  tcp.on("connect", () => {
    tcpReady = true;
    process.stdout.write(`[${id}] tcp connected\n`);
    for (const chunk of earlyBuffer) tcp.write(chunk);
    earlyBuffer.length = 0;
  });

  tcp.on("data", (chunk) => {
    // ws will fragment if too large, but for our framed protocol
    // (≤64KB-ish typical frames) one send call is fine. binary=true
    // is the default for Buffer input, but be explicit.
    ws.send(chunk, { binary: true }, (err) => {
      if (err) closeBoth(`ws send error: ${err.message}`);
    });
  });

  tcp.on("error", (err) => closeBoth(`tcp error: ${err.message}`));
  tcp.on("close", () => closeBoth("tcp closed"));

  ws.on("message", (data, isBinary) => {
    if (!isBinary) {
      closeBoth("non-binary ws message");
      return;
    }
    // `data` is either Buffer (single frame) or Buffer[] (fragmented).
    // Concatenate the latter so the TCP write is one ordered chunk.
    const chunk = Array.isArray(data) ? Buffer.concat(data) : data;
    if (tcpReady) {
      tcp.write(chunk);
    } else {
      earlyBuffer.push(chunk);
    }
  });

  ws.on("close", () => closeBoth("ws closed"));
  ws.on("error", (err) => closeBoth(`ws error: ${err.message}`));
});

const shutdown = (signal) => {
  process.stdout.write(`\nciri-bridge: ${signal} received, shutting down\n`);
  wss.close(() => process.exit(0));
  // Hard exit if not gone in 2s.
  setTimeout(() => process.exit(0), 2000).unref();
};
process.on("SIGINT", () => shutdown("SIGINT"));
process.on("SIGTERM", () => shutdown("SIGTERM"));
