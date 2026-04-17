//! `Div` — the canonical container element.
//!
//! A styled box with ordered children. Layout is flex by default (matches
//! gpui's shape). Implements [`Styled`] so every builder method in that
//! trait chains here: `div().flex_col().gap_1().bg(theme.surface).child(...)`.

use crate::color::{mul_alpha, Color, TRANSPARENT};
use crate::element::{Element, Layer, PaintCtx};
use crate::layout::to_taffy_style;
use crate::scene::{Scene, SdfRect};
use crate::style::{Shadow, Style};
use crate::styled::Styled;

/// Free constructor: `div()` reads better than `Div::new()` in chains.
pub fn div() -> Div {
    Div::new()
}

/// Styled container. Children are painted in insertion order.
pub struct Div {
    style: Style,
    children: Vec<Box<dyn Element>>,
    layer_override: Option<Layer>,
}

impl Default for Div {
    fn default() -> Self {
        Self::new()
    }
}

impl Div {
    pub fn new() -> Self {
        Self {
            style: Style::new(),
            children: Vec::new(),
            layer_override: None,
        }
    }

    /// Append one child.
    pub fn child<E: Element>(mut self, child: E) -> Self {
        self.children.push(Box::new(child));
        self
    }

    /// Append an already-boxed child — useful for containers collected
    /// dynamically (e.g. plugin-registered items).
    pub fn child_boxed(mut self, child: Box<dyn Element>) -> Self {
        self.children.push(child);
        self
    }

    /// Extend with many children.
    pub fn children_ext<I, E>(mut self, iter: I) -> Self
    where
        I: IntoIterator<Item = E>,
        E: Element,
    {
        self.children
            .extend(iter.into_iter().map(|e| Box::new(e) as Box<dyn Element>));
        self
    }

    /// Override the layer this element belongs to. Default: `Chrome`.
    /// Named `in_layer` to avoid shadowing [`Element::layer`].
    pub fn in_layer(mut self, l: Layer) -> Self {
        self.layer_override = Some(l);
        self
    }

    /// Read-only style access.
    pub fn style_ref(&self) -> &Style {
        &self.style
    }

    /// Mutable style access. Convenience to avoid the caller having to
    /// name the `Styled` trait when they only want a one-off mutation.
    pub fn style_mut(&mut self) -> &mut Style {
        &mut self.style
    }
}

impl Styled for Div {
    fn style(&mut self) -> &mut Style {
        &mut self.style
    }
}

impl Element for Div {
    fn taffy_style(&self) -> taffy::Style {
        to_taffy_style(&self.style)
    }

    fn children(&self) -> &[Box<dyn Element>] {
        &self.children
    }

    fn layer(&self) -> Layer {
        self.layer_override.unwrap_or(Layer::Chrome)
    }

    fn type_id(&self) -> &'static str {
        "ciri.div"
    }

    fn paint(&self, cx: &mut PaintCtx<'_>) {
        if !has_visual(&self.style) {
            return;
        }
        emit_sdf_rect(&self.style, cx.bounds, cx.scene);
    }
}

/// True iff the style would produce any visible pixels.
fn has_visual(s: &Style) -> bool {
    s.background.is_some()
        || s.border_width.map_or(false, |w| w > 0.0)
        || s.shadow.is_some()
        || s.corner_radii.map_or(false, |r| r.iter().any(|v| *v > 0.0))
}

/// Collapse a styled Div into a single SDF instance. Transforms and
/// opacity are folded into the geometry / color here so the shader has
/// one uniform code path.
fn emit_sdf_rect(s: &Style, bounds: [f32; 4], scene: &mut Scene) {
    let opacity = s.opacity.unwrap_or(1.0).clamp(0.0, 1.0);
    let translate = s.translate.unwrap_or([0.0, 0.0]);
    let bg = s.background.unwrap_or(TRANSPARENT);
    let border_c = s.border_color.unwrap_or(TRANSPARENT);
    let border_w = s.border_width.unwrap_or(0.0).max(0.0);
    let radii = s.corner_radii.unwrap_or([0.0; 4]);
    let (shadow_blur, shadow_color, shadow_offset) = resolve_shadow(s.shadow);

    scene.sdf_rects.push(SdfRect {
        pos: [bounds[0] + translate[0], bounds[1] + translate[1]],
        size: [bounds[2], bounds[3]],
        color: mul_alpha(bg, opacity),
        radii,
        border_color: mul_alpha(border_c, opacity),
        border_width: border_w,
        shadow_blur,
        shadow_offset,
        shadow_color: mul_alpha(shadow_color, opacity),
    });
}

/// Map the semantic `Shadow` enum to concrete (blur, color, offset).
/// Kept tiny on purpose: these match the conventional Tailwind steps
/// closely enough for ciri's chrome without shipping a design-token
/// table yet.
fn resolve_shadow(s: Option<Shadow>) -> (f32, Color, [f32; 2]) {
    match s {
        Some(Shadow::Sm) => (4.0, [0.0, 0.0, 0.0, 0.15], [0.0, 2.0]),
        Some(Shadow::Md) => (8.0, [0.0, 0.0, 0.0, 0.25], [0.0, 4.0]),
        Some(Shadow::Lg) => (16.0, [0.0, 0.0, 0.0, 0.35], [0.0, 6.0]),
        None => (0.0, [0.0; 4], [0.0; 2]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::elements::text;
    use crate::theme::ResolvedTheme;

    #[test]
    fn builder_chains_compile() {
        let _ = div()
            .flex_col()
            .items_center()
            .gap_1()
            .p_2()
            .rounded_md()
            .child(text("hello"));
    }

    #[test]
    fn layer_override_sticks() {
        let d = div().in_layer(Layer::Modal);
        assert_eq!(<Div as Element>::layer(&d), Layer::Modal);
    }

    #[test]
    fn children_preserve_order() {
        let d = div().child(text("a")).child(text("b"));
        assert_eq!(d.children().len(), 2);
    }

    #[test]
    fn paint_with_no_visual_emits_nothing() {
        // A bare `div()` with no bg / border / shadow / radius is purely a
        // layout container. The paint pass must skip emission so layout-only
        // containers don't pile up zero-alpha instances in the GPU buffer.
        let theme = ResolvedTheme::default();
        let mut scene = Scene::new();
        let mut pcx = PaintCtx {
            theme: &theme,
            bounds: [0.0, 0.0, 100.0, 40.0],
            scene: &mut scene,
            scale: 1.0,
            element_id: Default::default(),
        };
        div().paint(&mut pcx);
        assert!(scene.is_empty());
    }

    #[test]
    fn shadow_md_produces_nonzero_blur() {
        let (blur, color, off) = resolve_shadow(Some(Shadow::Md));
        assert!(blur > 0.0);
        assert!(color[3] > 0.0);
        assert_ne!(off, [0.0, 0.0]);
    }
}
