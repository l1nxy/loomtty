import { defineConfig } from "vitest/config";

// The renderer manipulates the DOM directly (createElement, replaceChildren,
// row-mounted text nodes) — jsdom gives us a faithful enough DOM to drive
// it from Node. The grid + theme + sgr-run modules don't need a DOM and
// would run fine under "node", but using one environment keeps the
// per-file boilerplate minimal.
export default defineConfig({
  test: {
    environment: "jsdom",
  },
});
