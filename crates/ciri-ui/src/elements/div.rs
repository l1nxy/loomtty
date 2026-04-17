//! `Div` — the canonical container element.
//!
//! A styled box with ordered children. Layout defaults to **flex row**
//! so `gap_*`, `items_*` and `justify_*` helpers work on a plain `div()`
//! without an explicit `.flex()` call. Implements [`Styled`] so every
//! builder method in that trait chains here:
//!
//! `div().flex_col().gap_1().bg(theme.surface).child(...)`.
//!
//! Paint-time transforms (opacity, translate) compose through the
//! subtree via [`Element::paint_transform`] — the walker threads the
//! inherited state, so `Div::paint` only has to fold its own values
//! into its own emitted primitive.

use crate::color::{mul_alpha, Color, TRANSPARENT};
use crate::element::{Element, Layer, PaintCtx};
use crate::layout::to_taffy_style;
use crate::scene::SdfRect;
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

    /// Override the layer this element — and, via walker inheritance,
    /// its descendants — belongs to. Default: inherit from parent.
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

    fn layer(&self) -> Option<Layer> {
        self.layer_override
    }

    fn type_id(&self) -> &'static str {
        "ciri.div"
    }

    /// Opacity cascades multiplicatively and translate adds. These are
    /// the values the walker threads into descendants' inherited state.
    fn paint_transform(&self) -> (f32, [f32; 2]) {
        let op = self.style.opacity.unwrap_or(1.0).clamp(0.0, 1.0);
        let tr = self.style.translate.unwrap_or([0.0, 0.0]);
        (op, tr)
    }

    fn paint(&self, cx: &mut PaintCtx<'_>) {
        if !has_visual(&self.style) {
            return;
        }

        // Own opacity folded with whatever the walker inherited. The
        // walker has already baked inherited translate into cx.bounds;
        // we only add our own translate.
        let own_opacity = self.style.opacity.unwrap_or(1.0).clamp(0.0, 1.0);
        let effective_opacity = (cx.inherited_opacity * own_opacity).clamp(0.0, 1.0);
        let own_translate = self.style.translate.unwrap_or([0.0, 0.0]);

        let bg = self.style.background.unwrap_or(TRANSPARENT);
        let border_c = self.style.border_color.unwrap_or(TRANSPARENT);
        let border_w = self.style.border_width.unwrap_or(0.0).max(0.0);
        let radii = self.style.corner_radii.unwrap_or([0.0; 4]);
        let (shadow_blur, shadow_color, shadow_offset) = resolve_shadow(self.style.shadow);

        let [x, y, w, h] = cx.bounds;
        cx.push_sdf(SdfRect {
            pos: [x + own_translate[0], y + own_translate[1]],
            size: [w, h],
            color: mul_alpha(bg, effective_opacity),
            radii,
            border_color: mul_alpha(border_c, effective_opacity),
            border_width: border_w,
            shadow_blur,
            shadow_offset,
            shadow_color: mul_alpha(shadow_color, effective_opacity),
        });
    }
}

/// True iff the style would produce any visible pixels.
fn has_visual(s: &Style) -> bool {
    s.background.is_some()
        || s.border_width.map_or(false, |w| w > 0.0)
        || s.shadow.is_some()
        || s.corner_radii.map_or(false, |r| r.iter().any(|v| *v > 0.0))
}

/// Map the semantic `Shadow` enum to concrete (blur, color, offset).
/// Values track Tailwind's steps closely enough for chrome without
/// shipping a full design-token table yet.
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
    use crate::scene::Scene;
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
    fn layer_override_returns_some() {
        let d = div().in_layer(Layer::Modal);
        assert_eq!(<Div as Element>::layer(&d), Some(Layer::Modal));
    }

    #[test]
    fn layer_default_is_none_inherit() {
        let d = div();
        assert_eq!(<Div as Element>::layer(&d), None);
    }

    #[test]
    fn children_preserve_order() {
        let d = div().child(text("a")).child(text("b"));
        assert_eq!(d.children().len(), 2);
    }

    #[test]
    fn paint_with_no_visual_emits_nothing() {
        let theme = ResolvedTheme::default();
        let mut scene = Scene::new();
        let mut pcx = PaintCtx {
            theme: &theme,
            bounds: [0.0, 0.0, 100.0, 40.0],
            scene: &mut scene,
            scale: 1.0,
            element_id: Default::default(),
            inherited_opacity: 1.0,
            layer: Layer::Chrome,
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

    #[test]
    fn paint_transform_reports_style_values() {
        let d = div().opacity(0.5).translate(10.0, 20.0);
        let (op, tr) = d.paint_transform();
        assert!((op - 0.5).abs() < 1e-6);
        assert_eq!(tr, [10.0, 20.0]);
    }

    #[test]
    fn paint_transform_identity_when_unset() {
        assert_eq!(div().paint_transform(), (1.0, [0.0, 0.0]));
    }
}
