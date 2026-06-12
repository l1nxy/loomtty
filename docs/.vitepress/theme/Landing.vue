<script setup>
import { withBase } from 'vitepress'

const features = [
  {
    n: '01',
    title: 'Column + workspace layout',
    body: 'Scrollable, niri-style columns. Workspaces stack vertically, tiles stack within a column.',
  },
  {
    n: '02',
    title: 'GPU rendering',
    body: 'Vulkan, Metal, OpenGL, and DirectX 11. Ligatures and inline images (Kitty / Sixel).',
  },
  {
    n: '03',
    title: 'Remote attach',
    body: 'Attach to a session on another host over a binary protocol and an SSH tunnel.',
  },
  {
    n: '04',
    title: 'Browser client',
    body: 'loomtty web serves the same session in the browser, installable as a PWA.',
  },
  {
    n: '05',
    title: 'Predictive echo',
    body: 'mosh-style local echo keeps typing responsive across high-latency links.',
  },
  {
    n: '06',
    title: 'Agent-aware sessions',
    body: 'Restores Claude Code, Codex, opencode, and Droid panes when you reattach.',
  },
  {
    n: '07',
    title: 'Lua plugins',
    body: 'Sandboxed scripting with an event API for extending the multiplexer.',
    planned: true,
  },
  {
    n: '08',
    title: 'Themeable',
    body: 'Nine built-in presets and per-color overrides, live from the settings panel.',
  },
]
</script>

