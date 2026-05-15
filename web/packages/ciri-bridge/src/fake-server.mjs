#!/usr/bin/env node
// Fake ciri-server stand-in for testing the bridge in isolation.
// Accepts a TCP connection on 7890, echoes 8 dummy bytes back
// (matching ServerHello shape), then idles. Used by the probe
// smoke-test when no real ciri-server is available.
import { createServer } from "node:net";
const port = Number(process.argv[2] ?? "7890");
const srv = createServer((sock) => {
  process.stdout.write(`fake-server: client connected from ${sock.remoteAddress}\n`);
  sock.on("data", (chunk) => {
    process.stdout.write(`fake-server: received ${chunk.length} bytes; echoing 8-byte preamble\n`);
    sock.write(Buffer.from([0x43, 0x49, 0x52, 0x49, 0, 0, 0, 0]));
  });
  sock.on("close", () => process.stdout.write(`fake-server: client disconnected\n`));
});
srv.listen(port, "127.0.0.1", () => {
  process.stdout.write(`fake-server: listening on 127.0.0.1:${port}\n`);
});
