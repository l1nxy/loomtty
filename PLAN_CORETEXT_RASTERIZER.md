# CoreText Rasterizer 实现计划

## 目标

为 ciri 的 glyph_cache 模块添加 macOS 原生 CoreText/Core Graphics 光栅化后端，替代当前 macOS 上使用的 crossfont+FreeType 路径。实现后三个平台各用最优方案：

- Linux: FreeType + fontconfig (现有，`#[cfg(target_os = "linux")]`)
- macOS: CoreText + Core Graphics (新增，`#[cfg(target_os = "macos")]`)
- Windows: DirectWrite (现有，`#[cfg(windows)]`)

## 架构参考

参照 Ghostty 的 `src/font/face/coretext.zig` 实现，使用 `core-text` 和 `core-graphics` Rust crate。

## 现有架构分析

### 当前平台分裂方式
所有代码使用 `#[cfg(not(windows))]` vs `#[cfg(windows)]`。macOS 和 Linux 共用同一套 crossfont+FreeType。

### 需要改动的文件和模块

1. **`glyph_cache/mod.rs`** — GlyphCache 结构体的平台字段和初始化
2. **`glyph_cache/rasterize.rs`** — 光栅化函数的 cfg 条件
3. **`glyph_cache/metrics.rs`** — 字体度量计算（当前 FreeType 专用）
4. **`glyph_cache/cjk.rs`** — CJK 字体大小调整（当前 FreeType 专用）
5. **`glyph_cache/types.rs`** — FontKeySet（当前 crossfont 专用）
6. **`Cargo.toml`** — 添加 macOS 依赖

### 关键接口（不变）
- `RasterizedGlyph` — 光栅化输出格式，所有平台共用
- `cache_rasterized_glyph()` — atlas 分配 + pending upload 队列，平台无关
- `GlyphEntry` — atlas UV 坐标 + bearing，平台无关
- `PendingUpload` — GPU 上传数据，平台无关

## 实现步骤

### Step 1: 修改 Cargo.toml

在 `crates/ciri-render/Cargo.toml` 添加 macOS 专用依赖：

```toml
[target.'cfg(target_os = "macos")'.dependencies]
core-text = "21"
core-graphics = "0.24"
core-foundation = "0.10"
```

### Step 2: 创建 `rasterize_coretext.rs`

新文件 `crates/ciri-render/src/glyph_cache/rasterize_coretext.rs`，实现：

#### 2.1 CoreTextRasterizer 结构体

```rust
pub(crate) struct CoreTextRasterizer {
    /// 主字体的 4 种样式变体 (Regular/Bold/Italic/BoldItalic)
    primary_fonts: [CTFont; 4],
    /// Emoji 字体（通常只有 Regular）
    emoji_font: Option<CTFont>,
    /// CJK 字体
    cjk_font: Option<CTFont>,
    /// Emoji 是否是颜色字体 (sbix/COLR)
    emoji_is_color: bool,
}
```

#### 2.2 字体加载 — `CoreTextRasterizer::new()`

- 接收字体路径+face index 或 family name
- 用 `CTFont::new_from_descriptor()` 或从文件数据创建
- 为 Bold/Italic/BoldItalic 创建变体（优先查系统变体，fallback 用合成）
- 检测 emoji 字体是否为颜色字体（检查 sbix 表）

#### 2.3 字符光栅化 — `rasterize_char()`

对应 `ensure_styled_char()` 中的 crossfont 路径：

1. 用 `CTFont::get_glyphs_for_characters()` 获取 glyph index
2. 用 `CTFont::get_bounding_rects_for_glyphs()` 获取边界框
3. 创建 `CGBitmapContext`：
   - 灰度文字：`CGColorSpace::create_device_gray()`，`kCGImageAlphaOnly` 格式
   - 彩色 emoji：`CGColorSpace::create_with_name(kCGColorSpaceDisplayP3)`，premultiplied RGBA
4. 设置抗锯齿：`context.set_should_antialias(true)`，`set_should_smooth_fonts(true)`
5. 调用 `CTFont::draw_glyphs()` 渲染到 bitmap context
6. 提取像素数据，转为 `RasterizedGlyph`

#### 2.4 Glyph-ID 光栅化 — `rasterize_glyph_id()`

对应 `ensure_glyph_id()` 中的 FreeType thin path：

- 与 `rasterize_char()` 类似，但直接传入 glyph index 而非字符
- 对 emoji 尝试颜色渲染
- 合成粗体：设置 `CGContext` text drawing mode 为 `FillStroke`，stroke width 按字体大小比例
- 合成斜体：应用 `CGAffineTransform` 倾斜矩阵 (tan(14°) ≈ 0.25)

#### 2.5 颜色字体检测 — `is_color_glyph()`

- 检查字体是否有 sbix 表（`CTFont::copy_table(b"sbix")`）
- 有 sbix 表则所有 glyph 视为颜色 glyph（Apple emoji 都是 sbix）

### Step 3: 创建 `metrics_coretext.rs`

新文件 `crates/ciri-render/src/glyph_cache/metrics_coretext.rs`，实现：

#### 3.1 `compute_ct_metrics()`

对应 `compute_ft_metrics()`，计算 cell_width, cell_height, ascent, face_width：

1. 从 CTFont 获取 ascent/descent/leading
2. 使用和 FreeType 版本相同的 ghostty 方法：`round()` 而非 `ceil()`
3. 垂直居中：将 line_gap 平分到上下
4. 测量所有可打印 ASCII 字符的最大 advance 获取 cell_width

### Step 4: 创建 `cjk_coretext.rs`

新文件 `crates/ciri-render/src/glyph_cache/cjk_coretext.rs`，实现：

