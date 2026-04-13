# CoreText Shaping 实现计划

## 目标

在 macOS 上用 CoreText 原生文本整形替换 rustybuzz，三平台各用最优方案：

- Linux: rustybuzz (HarfBuzz Rust 端口，现有)
- macOS: CoreText (CTTypesetter/CTLine/CTRun，新增)
- Windows: rustybuzz (现有，后续可换 DirectWrite)

## 架构分析

### 现有 shaping 公共接口

`TextShaper` 提供以下关键方法，被 `terminal/shaping.rs` 调用：

1. `face_set()` → `FaceSet<'_>` — 包含 primary/cjk/emoji 三个字体引用
2. `detect_ligatures_with_face(text, face, font_id)` → `Vec<Ligature>` — 连字检测
3. `shape_char_with_fallback(ch, faces)` → `Option<(glyph_id, font_id)>` — 单字符整形
4. `shape_grapheme_with_fallback(cluster, faces)` → `Option<(glyph_id, font_id)>` — 字素簇整形

调用方 `terminal/shaping.rs::precompute_row_shaping()` 流程：
1. 获取 `FaceSet`
2. 检测连字 → `detect_ligatures_with_face(text, faces.primary, faces.primary_id)`
3. 整形字素簇 → `shape_grapheme_with_fallback(cluster, faces)`
4. 整形单字符 → `shape_char_with_fallback(ch, faces)`

### 关键类型

```rust
// 需要平台化的类型
pub struct FaceSet<'a> {
    pub primary: &'a rustybuzz::Face<'a>,  // macOS 上需要改为 &'a CTFont
    pub primary_id: fontdb::ID,
    pub cjk: Option<&'a rustybuzz::Face<'a>>,
    pub cjk_id: Option<fontdb::ID>,
    pub emoji: Option<&'a rustybuzz::Face<'a>>,
    pub emoji_id: Option<fontdb::ID>,
}

// 不需要改的类型
pub struct Ligature { start_col, char_count, glyph_id, font_id }
```

## 实现方案

### 核心思路

将 `FaceSet` 中的 face 类型平台化：
- Linux/Windows: `&rustybuzz::Face<'a>`
- macOS: `&CTFont`

`TextShaper` 的内部状态也平台化：
- Linux/Windows: `CachedFace` (持有 `rustybuzz::Face<'static>`)
- macOS: `CTFont` (CoreText 字体对象)

公共 API 不变，调用方 (`terminal/shaping.rs`) 无需修改。

### Step 1: 平台化 `FaceSet`

修改 `shaper.rs` 中的 `FaceSet`：

```rust
pub struct FaceSet<'a> {
    #[cfg(not(target_os = "macos"))]
    pub primary: &'a rustybuzz::Face<'a>,
    #[cfg(target_os = "macos")]
    pub primary: &'a core_text::font::CTFont,

    pub primary_id: fontdb::ID,

    #[cfg(not(target_os = "macos"))]
    pub cjk: Option<&'a rustybuzz::Face<'a>>,
    #[cfg(target_os = "macos")]
    pub cjk: Option<&'a core_text::font::CTFont>,

    pub cjk_id: Option<fontdb::ID>,

    #[cfg(not(target_os = "macos"))]
    pub emoji: Option<&'a rustybuzz::Face<'a>>,
    #[cfg(target_os = "macos")]
    pub emoji: Option<&'a core_text::font::CTFont>,

    pub emoji_id: Option<fontdb::ID>,
}
```

### Step 2: 平台化 `TextShaper` 内部状态

macOS 上不需要 `CachedFace`（rustybuzz 专用），改为持有 `CTFont`：

```rust
// Linux/Windows
#[cfg(not(target_os = "macos"))]
primary_face: Option<CachedFace>,
#[cfg(not(target_os = "macos"))]
cjk_face: Option<CachedFace>,
#[cfg(not(target_os = "macos"))]
emoji_face: Option<CachedFace>,

// macOS
#[cfg(target_os = "macos")]
primary_ct_font: Option<CTFont>,
#[cfg(target_os = "macos")]
cjk_ct_font: Option<CTFont>,
#[cfg(target_os = "macos")]
emoji_ct_font: Option<CTFont>,
```

### Step 3: 创建 `shaper_coretext.rs`

新文件 `crates/ciri-render/src/shaper_coretext.rs`，实现 CoreText 整形核心函数：

#### 3.1 `ct_shape_char(font: &CTFont, ch: char) -> Option<u32>`

单字符整形：
1. 将 char 编码为 UTF-16
2. 创建 `CFString`
3. 创建 `CFAttributedString` 附加字体属性
4. `CTTypesetter::new()` → `CTLine` → 获取第一个 `CTRun`
5. 从 run 提取 glyph ID
6. 返回 glyph_id

#### 3.2 `ct_shape_grapheme(font: &CTFont, cluster: &str) -> Option<u32>`

字素簇整形（ZWJ 序列、组合字符等）：
1. 同上流程，但输入是多字符串
2. 如果输出的 glyph 数 >= 输入字符数，说明字体没有组合 → 返回 None
3. 否则返回第一个非零 glyph_id

#### 3.3 `ct_detect_ligatures(font: &CTFont, text: &str, font_id: fontdb::ID) -> Vec<Ligature>`