<template>
  <div class="lt">
    <!-- ───────────────────────── Hero ───────────────────────── -->
    <section class="lt-hero">
      <div class="lt-hero__grid">
        <div class="lt-hero__copy">
          <p class="lt-eyebrow">terminal multiplexer · rust · gpu</p>
          <h1 class="lt-title">Run many shells<br />in one window.</h1>
          <p class="lt-lede">
            loomtty lays your sessions out in scrollable columns, renders them on
            the GPU, and lets you attach from a browser or another machine.
          </p>

          <div class="lt-actions">
            <a class="lt-btn lt-btn--primary" :href="withBase('/guide/')">
              Get started
              <span class="lt-btn__arrow">→</span>
            </a>
            <a
              class="lt-btn lt-btn--ghost"
              href="https://github.com/l1nxy/loomtty"
              target="_blank"
              rel="noreferrer"
            >
              View on GitHub
            </a>
          </div>

          <p class="lt-meta">
            <span class="lt-tag">v0.1</span>
            <span class="lt-dot">·</span>
            <span>AGPL-3.0</span>
            <span class="lt-dot">·</span>
            <span>Linux</span>
            <span class="lt-dot">·</span>
            <span>Windows</span>
            <span class="lt-dot">·</span>
            <span class="lt-muted">macOS (WIP)</span>
          </p>
        </div>

        <!-- CSS terminal: a real loomtty column/tile layout -->
        <div class="lt-term" aria-hidden="true">
          <div class="lt-term__bar">
            <span class="lt-term__dots"><i></i><i></i><i></i></span>
            <span class="lt-term__path">loomtty — ~/projects/loom</span>
          </div>

          <div class="lt-term__status">
            <span class="lt-st-name">loom</span>
            <span class="lt-st-ws"><b>1</b>·2·3</span>
            <span class="lt-st-mode">NORMAL</span>
            <span class="lt-st-leader">ALT</span>
          </div>

          <div class="lt-term__body">
            <!-- focused column -->
            <div class="lt-col lt-col--focus">
              <div class="lt-line"><span class="lt-mark">●</span><span class="lt-path">~/loom</span><span class="lt-prompt">λ</span> cargo&nbsp;run</div>
              <div class="lt-line lt-dim">&nbsp;&nbsp;Compiling loom v0.1.0</div>
              <div class="lt-line lt-dim">&nbsp;&nbsp;Compiling loom-render</div>
              <div class="lt-line"><span class="lt-ok">✓</span>&nbsp;Finished in 4.2s</div>
              <div class="lt-line"><span class="lt-mark">●</span><span class="lt-path">~/loom</span><span class="lt-prompt">λ</span> <span class="lt-cursor"></span></div>
            </div>

            <!-- second column, two stacked tiles -->
            <div class="lt-col">
              <div class="lt-tile">
                <div class="lt-line lt-bright">src/main.rs</div>
                <div class="lt-line lt-dim">fn main() {</div>
                <div class="lt-line lt-dim">&nbsp;&nbsp;loom::run()</div>
                <div class="lt-line lt-dim">}</div>
              </div>
              <div class="lt-tile lt-tile--b">
                <div class="lt-line lt-bright">● remote/devbox</div>
                <div class="lt-bars">
                  <span style="--w: 72%"></span>
                  <span style="--w: 41%"></span>
                  <span style="--w: 88%"></span>
                </div>
              </div>
            </div>
          </div>
        </div>
      </div>

      <div class="lt-glow" aria-hidden="true"></div>
    </section>

    <!-- ──────────────────────── Features ─────────────────────── -->
    <section class="lt-section">
      <div class="lt-section__head">
        <p class="lt-eyebrow">features</p>
        <h2 class="lt-h2">Built for the terminal.</h2>
      </div>

      <div class="lt-grid">
        <article v-for="f in features" :key="f.n" class="lt-card">
          <span class="lt-card__head">
            <span class="lt-card__n">{{ f.n }}</span>
            <span v-if="f.planned" class="lt-card__tag">planned</span>
          </span>
          <h3 class="lt-card__title">{{ f.title }}</h3>
          <p class="lt-card__body">{{ f.body }}</p>
        </article>
      </div>
    </section>

    <!-- ──────────────────────── Quickstart ───────────────────── -->
    <section class="lt-section lt-section--cta">
      <div class="lt-cta">
        <div class="lt-cta__copy">
          <p class="lt-eyebrow">build it and run</p>
          <h2 class="lt-h2">Up and running in three commands.</h2>
          <p class="lt-lede lt-lede--tight">
            Needs Rust 1.82+. Prebuilt binaries live on the releases page; from
            source it's a single cargo build.
          </p>
          <div class="lt-actions">
            <a class="lt-btn lt-btn--primary" :href="withBase('/guide/installation')">
              Install guide
              <span class="lt-btn__arrow">→</span>
            </a>
            <a
              class="lt-btn lt-btn--ghost"
              href="https://github.com/l1nxy/loomtty/releases"
              target="_blank"
              rel="noreferrer"
            >
              Releases
            </a>
          </div>
        </div>

        <div class="lt-term lt-term--code" aria-hidden="true">
          <div class="lt-term__bar">
            <span class="lt-term__dots"><i></i><i></i><i></i></span>
            <span class="lt-term__path">bash</span>
          </div>
          <div class="lt-term__body lt-term__body--code">
            <div class="lt-line"><span class="lt-prompt">$</span> git clone https://github.com/l1nxy/loomtty</div>
            <div class="lt-line"><span class="lt-prompt">$</span> cd loomtty &amp;&amp; cargo build --release</div>
            <div class="lt-line"><span class="lt-prompt">$</span> ./target/release/loomtty</div>
            <div class="lt-line lt-dim">&nbsp;&nbsp;session attached · 2 panes restored</div>
          </div>
        </div>
      </div>
    </section>
  </div>
</template>

<style scoped>
.lt {
  /* terminal panel palette — monochrome, always dark */
  --t-bg: #000;
  --t-surface: #0b0b0b;
  --t-border: #262626;
  --t-text: #e4e4e4;
  --t-dim: #6e6e6e;
  --t-bright: #fafafa;

  max-width: 1180px;
  margin: 0 auto;
  padding: 0 24px;
}

/* ── shared type ── */
.lt-eyebrow {
  font-family: var(--vp-font-family-mono);
  font-size: 12px;
  letter-spacing: 0.16em;
  text-transform: uppercase;
  color: var(--vp-c-text-3);
  margin: 0 0 18px;
}
.lt-muted { color: var(--vp-c-text-3); }