#### 4.1 `compute_cjk_pixel_size_ct()`

对应 `compute_cjk_pixel_size()`，使用 CTFont API：

1. 测量 "水" 字符在主字体和 CJK 字体中的 advance
2. 归一化到 em 单位
3. 计算缩放系数使 CJK 的 ic_width 比例匹配主字体

### Step 5: 修改 `glyph_cache/mod.rs`

将 `#[cfg(not(windows))]` 拆分为 `#[cfg(target_os = "linux")]` 和 `#[cfg(target_os = "macos")]`：

#### 5.1 模块声明

```rust
#[cfg(target_os = "linux")]
mod cjk;
#[cfg(target_os = "linux")]
mod metrics;
#[cfg(target_os = "macos")]
mod cjk_coretext;
#[cfg(target_os = "macos")]
mod metrics_coretext;
#[cfg(target_os = "macos")]
mod rasterize_coretext;
```

#### 5.2 GlyphCache 结构体字段

添加 macOS 专用字段：

```rust
#[cfg(target_os = "macos")]
coretext: CoreTextRasterizer,
```

移除 macOS 上不需要的 crossfont/FreeType 字段（将 `#[cfg(not(windows))]` 改为 `#[cfg(target_os = "linux")]`）。

#### 5.3 GlyphCache::new() 初始化

添加 `#[cfg(target_os = "macos")]` 分支：
- 创建 CoreTextRasterizer
- 调用 `compute_ct_metrics()` 获取度量
- 调用 `compute_cjk_pixel_size_ct()` 获取 CJK 像素大小

#### 5.4 ensure_styled_char() 和 ensure_glyph_id()

添加 `#[cfg(target_os = "macos")]` 分支，调用 CoreTextRasterizer 的方法。

### Step 6: 修改 `glyph_cache/rasterize.rs`

将 `#[cfg(not(windows))]` 条件改为 `#[cfg(target_os = "linux")]`：
- `rasterize_glyph_id_ft()` → `#[cfg(target_os = "linux")]`
- `convert_crossfont_glyph()` → `#[cfg(target_os = "linux")]`
- `downsample_rgba()` — 检查是否 macOS 也需要（emoji 缩放），如果需要则保持 `#[cfg(not(windows))]`

### Step 7: 修改 `glyph_cache/types.rs`

将 `FontKeySet` 的 cfg 改为 `#[cfg(target_os = "linux")]`（crossfont FontKey 仅 Linux 使用）。

### Step 8: 修改 `glyph_cache/metrics.rs` 和 `cjk.rs`

将这两个文件的 cfg guard 从 `#[cfg(not(windows))]` 改为 `#[cfg(target_os = "linux")]`。

## 关键技术细节

### Core Graphics 光栅化管线（参照 Ghostty）

```
1. CTFont::get_glyphs_for_characters() → glyph indices
2. CTFont::get_bounding_rects_for_glyphs() → 边界框 (注意: CG 坐标系 y 轴向上)
3. CGBitmapContext::create(width + padding, height + padding)
   - 灰度: CGImageAlphaOnly, DeviceGray, 1 byte/pixel
   - 彩色: PremultipliedFirst, DisplayP3/SRGB, 4 bytes/pixel
4. context.set_should_antialias(true)
   context.set_should_smooth_fonts(true)  // macOS font smoothing
   context.set_allows_font_subpixel_positioning(true)
   context.set_allows_font_subpixel_quantization(false)  // 精确定位
5. CTFont::draw_glyphs(&[glyph], &[position], context)
6. 提取 context.data() 像素数据
```

### 合成粗体和斜体

- **合成粗体**: CGContext fill+stroke 模式，stroke width = font_size * 0.03 (Ghostty 用法)
- **合成斜体**: CGAffineTransform(1, 0, tan(14°), 1, 0, 0)，14° 是 Ghostty 的选择

### 颜色字体 BGRA → RGBA

Core Graphics 输出 premultiplied BGRA，需要转换为 RGBA：
- 交换 B 和 R 通道
- 保持预乘 alpha（shader 侧处理）

### 坐标系转换

Core Graphics 坐标系 y 轴向上（数学坐标系），bearing_y 需要从 CG 的 bounds.origin.y + bounds.size.height 计算，转换为"从基线向上"的偏移。

## 不需要改的部分

- `atlas.rs` — atlas 分配器，完全平台无关
- `cache_rasterized_glyph()` — 已经是通用的
- `RasterizedGlyph` 结构体 — 通用输出格式
- `GlyphEntry`, `GlyphInstance`, `ScissoredRange` — 渲染侧类型
- `font_resolver/` — 字体回退逻辑（后续独立任务）
- `shaper.rs` — 文本整形（后续独立任务）
- GPU 后端代码 — 只消费 PendingUpload，不关心光栅化来源

## 验证方法

1. 在 Mac Mini 上 `cargo build` 编译通过
2. 在 Linux 上 `cargo build` 编译通过（不能破坏现有功能）
3. 在 Mac Mini 上 `cargo test` 通过
4. Windows 交叉编译不受影响（`cargo check --target x86_64-pc-windows-msvc` 如果工具链可用）

## 注意事项

- `core-text`, `core-graphics`, `core-foundation` crate 的版本要与 Cargo.lock 中已有的其他 crate 依赖兼容
- `downsample_rgba()` 函数可能 macOS 也需要（emoji 缩放），判断后决定 cfg
- macOS 不使用 FreeType hinting（macOS 哲学是不 hint），光栅化时不需要 hinting 相关逻辑
- 测试要覆盖 RGB→alpha 转换、BGRA→RGBA 转换、空 glyph 处理
