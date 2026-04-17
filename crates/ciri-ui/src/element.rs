//! Core `Element` trait and the contexts passed through its lifecycle.
//!
//! This is the foundation surface; the concrete Taffy + GPU integrations
//! that actually fill in `UiCtx` / `PaintCtx` / `EventCtx` land with the
//! renderer wiring in a follow-up change. The traits below are stable and
//! safe to implement now.

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
///
/// Defined here (not in a separate `event.rs`) so the `Element` trait can
/// reference it without a circular import in the foundation crate.
#[derive(Clone, Debug)]
pub enum UiEvent {
    PointerDown {
        x: f32,
        y: f32,
    },
    PointerUp {
        x: f32,
        y: f32,
    },
    PointerMove {
        x: f32,
        y: f32,
    },
    Scroll {
        dx: f32,
        dy: f32,
    },
    FocusGained,
    FocusLost,
}

/// Read-only context threaded through layout + paint. Holds the theme, the
/// viewport, and scaling info — things every element wants and no element
/// should mutate.
///
/// Renderer integration (PR-3) fills in the `'a` lifetime holders with real
/// references into the app; today this is a placeholder so the `Element`
/// trait is stable.
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

/// Paint-time context. Carries the laid-out bounds plus whatever scene
/// accumulators the renderer chooses to expose (SDF rect vec, glyph vec,
/// clip stack, …). This version holds only bounds; the renderer PR extends
/// it with the real emit methods.
pub struct PaintCtx<'a> {
    pub ui: UiCtx<'a>,
    pub bounds: [f32; 4],
    pub element_id: ElementId,
}

impl<'a> PaintCtx<'a> {
    pub fn new(ui: UiCtx<'a>, bounds: [f32; 4], element_id: ElementId) -> Self {
        Self {
            ui,
            bounds,
            element_id,
        }
    }

    pub fn theme(&self) -> &ResolvedTheme {
        self.ui.theme
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

/// The core trait. Everything in the retained tree is an `Element` —
/// builtin widgets, containers, plugin-authored nodes, legacy `UiComponent`
/// shims.
///
/// The default method bodies are intentionally no-ops so leaf elements like
/// `Text` can implement only `paint`.
pub trait Element: 'static {
    /// Which layer this element belongs to. Dispatch and paint order.
    fn layer(&self) -> Layer {
        Layer::Chrome
    }

    /// Stable type identifier (used by plugin hosts + debug logs).
    fn type_id(&self) -> &'static str;

    /// Paint this element using its laid-out `cx.bounds`.
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
}
