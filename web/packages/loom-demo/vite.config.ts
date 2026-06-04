import { defineConfig } from "vite";

// Vite config for the dev demo. Resolves @loom/* via the workspace
// `dist` outputs (matching the package.json `exports` field), so the
// prebuild-chain in the workspace root must have run at least once
// before `vite dev`.
//
// `server.host=127.0.0.1` mirrors the bridge's default bind host —
// the bridge is dev-only, never on a public interface. Override with
// `--host` on the CLI if you need LAN access (and accept the risk).
export default defineConfig({
  server: {
    host: "127.0.0.1",
    port: 5173,
    strictPort: false,
  },
});
