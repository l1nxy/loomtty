//! Core `Element` trait and the contexts passed through its lifecycle.
//!
//! Layout is driven by [`taffy`] — each element exposes a `taffy_style()`
//! and a flat `children()` list that the paint walker turns into a Taffy
//! tree before `compute_layout`. The walker then calls `paint(&self, cx)`
//! on each element with its computed bounds filled into `PaintCtx`.
//!
//! Paint-time inheritance — opacity, translate and layer — is propagated
//! by the walker. Elements can therefore compose transforms across parent
//! boundaries cleanly: a fading/sliding wrapper visibly affects every
//! descendant, and `in_layer(Modal)` wins z-order for its entire subtree.

use crate::color::Color;
use crate::scene::Scene;
use crate::shaper::TextShaper;
use crate::theme::ResolvedTheme;

/// Opaque identifier for a single element in the retained tree. The ID
/// space is generation-counted so reload / rebuild cycles don't clash.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default)]
pub struct ElementId(pub u64);

/// Z-order layers, in paint order (lowest drawn first, highest on top).
/// Hit-testing walks the inverse order so modals win over chrome.
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug)]
#[repr(u8)]
pub enum Layer {
    /// Permanent chrome: top bar, tab bar, hints bar, borders.
    Chrome = 0,
    /// Plugin sidebars / dock panels.
    Sidebar = 1,
    /// Non-modal overlays: connection banner, inline hints.
    Overlay = 2,
    /// Modal UIs: palette, paste dialog, context menu.
    Modal = 3,
    /// Short-lived hints that never receive pointer events.
    Tooltip = 4,
}

impl Default for Layer {
    fn default() -> Self {
        Self::Chrome
    }
}

/// Input events routed to elements by the dispatch tree.
#[derive(Clone, Debug)]
pub enum UiEvent {
    PointerDown { x: f32, y: f32 },
    PointerUp { x: f32, y: f32 },
    PointerMove { x: f32, y: f32 },
    Scroll { dx: f32, dy: f32 },
    FocusGained,
    FocusLost,
}

/// Read-only context threaded through layout + paint. Holds the theme, the
/// viewport, and scaling info — things every element wants and no element
/// should mutate.
pub struct UiCtx<'a> {
    pub theme: &'a ResolvedTheme,
    pub viewport: [f32; 2],
    pub scale: f32,
}

impl<'a> UiCtx<'a> {
    pub fn new(theme: &'a ResolvedTheme, viewport: [f32; 2], scale: f32) -> Self {
        Self {
            theme,
            viewport,
            scale,
        }
    }
}

/// Paint-time context. Carries the element's laid-out bounds, the scene
/// accumulator, and the paint-time transforms the walker has inherited
/// from ancestors.
///
/// Elements fold `inherited_opacity` into their own opacity, emit into
/// `scene` via `push_sdf(layer, ...)` using `layer`, and trust that the
/// walker has already added the inherited translate into `bounds`. An
/// element's own translate/opacity affect only self and descendants —
/// the walker passes them on via fresh inherited state.
pub struct PaintCtx<'a> {
    pub theme: &'a ResolvedTheme,
    /// `[x, y, w, h]` in logical pixels. Already includes the inherited
    /// paint-time translate from ancestors; the element only needs to
    /// add its own `translate` (if any) when emitting its primitive.
    pub bounds: [f32; 4],
    pub scene: &'a mut Scene,
    /// Text shaper for glyph measurement + emission. Provided by the
    /// host (ciri-app wraps `UiTextShaper` + `GlyphCache`); tests use
    /// [`crate::shaper::NullShaper`].
    pub text_shaper: &'a mut dyn TextShaper,
    pub scale: f32,
    /// Stable element identity for event dispatch / plugin hosts. `None`
    /// when the walker hasn't assigned an ID (current scaffolding state);
    /// making this explicit keeps consumers from treating a zero value
    /// as a real identity.
    pub element_id: Option<ElementId>,
    /// Cumulative opacity from the walker. Multiply this with the
    /// element's own `opacity` before emitting.
    pub inherited_opacity: f32,
    /// Inherited text colour from the nearest ancestor whose style set
    /// `text_color`. `None` = fall back to `theme.on_surface`. Elements
    /// that render text consult this (own color → inherited → theme).
    pub inherited_text_color: Option<Color>,
    /// Effective layer for the emitted primitive. Already resolved by
    /// the walker (own `in_layer` → parent's inherited layer → default).
    pub layer: Layer,
}

impl<'a> PaintCtx<'a> {
    pub fn theme(&self) -> &ResolvedTheme {
        self.theme
    }

    /// Convenience: push an SDF rect into the effective layer.
    pub fn push_sdf(&mut self, rect: crate::scene::SdfRect) {
        self.scene.push_sdf(self.layer, rect);
    }

