# Plan: shaper-based UI text layout (allow non-monospace UI fonts)

## Goal

让 UI 层（palette、tab bar、status bar、context menu、info_box、hints_bar、overview action bar、paste dialog 等所有非 terminal 文本）使用真正的 text shaper（rustybuzz / CoreText）来排版，而非"按字符迭代 + col×cell_w"的等宽假设。这样：

1. UI 字体可以是 **任意比例字体**（Inter、SF Pro、Segoe UI 等），不再被强制等宽。
2. 等宽字体下，连字 / kerning 也能正确显示（FiraCode 的 `==>`、`!=` 等）。
3. UI 字体可以独立于 terminal 字体配置。

## Non-goals

- 不改 terminal 渲染路径 — terminal cell 仍是网格 + 现有 shaper 流程，保持行为不变。
- 不引入新的 shaper / 字体后端 — 复用 `crates/ciri-render/src/shaper.rs`（rustybuzz）和 `crates/ciri-render/src/shaper_coretext.rs`（macOS）。
- 不做 RTL / vertical script / 复杂 BIDI（UI 文案以拉丁 + CJK 为主）。

## 现状（必读）

UI 文本路径：

- 入口：`crates/ciri/src/app/ui/builder.rs`
  - `abs_text(text, x, y, color)` → 调 `emit_status_text`
  - `text_width(text) -> f32` → `UnicodeWidthStr::width(text) as f32 * cell_w`
  - `row_height() -> f32` → `cell_h`
- 实际 glyph 生成：`crates/ciri/src/app/status_bar.rs`
  - `emit_status_text` 按 `text.chars()` 迭代，`col` 累加 `UnicodeWidthChar::width(ch)`，glyph 位置 = `x_start + col * cell_width`
  - `atlas.ensure_char(ch)` 一个 char → 一个 glyph（按 codepoint，不经 shaping）
- 调用方（grep `abs_text|text_width`）：palette、tab_bar、top_bar、context_menu、hints_bar、info_box、overview、paste_dialog、status bar 等，分布广泛。

Terminal 路径（参考用，**不要改**）：

- `crates/ciri-render/src/shaper.rs::TextShaper` — `shape_*` 方法返回 `Vec<ShapedGlyph>{glyph_id, x_advance, ...}`
- 已有按 cluster 反向映射 col 的逻辑（terminal 用）。

字体配置：`crates/ciri-config/src/schema.rs::FontConfig { family, size }`，全局只一份。

## 设计

### 1. 配置：UI 字体独立

在 `FontConfig` 上加可选 UI 字体：

```toml
[font]
family = "Iosevka"
size = 11.0

[font.ui]               # 可选；不写则继承 terminal 字体
family = "Inter"
size = 11.0
```

实现：
- `FontConfig` 增加 `pub ui: Option<UiFontConfig>` 字段，`UiFontConfig { family, size }`。
- 不强制 monospace。
- 缺省 `ui = None` 时复用 `family` / `size`（行为不变）。

### 2. 新增 `UiTextShaper`

新文件 `crates/ciri-render/src/ui_shaper.rs`（或挂在现有 shaper 模块下）：

```rust
pub struct UiTextShaper {
    // 内部：rustybuzz Face + cmap，平台等价物
    // pixel_size: f32
}

pub struct UiShapedGlyph {
    pub glyph_id: u32,
    pub x_advance: f32,   // 像素
    pub x_offset: f32,
    pub y_offset: f32,
    pub cluster: usize,   // 字节 offset，用于 hit-test
}

impl UiTextShaper {
    pub fn new(font_path: &str, face_index: u32, pixel_size: f32) -> Self;
    pub fn shape(&mut self, text: &str) -> Vec<UiShapedGlyph>;
    pub fn measure(&mut self, text: &str) -> f32;            // sum of x_advance
    pub fn line_height(&self) -> f32;                         // ascent + descent + line gap
    pub fn ascent(&self) -> f32;
}
```

- shape 结果带 cluster（字节 offset），后面截断 / hit-test 用。
- 内部缓存最近 N 条 shape 结果（LRU，比如 256 条），UI 文本重复率高，避免每帧都 shape。

### 3. UI 字体 atlas 与 glyph

复用现有 `GlyphCache`：

- `FontInitParams` 增加 `ui_font_path: Option<(String, u32)>` + `ui_pixel_size`。
- `GlyphCache::ensure_glyph_id(glyph_id, FontKind::Ui)` — 按 glyph_id 而非 char 缓存（新分支）。
- terminal / CJK / emoji 路径不动；UI 走单独的 face + atlas 池（或共用 atlas 但 key 加 FontKind 维度）。
- key：`(FontKind, glyph_id, pixel_size_bucket)`，避免和 terminal 字符冲突。

### 4. 改 `emit_status_text` 路径

`crates/ciri/src/app/status_bar.rs::emit_status_text` 重写为：

```rust
pub fn emit_status_text(
    atlas: &mut GlyphCache,
    shaper: &mut UiTextShaper,
    text: &str,
    params: &TextEmitParams,
    glyphs: &mut Vec<GlyphInstance>,
    color_glyphs: &mut Vec<GlyphInstance>,
) {
    let shaped = shaper.shape(text);
    let mut pen_x = params.x_start;
    for g in shaped {
        if let Some(entry) = atlas.ensure_glyph_id(g.glyph_id) {
            // emit instance at (pen_x + x_offset + bearing, baseline + y_offset - bearing_y)
        }
        pen_x += g.x_advance;
    }
}
```

`UiBuilder` 持有（或借引用） `UiTextShaper`：

