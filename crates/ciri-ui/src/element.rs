//! Core `Element` trait and the contexts passed through its lifecycle.
//!
//! Layout is driven by [`taffy`] — each element exposes a `taffy_style()`
//! and a flat `children()` list that the paint walker turns into a Taffy
//! tree before `compute_layout`. The walker then calls `paint(&self, cx)`
//! on each element with its computed bounds filled into `PaintCtx`.

use crate::scene::Scene;
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

/// Paint-time context. Carries the element's laid-out bounds plus the
/// scene accumulator the paint pass emits into.
pub struct PaintCtx<'a> {
    pub theme: &'a ResolvedTheme,
    /// `[x, y, w, h]` in logical (device-independent) pixels, already
    /// resolved by the paint walker from the element's Taffy layout.
    pub bounds: [f32; 4],
    pub scene: &'a mut Scene,
    pub scale: f32,
    pub element_id: ElementId,
}

impl<'a> PaintCtx<'a> {
    pub fn theme(&self) -> &ResolvedTheme {
        self.theme
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
/// The default bodies are intentionally no-ops so leaves like `Text` can
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

    /// Which layer this element belongs to. Dispatch and paint order.
    fn layer(&self) -> Layer {
        Layer::Chrome
    }

    /// Stable type identifier (used by plugin hosts + debug logs).
    fn type_id(&self) -> &'static str;

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
    fn default_layer_is_chrome() {
        assert_eq!(Dummy.layer(), Layer::Chrome);
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
    fn default_taffy_style_and_children() {
        let d = Dummy;
        assert_eq!(d.children().len(), 0);
        // Taffy's Default is Block layout — paint walker interprets it as
        // "no flex container", perfectly valid for leaves.
        let _ = d.taffy_style();
    }
}