    /// Convenience: shape `content` through the host shaper into the
    /// current layer. Folds in `inherited_opacity` on the alpha channel
    /// so text fades with its wrapper.
    pub fn emit_text(&mut self, content: &str, pos: [f32; 2], color: Color, font_size_px: f32) {
        let faded = [color[0], color[1], color[2], color[3] * self.inherited_opacity];
        self.text_shaper
            .emit(content, pos, faded, font_size_px, self.layer, self.scene);
    }
}

/// Event-dispatch context. Elements call `request_redraw()` when their
/// state changes. Renderer PR wires this into the winit event loop.
pub struct EventCtx {
    redraw: bool,
}

impl EventCtx {
    pub fn new() -> Self {
        Self { redraw: false }
    }

    pub fn request_redraw(&mut self) {
        self.redraw = true;
    }

    pub fn needs_redraw(&self) -> bool {
        self.redraw
    }
}

impl Default for EventCtx {
    fn default() -> Self {
        Self::new()
    }
}

/// The core trait. Everything in the retained tree is an `Element`.
///
/// Default bodies are intentionally no-ops so leaves like `Text` can
/// implement only `paint`.
pub trait Element: 'static {
    /// Layout style for this element as a Taffy `Style`. Called by the
    /// paint walker when it builds the Taffy tree.
    fn taffy_style(&self) -> taffy::Style {
        taffy::Style::default()
    }

    /// Direct children in paint order. The Taffy tree's children are
    /// built from this slice in the same order so the post-layout walker
    /// can zip them together.
    fn children(&self) -> &[Box<dyn Element>] {
        &[]
    }

    /// Taffy node context for layout-time measurement.
    ///
    /// Leaves that need shaper-driven sizing (Text, eventually Image)
    /// return `Some(NodeContext::Text { .. })` so the paint walker's
    /// `compute_layout_with_measure` callback can ask the host shaper
    /// for an exact `[w, h]`. Returning `None` (the default) makes
    /// Taffy honour `taffy_style().size`.
    fn taffy_context(&self) -> Option<crate::layout::NodeContext> {
        None
    }

    /// Optional z-layer override for this element and its descendants.
    /// Returning `None` means "inherit from parent" — the walker tracks
    /// the running inherited layer and only concrete overrides change it.
    ///
    /// This differs from the pre-review signature (which returned a
    /// default `Chrome` concretely) — that made `in_layer(Modal)`
    /// effectively an every-child no-op, since default descendants would
    /// silently reset back to `Chrome`.
    fn layer(&self) -> Option<Layer> {
        None
    }

    /// Stable type identifier (used by plugin hosts + debug logs).
    fn type_id(&self) -> &'static str;

    /// Paint-time transform this element contributes to its descendants.
    /// `(opacity_multiplier, translate_delta)` — the walker multiplies /
    /// adds these into the inherited state when recursing into children.
    ///
    /// Default: identity (does nothing). Containers that support
    /// animated styles override this to return their own current opacity
    /// and translate so transitions propagate through the subtree.
    fn paint_transform(&self) -> (f32, [f32; 2]) {
        (1.0, [0.0, 0.0])
    }

    /// Text colour this element wants to impose on its descendants, if
    /// any. Returning `Some(c)` makes the walker thread `c` through the
    /// subtree as the inherited text colour; returning `None` keeps the
    /// ancestor's value. Div overrides to return its `style.text_color`.
    fn text_color_override(&self) -> Option<crate::color::Color> {
        None
    }

    /// Paint this element using `cx.bounds`. Children paint themselves
    /// via the walker; `paint` only emits primitives for `self`.
    fn paint(&self, cx: &mut PaintCtx<'_>);

    /// True if `(x, y)` (screen px) lies within the element's interactive area.
    fn hit_test(&self, x: f32, y: f32, bounds: [f32; 4]) -> bool {
        x >= bounds[0] && x < bounds[0] + bounds[2] && y >= bounds[1] && y < bounds[1] + bounds[3]
    }

    /// Handle an input event. Return `true` if consumed. Default: ignore.
    fn on_event(&mut self, _event: &UiEvent, _cx: &mut EventCtx) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Dummy;
    impl Element for Dummy {
        fn type_id(&self) -> &'static str {
            "dummy"
        }
        fn paint(&self, _cx: &mut PaintCtx<'_>) {}
    }

    #[test]
    fn default_layer_is_inherit() {
        // Default (`None`) means "inherit from parent"; the walker starts
        // the root at Chrome and threads that through.
        assert_eq!(Dummy.layer(), None);
    }

    #[test]
    fn default_hit_test_is_aabb() {
        let d = Dummy;
        assert!(d.hit_test(5.0, 5.0, [0.0, 0.0, 10.0, 10.0]));
        assert!(!d.hit_test(15.0, 5.0, [0.0, 0.0, 10.0, 10.0]));
    }

    #[test]
    fn layer_order() {
        assert!(Layer::Chrome < Layer::Overlay);
        assert!(Layer::Overlay < Layer::Modal);
        assert!(Layer::Modal < Layer::Tooltip);
    }

    #[test]
    fn default_paint_transform_is_identity() {
        assert_eq!(Dummy.paint_transform(), (1.0, [0.0, 0.0]));
    }
}