- `UiContext` 增加 `ui_shaper: &mut UiTextShaper` + `ui_cell_h: f32`（line_height）。
- `text_width(s)` → `shaper.measure(s)`。
- `row_height()` → `ui_shaper.line_height()`。

### 5. Hit-test / 截断 / 光标

凡是用过 `cell_w` × col 算位置的 UI 代码（palette truncate_label、cursor_x = text_width(...)、context menu、tab bar 宽度），全改为：

- 截断：用 `shape(text)`，按累计 `x_advance` 找到适合 panel_w 的最长前缀（按 cluster 字节边界切），在切点后追加 "..."。
- 光标：cursor_x = pen_x_at_byte(query.len())，用 shape 结果 cluster 反查。
- 行高：用 `ui_shaper.line_height()` 替换 `cx.cell_h`（或保留 `cx.cell_h` 含义为 ui line_height）。

### 6. 兼容性

- `[font.ui]` 缺省 → UiTextShaper 用 terminal 字体 + size，行为接近现状（但走 shaper，会启用 ligature / kerning，可能微调位置）。
- 文档更新：`config.toml` 注释加 `[font.ui]` 说明。

## 涉及文件清单

**新增**
- `crates/ciri-render/src/ui_shaper.rs`（或在 `shaper.rs` 内加 `UiTextShaper`）

**修改**（结构性）
- `crates/ciri-config/src/schema.rs` — `FontConfig` 加 ui
- `crates/ciri-render/src/glyph_cache/mod.rs` — UI face 加载、`ensure_glyph_id` 接口
- `crates/ciri-render/src/glyph_cache/rasterize.rs` — 按 glyph_id rasterize（FreeType 已支持 `Face::load_glyph(gid)`)
- `crates/ciri-render/src/glyph_cache/rasterize_dwrite.rs` 同上
- `crates/ciri-render/src/glyph_cache/rasterize_coretext.rs` 同上
- `crates/ciri/src/app/status_bar.rs::emit_status_text` 重写
- `crates/ciri/src/app/ui/builder.rs` — `text_width`/`abs_text`/`row_height` 走 shaper；`UiContext` 加 `ui_shaper`、`ui_cell_h`
- `crates/ciri/src/app/ui/types.rs::UiContext`（如有）
- 渲染主循环：把 `UiTextShaper` 实例 wire 进 UiContext

**改 hit-test/截断**
- `crates/ciri/src/app/ui/palette.rs` — `truncate_label`、cursor 位置
- `crates/ciri/src/app/ui/context_menu.rs`
- `crates/ciri/src/app/ui/tab_bar/mod.rs`
- `crates/ciri/src/app/ui/top_bar/*.rs`
- `crates/ciri/src/app/ui/info_box.rs`
- `crates/ciri/src/app/ui/hints_bar.rs`
- `crates/ciri/src/app/ui/paste_dialog.rs`
- `crates/ciri/src/app/ui/overview.rs`

## 验收 (DoD)

1. `cargo build --workspace` / `cargo clippy --workspace -- -D warnings` 干净。
2. `cargo test --workspace` 全过；新增覆盖：
   - `UiTextShaper::measure` 返回值 = 各 glyph advance 之和。
   - 截断函数：等宽字体下与旧 `truncate_label` 行为一致；非等宽字体下截断点不超过 panel_w。
3. 目视：
   - 不配 `[font.ui]` 时，UI 渲染与改前像素级近似（允许 ligature/kerning 引起的微小位移）。
   - 配 `[font.ui] family = "Inter"` 时，palette 文字按真实 advance 排列，不会重叠；命令面板 query / cursor 跟随 query 真实宽度。
   - 配 `family = "FiraCode Nerd Font"`（terminal）+ `[font.ui] family = "Inter"` 时，terminal 仍是等宽 + ligature，UI 是 Inter。
4. Hit-test 正确：palette 鼠标点击落在哪个文字上 = 视觉上看到的那行，无 ±1 行错位。
5. macOS / Windows / Linux 三平台路径都编译通过（macOS、Windows 由 CI 兜底，Linux 本地实测）。

## 风险

- shaper 调用每帧若不缓存会拖慢渲染：必须实现 LRU 缓存。
- atlas key 冲突：terminal char-key 与 UI glyph-id-key 必须分维度，否则会渲染错。
- emoji / CJK：UI 字体一般不含，需要 fallback —— 复用现有 emoji/cjk 字体路径但改成按 glyph_id。短期可只支持 BMP 拉丁 + CJK 落到现有 CJK 字体；emoji 在 UI 文案里少见，先不处理。
- macOS CoreText shaping 接口与 rustybuzz 不同：`UiTextShaper` 用 trait + 平台 impl 隔离。

## 步骤（建议执行顺序）

1. 加 `[font.ui]` 配置 + UiTextShaper 骨架（rustybuzz 实现，先不接 UI）。
2. 改 `GlyphCache` 支持 `ensure_glyph_id` + UI face。
3. 改 `emit_status_text` 走 shaper；先验证 status bar / palette 显示正确。
4. 把 `text_width` / `truncate_label` / cursor_x 全部切到 advance-based。
5. 跑全部 UI 路径的 hit-test，修边界。
6. macOS / Windows shaper impl。
7. clippy / test / 目视验收。

## 仓库约定

- 用 jj，不用 git。
- 当前分支 `feat/ui-shaper`（已切）。
- 提交：每个步骤一个 jj change，描述清楚。
- 不要新增 `*.md` 文档（除本计划文件本身），不要写解释性注释 — WHY 注释只在不显然处加。