/* ─────────────── hero ─────────────── */
.lt-hero {
  position: relative;
  overflow: hidden;
  padding: 72px 0 88px;
}
.lt-hero__grid {
  position: relative;
  z-index: 1;
  display: grid;
  grid-template-columns: 1fr;
  gap: 48px;
  align-items: center;
}
.lt-title {
  font-size: 52px;
  line-height: 1.02;
  letter-spacing: -0.03em;
  font-weight: 760;
  margin: 0 0 22px;
  background: linear-gradient(
    180deg,
    var(--vp-c-text-1),
    color-mix(in srgb, var(--vp-c-text-1) 64%, var(--vp-c-bg))
  );
  -webkit-background-clip: text;
  background-clip: text;
  color: transparent;
}
.lt-lede {
  font-size: 18px;
  line-height: 1.6;
  color: var(--vp-c-text-2);
  max-width: 30em;
  margin: 0 0 32px;
}
.lt-lede--tight { margin-bottom: 28px; }

.lt-actions {
  display: flex;
  flex-wrap: wrap;
  gap: 12px;
  margin-bottom: 26px;
}
.lt-btn {
  display: inline-flex;
  align-items: center;
  gap: 8px;
  height: 46px;
  padding: 0 22px;
  border-radius: 8px;
  font-size: 15px;
  font-weight: 600;
  transition: transform 0.12s ease, border-color 0.15s ease,
    background 0.15s ease, opacity 0.15s ease;
  border: 1px solid transparent;
  white-space: nowrap;
}
.lt-btn:hover { transform: translateY(-1px); }
.lt-btn--primary {
  color: var(--vp-c-bg);
  background: var(--vp-c-text-1);
}
.lt-btn--primary:hover { opacity: 0.86; }
.lt-btn__arrow { transition: transform 0.15s ease; }
.lt-btn--primary:hover .lt-btn__arrow { transform: translateX(3px); }
.lt-btn--ghost {
  color: var(--vp-c-text-1);
  border-color: var(--vp-c-divider);
  background: transparent;
}
.lt-btn--ghost:hover { border-color: var(--vp-c-text-1); }

.lt-meta {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  font-family: var(--vp-font-family-mono);
  font-size: 13px;
  color: var(--vp-c-text-2);
  margin: 0;
}
.lt-tag {
  color: var(--vp-c-text-1);
  border: 1px solid var(--vp-c-divider);
  border-radius: 6px;
  padding: 1px 7px;
}
.lt-dot { color: var(--vp-c-text-3); }

/* subtle monochrome glow behind the hero */
.lt-glow {
  position: absolute;
  inset: -140px -80px auto auto;
  width: 620px;
  height: 460px;
  z-index: 0;
  background: radial-gradient(
    50% 50% at 70% 32%,
    color-mix(in srgb, var(--vp-c-text-1) 7%, transparent),
    transparent 70%
  );
  pointer-events: none;
}

/* ─────────────── terminal mock ─────────────── */
.lt-term {
  border: 1px solid var(--t-border);
  border-radius: 12px;
  background: var(--t-bg);
  overflow: hidden;
  box-shadow:
    0 1px 0 0 rgba(255, 255, 255, 0.04) inset,
    0 30px 60px -30px rgba(0, 0, 0, 0.7);
  font-family: var(--vp-font-family-mono);
  font-size: 12.5px;
  line-height: 1.72;
}
.lt-term__bar {
  display: flex;
  align-items: center;
  gap: 12px;
  height: 38px;
  padding: 0 14px;
  background: var(--t-surface);
  border-bottom: 1px solid var(--t-border);
}
.lt-term__dots { display: inline-flex; gap: 7px; }
.lt-term__dots i {
  width: 11px;
  height: 11px;
  border-radius: 50%;
  background: #2e2e2e;
}
.lt-term__path {
  color: var(--t-dim);
  font-size: 12px;
}

.lt-term__status {
  display: flex;
  align-items: center;
  gap: 14px;
  padding: 6px 14px;
  background: #060606;
  border-bottom: 1px solid var(--t-border);
  color: var(--t-dim);
  font-size: 11.5px;
}
.lt-st-name { color: var(--t-bright); font-weight: 600; }
.lt-st-ws b { color: var(--t-text); }
.lt-st-mode { margin-left: auto; color: var(--t-text); letter-spacing: 0.08em; }
.lt-st-leader {
  color: var(--t-text);
  border: 1px solid var(--t-border);
  border-radius: 4px;
  padding: 0 6px;
  font-size: 10.5px;
}