连字检测：
1. 创建 `CTTypesetter` → `CTLine`
2. 遍历所有 `CTRun`
3. 获取 glyph_count、string_indices (cluster 信息)
4. 对比 cluster 跨度：如果一个 glyph 覆盖了多个输入字符 → 连字
5. 构建 `Ligature` 列表

### Step 4: 修改 `shaper.rs` 整形方法

将以下方法的内部实现 cfg 分支：

#### `face_set()` → macOS 返回 CTFont 引用

```rust
#[cfg(target_os = "macos")]
pub fn face_set(&self) -> Option<FaceSet<'_>> {
    Some(FaceSet {
        primary: self.primary_ct_font.as_ref()?,
        primary_id: self.primary_font_id?,
        cjk: self.cjk_ct_font.as_ref(),
        cjk_id: self.cjk_font_id,
        emoji: self.emoji_ct_font.as_ref(),
        emoji_id: self.emoji_font_id,
    })
}
```

#### `shape_char_with_fallback()` → macOS 调用 `ct_shape_char`

```rust
// macOS 版
fn shape_single_char(&self, s: &str, font: &CTFont) -> Option<u32> {
    ct_shape_char(font, s.chars().next()?)
}
```

#### `shape_grapheme_with_face()` → macOS 调用 `ct_shape_grapheme`

#### `detect_ligatures_uncached()` → macOS 调用 `ct_detect_ligatures`

### Step 5: 修改 `TextShaper::new()` 初始化

macOS 分支：
1. 使用 fontdb 发现字体（不变）
2. 从字体路径创建 `CTFont` 对象（代替 rustybuzz Face）
3. 不需要 `CachedFace` 的 unsafe transmute

```rust
#[cfg(target_os = "macos")]
{
    shaper.primary_ct_font = primary_font_id
        .and_then(|fid| shaper.font_path(fid))
        .and_then(|(path, index)| load_ct_font(&path, index, 12.0));
    // ... 同理 cjk/emoji
}
```

注意：CTFont 的 size 参数对 shaping 不影响（shaping 只关心 glyph ID 和 cluster，不关心大小），但需要一个合理的默认值。

### Step 6: 修改 `terminal/shaping.rs` 的调用

`precompute_row_shaping` 中有一处直接引用 `faces.primary`：

```rust
// 第 50 行
shaper.detect_ligatures_with_face(run.text, faces.primary, faces.primary_id)
```

这里 `faces.primary` 的类型在 macOS 上变为 `&CTFont`。`detect_ligatures_with_face` 的签名也需要平台化。

### Step 7: 处理 Cargo.toml 依赖

`core-text` 和 `core-foundation` 已经在 macOS 依赖中（rasterizer 添加的）。
`rustybuzz` 可以改为 Linux/Windows 专用：

```toml
[target.'cfg(not(target_os = "macos"))'.dependencies]
rustybuzz = { workspace = true }
```

但这可能影响其他用到 rustybuzz 的地方。如果只有 shaper.rs 用它，就可以这么做。

## CoreText 整形管线详解（参照 Ghostty）

```
1. 输入 text (Rust &str)
2. 编码为 UTF-16 → CFString
3. 创建 CFAttributedString，设置 kCTFontAttributeName = CTFont
4. CTTypesetter::new(attributed_string)
5. CTTypesetter::create_line(range) → CTLine
6. CTLine::glyph_runs() → [CTRun]
7. 对每个 CTRun:
   - CTRun::glyph_count() → n
   - CTRun::glyphs() → [CGGlyph] (glyph IDs)
   - CTRun::string_indices() → [CFIndex] (cluster mapping)
   - CTRun::positions() → [CGPoint] (可选，连字检测不需要)
8. 从 glyphs + string_indices 提取 glyph_id 和 cluster 信息
```

### 关键 API 说明

- `CTTypesetter::create_line()`: 需要传 `CFRange`，`{0, 0}` 表示整个字符串
- `CTRun::string_indices()`: 返回每个 glyph 对应的原始字符串偏移（UTF-16 单位）
- 连字判断：如果相邻 glyph 的 string_index 跨度 > 1 个字符 → 连字

### BiDi 处理

Ghostty 强制 LTR（`kCTTypesetterOptionForcedEmbeddingLevel = 0`）。终端文本通常是 LTR，我们也应该这样做。

## 不需要改的部分

- `Ligature` 结构体 — 纯数据，平台无关
- 字体发现逻辑 (`find_primary_font`, `find_cjk_font`, `find_emoji_font`) — fontdb 跨平台
- `font_resolver` — 独立模块
- `FontData` — 字体原始数据缓存，macOS 上仍需要（用于 fontdb 和路径查找）
- `char_shape_cache`, `grapheme_shape_cache`, `ligature_cache` — 缓存层平台无关

## 验证方法

1. macOS: `cargo build` 编译通过
2. Linux: `cargo check` 编译通过
3. macOS: `cargo test` 通过
4. 在 Mac Mini 上运行 ciri，验证：
   - 普通英文字符正确渲染
   - 编程连字（Fira Code 等）正确显示
   - CJK 字符正确回退
   - Emoji（包括 ZWJ 序列）正确显示
