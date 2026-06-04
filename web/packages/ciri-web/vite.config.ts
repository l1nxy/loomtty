import { defineConfig } from "vite";

// Production SPA, served by ciri-server's `[web]` gateway on the same
// port that handles the `/ws` upgrade. `base: "./"` keeps every asset URL
// relative so the bundle works no matter where it's mounted. The default
// build output (`dist/`) is copied into the server's static dir
// (`<server-exe-dir>/web`); see web/scripts/install-assets.mjs.
export default defineConfig({
  base: "./",
  build: {
    target: "es2022",
  },
});