.lt-term__body {
  display: flex;
  gap: 10px;
  padding: 14px;
  color: var(--t-text);
  min-height: 232px;
}
.lt-col {
  flex: 1;
  min-width: 0;
  border: 1px solid var(--t-border);
  border-radius: 8px;
  padding: 10px 12px;
  background: #0a0a0a;
}
.lt-col--focus {
  border-color: rgba(255, 255, 255, 0.42);
  box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.12);
}
.lt-tile { padding-bottom: 8px; }
.lt-tile--b {
  margin-top: 8px;
  padding-top: 10px;
  border-top: 1px dashed var(--t-border);
}
.lt-line { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
.lt-dim { color: var(--t-dim); }
.lt-bright { color: var(--t-bright); }
.lt-ok { color: var(--t-bright); }
.lt-mark { color: var(--t-bright); margin-right: 7px; }
.lt-path { color: var(--t-dim); margin-right: 6px; }
.lt-prompt { color: var(--t-bright); margin-right: 6px; }

.lt-cursor {
  display: inline-block;
  width: 7px;
  height: 14px;
  vertical-align: -2px;
  background: var(--t-bright);
  animation: lt-blink 1.1s steps(2, start) infinite;
}
@keyframes lt-blink { 50% { opacity: 0; } }

.lt-bars {
  display: flex;
  flex-direction: column;
  gap: 5px;
  margin-top: 8px;
}
.lt-bars span {
  height: 6px;
  border-radius: 3px;
  width: var(--w);
  background: rgba(255, 255, 255, 0.55);
}

/* ─────────────── sections ─────────────── */
.lt-section {
  padding: 64px 0;
  border-top: 1px solid var(--vp-c-divider);
}
.lt-section__head { margin-bottom: 40px; }
.lt-h2 {
  font-size: 32px;
  line-height: 1.15;
  letter-spacing: -0.02em;
  font-weight: 700;
  margin: 0;
  color: var(--vp-c-text-1);
  max-width: 18em;
}

.lt-grid {
  display: grid;
  grid-template-columns: 1fr;
  gap: 1px;
  background: var(--vp-c-divider);
  border: 1px solid var(--vp-c-divider);
  border-radius: 14px;
  overflow: hidden;
}
.lt-card {
  background: var(--vp-c-bg);
  padding: 26px 24px;
  transition: background 0.15s ease;
}
.lt-card:hover { background: var(--vp-c-bg-soft); }
.lt-card__head {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 8px;
}
.lt-card__n {
  font-family: var(--vp-font-family-mono);
  font-size: 12px;
  color: var(--vp-c-text-3);
  letter-spacing: 0.1em;
}
.lt-card__tag {
  font-family: var(--vp-font-family-mono);
  font-size: 10px;
  letter-spacing: 0.08em;
  text-transform: uppercase;
  color: var(--vp-c-text-3);
  border: 1px solid var(--vp-c-divider);
  border-radius: 5px;
  padding: 1px 6px;
}
.lt-card__title {
  font-size: 16.5px;
  font-weight: 650;
  margin: 12px 0 8px;
  color: var(--vp-c-text-1);
  line-height: 1.3;
}
.lt-card__body {
  font-size: 14px;
  line-height: 1.6;
  color: var(--vp-c-text-2);
  margin: 0;
}

/* ─────────────── cta ─────────────── */
.lt-section--cta { padding-bottom: 90px; }
.lt-cta {
  display: grid;
  grid-template-columns: 1fr;
  gap: 40px;
  align-items: center;
}
.lt-term--code .lt-term__body--code {
  display: block;
  min-height: 0;
  padding: 16px;
  font-size: 13px;
  line-height: 2;
  color: var(--t-text);
}

/* ─────────────── responsive ─────────────── */
@media (min-width: 760px) {
  .lt-grid { grid-template-columns: repeat(2, 1fr); }
}
@media (min-width: 860px) {
  .lt-hero { padding: 96px 0 104px; }
  .lt-hero__grid { grid-template-columns: 1.02fr 0.98fr; gap: 56px; }
  .lt-title { font-size: 60px; }
  .lt-cta { grid-template-columns: 1fr 1fr; gap: 56px; }
}
@media (min-width: 1080px) {
  .lt-grid { grid-template-columns: repeat(4, 1fr); }
  .lt-title { font-size: 64px; }
}
</style>
