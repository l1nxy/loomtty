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

for (const { example, out } of tasks) {
  process.stdout.write(`running cargo example: ${example} → ${out}\n`);
  let stdout;
  try {
    stdout = execFileSync(
      "cargo",
      ["run", "--quiet", "-p", "ciri-protocol", "--example", example],
      { cwd: repoRoot, encoding: "buffer", stdio: ["ignore", "pipe", "inherit"] },
    );
  } catch (e) {
    if (e && typeof e === "object" && "code" in e && e.code === "ENOENT") {
      process.stderr.write(
        "\nfailed to find `cargo` on PATH — install Rust (https://rustup.rs) " +
          "and re-run.\n",
      );
      process.exit(1);
    }
    throw e;
  }
  // Write as raw bytes so the UTF-8 output cargo wrote is preserved
  // byte-for-byte regardless of the host shell's default encoding.
  writeFileSync(out, stdout);
}
