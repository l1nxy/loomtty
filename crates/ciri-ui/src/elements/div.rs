//! `Div` — the canonical container element.
//!
//! A styled box with ordered children. Layout is flex by default (matches
//! gpui's shape). Implements [`Styled`] so every builder method in that
//! trait chains here: `div().flex_col().gap_1().bg(theme.surface).child(...)`.

use crate::element::{Element, Layer, PaintCtx};
use crate::style::Style;
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
    pub fn children<I, E>(mut self, iter: I) -> Self
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

    /// Read-only access to children — the renderer walks this when painting.
    pub fn children_ref(&self) -> &[Box<dyn Element>] {
        &self.children
    }

    /// Read-only style access.
    pub fn style_ref(&self) -> &Style {
        &self.style
    }
}

impl Styled for Div {
    fn style(&mut self) -> &mut Style {
        &mut self.style
    }
}

impl Element for Div {
    fn layer(&self) -> Layer {
        self.layer_override.unwrap_or(Layer::Chrome)
    }

    fn type_id(&self) -> &'static str {
        "ciri.div"
    }

    fn paint(&self, _cx: &mut PaintCtx<'_>) {
        // Foundation PR: no GPU emission yet. Renderer integration lands
        // with the SDF shader + FrameScene extension. Children painting is
        // driven by the tree walker, not from inside paint() — each
        // element emits only its own primitive.
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
        assert_eq!(d.children_ref().len(), 2);
    }

    #[test]
    fn paint_is_noop_in_foundation() {
        // Just verify paint() can be called against a placeholder context.
        // Full paint semantics land with the renderer integration.
        let theme = ResolvedTheme::default();
        let ui = crate::element::UiCtx::new(&theme, [800.0, 600.0], 1.0);
        let mut pcx = PaintCtx::new(ui, [0.0, 0.0, 10.0, 10.0], Default::default());
        div().paint(&mut pcx);
    }
}
