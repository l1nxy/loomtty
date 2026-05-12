#!/usr/bin/env node
// Regenerate @ciri/codec's constants and Rust-emitted fixtures.
// Cross-platform: avoids `>` shell redirection (which uses the system
// codepage on Windows cmd.exe and can corrupt UTF-8 / line endings).

import { execFileSync } from "node:child_process";
import { mkdirSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, "..", "..");
const codecSrc = resolve(repoRoot, "web", "packages", "ciri-codec", "src");
const fixturesDir = resolve(codecSrc, "__fixtures__");
mkdirSync(fixturesDir, { recursive: true });

const tasks = [
  {
    example: "dump_constants",
    out: resolve(codecSrc, "constants.ts"),
  },
  {
    example: "sm_fixtures",
    out: resolve(fixturesDir, "sm.json"),
  },
  {
    example: "lz4_fixture",
    out: resolve(fixturesDir, "lz4.json"),
  },
];

// Cap stdout at 64 MiB. The default execFileSync maxBuffer is 1 MiB —
// the current dump outputs are tens of KB, but a future constants
// table growth (or sm_fixtures expansion) could silently truncate
// without an explicit cap. 64 MiB is well above what we'll ever emit
// and well below "you should worry about RAM" territory.
const MAX_STDOUT = 64 * 1024 * 1024;

for (const { example, out } of tasks) {
  process.stdout.write(`running cargo example: ${example} → ${out}\n`);
  let stdout;
  try {
    stdout = execFileSync(
      "cargo",
      ["run", "--quiet", "-p", "ciri-protocol", "--example", example],
      {
        cwd: repoRoot,
        encoding: "buffer",
        stdio: ["ignore", "pipe", "inherit"],
        maxBuffer: MAX_STDOUT,
      },
    );
  } catch (e) {
    if (e && typeof e === "object" && "code" in e) {
      if (e.code === "ENOENT") {
        process.stderr.write(
          "\nfailed to find `cargo` on PATH — install Rust (https://rustup.rs) " +
            "and re-run.\n",
        );
        process.exit(1);
      }
      if (e.code === "ENOBUFS") {
        process.stderr.write(
          `\ncargo example ${example} exceeded the ${MAX_STDOUT} byte stdout ` +
            "buffer cap — raise MAX_STDOUT in scripts/gen.mjs if this is " +
            "legitimate, otherwise something is wrong with the example.\n",
        );
        process.exit(1);
      }
    }
    throw e;
  }
  // Write as raw bytes so the UTF-8 output cargo wrote is preserved
  // byte-for-byte regardless of the host shell's default encoding.
  writeFileSync(out, stdout);
}
