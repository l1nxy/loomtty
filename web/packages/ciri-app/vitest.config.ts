import { defineConfig } from "vitest/config";

// `@ciri/app` mounts real DOM trees (workspace switcher + per-pane
// renderers) and listens to KeyboardEvent / WheelEvent / ResizeObserver
// — jsdom gives us the parts of the browser API we touch in v1. The
// non-DOM modules (input encoder, key→bytes mapping) are pure and
// would run fine under "node", but a single environment keeps the
// per-file boilerplate minimal.
export default defineConfig({
  test: {
    environment: "jsdom",
  },
});
