#!/usr/bin/env node
// Build the production web bundle (@ciri/web) and copy it into the
// directory ciri-server serves the SPA from.
//
// ciri-server resolves `web.static_dir` (empty → `<server-exe-dir>/web`)
// at startup and serves those files over HTTP on the `[web]` port. This
// script produces that directory end to end:
//
//   node web/scripts/install-assets.mjs [dest]
//
// `dest` defaults to `<repo>/target/debug/web` — the neighbour of the
// dev server binary (`target/debug/ciritty-server`), i.e. the default
// `static_dir`. For a release build pass `target/release/web`; for a
// real install pass the directory next to the installed binary.

import { execSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, rmSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const webRoot = resolve(here, ".."); // web/
const repoRoot = resolve(webRoot, ".."); // repository root
const dist = join(webRoot, "packages", "ciri-web", "dist");
const dest = resolve(process.argv[2] ?? join(repoRoot, "target", "debug", "web"));

const run = (cmd) => execSync(cmd, { cwd: webRoot, stdio: "inherit" });

// The bundle imports @ciri/{codec,client,dom,app} through their built
// `dist/` outputs, so prime the chain before vite resolves them.
console.log("→ building library chain…");
run("npm run prebuild-chain");
console.log("→ building @ciri/web…");
run("npm run build -w @ciri/web");

if (!existsSync(dist)) {
  console.error(`✗ build produced no dist at ${dist}`);
  process.exit(1);
}

console.log(`→ installing ${dist}\n            → ${dest}`);
rmSync(dest, { recursive: true, force: true });
mkdirSync(dest, { recursive: true });
cpSync(dist, dest, { recursive: true });

console.log(
  `✓ done.\n` +
    `  Start it with:  ciritty web\n` +
    `  (or, pointing the server straight at this dir:\n` +
    `     ciritty-server --headless --web --web-static-dir "${dest}")`,
);
