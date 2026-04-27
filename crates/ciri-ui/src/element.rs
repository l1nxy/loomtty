//! Core `Element` trait and the contexts passed through its lifecycle.
//!
//! Layout is driven by [`taffy`] — each element exposes a `taffy_style()`
//! and a flat `children()` list that the paint walker turns into a Taffy
//! tree before `compute_layout`. The walker then calls `paint(&self, cx)`
//! on each element with its computed bounds filled into `PaintCtx`.
//!
//! Paint-time inheritance — opacity, translate, text colour — is
//! propagated by the walker. Z-order falls out of paint sequence:
//! non-deferred elements emit in tree order, then queued
//! [`crate::elements::Deferred`] subtrees drain in ascending priority.
//! Containers that need to float above siblings (modals, tooltips,
//! anchored popovers) wrap themselves in `deferred(...)`.

use crate::arena::{ArenaBox, with_element_arena};
use crate::color::Color;
use crate::scene::Scene;
use crate::shaper::TextShaper;
use crate::theme::ResolvedTheme;
use std::ops::Deref;

/// Opaque identifier for a single element in the retained tree. The ID
/// space is generation-counted so reload / rebuild cycles don't clash.
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default)]
pub struct ElementId(pub u64);

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
/// `scene` via `push_sdf(rect)` (or `emit_text` for glyphs), and trust
/// that the walker has already added the inherited translate into
/// `bounds`. An element's own translate/opacity affect only self and
/// descendants — the walker passes them on via fresh inherited state.
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
    /// `hit_id` of the topmost element under the cursor for this paint
    /// pass, if any. The walker computes this from a pre-paint
    /// `LayoutSnapshot` walk (so paint sees it immediately, without a
    /// 1-frame lag) and threads it through every element's paint call.
    /// Elements check `cx.is_hovered(self_hit_id)` to apply hover
    /// styles; see [`PaintCtx::is_hovered`].
    pub hovered_hit_id: Option<u64>,
    /// Cross-frame state map. Elements that opt into persistence
    /// (typically by overriding [`Element::id`]) call
    /// `cx.states.use_state::<S>(id)` to borrow their slot. `None`
    /// when the host hasn't published one (legacy paint paths,
    /// minimal tests).
    pub states: Option<&'a mut ElementStates>,
}

impl<'a> PaintCtx<'a> {
    pub fn theme(&self) -> &ResolvedTheme {
        self.theme
    }

    /// True iff the topmost element under the cursor right now has the
    /// given `hit_id`. Use this to gate hover-state styling — only the
    /// receiver of pointer events should light up, not every ancestor
    /// that happens to contain the cursor.
    #[inline]
    pub fn is_hovered(&self, hit_id: u64) -> bool {
        self.hovered_hit_id == Some(hit_id)
    }

    /// Convenience: append an SDF rect to the scene in paint order.
    pub fn push_sdf(&mut self, rect: crate::scene::SdfRect) {
        self.scene.push_sdf(rect);
    }

    /// Convenience: shape `content` through the host shaper, folding
    /// `inherited_opacity` into the alpha so text fades with its
    /// animated wrapper. Glyphs land in the scene in emit order.
    pub fn emit_text(&mut self, content: &str, pos: [f32; 2], color: Color, font_size_px: f32) {
        let faded = [color[0], color[1], color[2], color[3] * self.inherited_opacity];
        self.text_shaper
            .emit(content, pos, faded, font_size_px, self.scene);
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

/// Anything that can be converted into an [`Element`]. Lets builders
/// like `Div::child(impl IntoElement)` accept strings, `Text`, `Div`,
/// or any future component without per-call wrapping.
///
/// Identity impls are provided for `Text` and `Div`. String types
/// (`&str`, `String`, `SharedString`) convert to a `Text` element.
pub trait IntoElement {
    type Element: Element;
    fn into_element(self) -> Self::Element;
}

/// Light-weight context passed to [`Render::render`]. Exposes the
/// theme and viewport that a view needs to compose its element tree.
/// More fields can be added without breaking the trait — `Render`
/// implementations only read what they need.
pub struct RenderCtx<'a> {
    pub theme: &'a ResolvedTheme,
    pub viewport: [f32; 2],
    pub scale: f32,
}

/// Implemented by stateful "view" types — long-lived structs that
/// hold their own state (filtered rows, scroll offset, hover index)
/// and produce a fresh element tree per frame from that state.
///
/// Distinct from [`Element`]: an `Element` *is* a tree node in this
/// frame's paint pass. A `Render` implementor is a host-side object
/// that knows how to *produce* an element tree on demand. The two
/// connect through [`IntoElement`] — `render()` returns anything that
/// can be coerced into an element, including a `Div` tree built up
/// via the Tailwind-shaped builder.
///
/// ```ignore
/// struct PaletteView { /* state */ }
/// impl Render for PaletteView {
///     fn render(&mut self, cx: &RenderCtx<'_>) -> impl IntoElement {
///         div().bg(cx.theme.surface).child(...)
///     }
/// }
/// ```
pub trait Render: 'static + Sized {
    fn render(&mut self, cx: &RenderCtx<'_>) -> impl IntoElement;
}

/// Cross-frame persistent state map keyed by `(ElementId, TypeId)`.
///
/// Elements that need state across paints (scroll offsets, virtual
/// list anchors, animation timers) call
/// [`ElementStates::use_state`] in their paint method passing their
/// own [`ElementId`] and the state type. The first call in a frame
/// inserts a `Default` instance; subsequent calls in subsequent
/// frames return the same instance for mutation.
///
/// ciri-ui does **not** GC unused entries automatically — callers
/// that recycle ids (palette opens/closes) should drop entries
/// explicitly via [`ElementStates::clear_id`] when an element goes
/// away. For the single-window chrome use case the map is bounded
/// and stays small.
pub struct ElementStates {
    map: std::collections::HashMap<(ElementId, std::any::TypeId), Box<dyn std::any::Any>>,
}

impl Default for ElementStates {
    fn default() -> Self {
        Self {
            map: std::collections::HashMap::new(),
        }
    }
}

impl ElementStates {
    pub fn new() -> Self {
        Self::default()
    }

    /// Borrow (creating with `Default` if missing) the state slot for
    /// `(id, S)`. Same call across frames returns the same `&mut S`,
    /// so elements can persist scroll offsets, hover timers, etc.
    pub fn use_state<S: 'static + Default>(&mut self, id: ElementId) -> &mut S {
        let key = (id, std::any::TypeId::of::<S>());
        let slot = self
            .map
            .entry(key)
            .or_insert_with(|| Box::new(S::default()) as Box<dyn std::any::Any>);
        slot.downcast_mut::<S>()
            .expect("element_states slot type mismatch — same id reused with different type")
    }

    /// Drop every state slot belonging to `id`, regardless of state
    /// type. Call from the host when an element with this id is
    /// guaranteed not to render any more (component closes, modal
    /// dismissed, etc.).
    pub fn clear_id(&mut self, id: ElementId) {
        self.map.retain(|(eid, _), _| *eid != id);
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }
}

/// Type-erased, arena-allocated element. Returned by
/// [`AnyElement::new`] and stored as a `Div`'s child slot. Acts like a
/// `Box<dyn Element>` for the rest of the framework — derefs to
/// `dyn Element`, dropped when the arena is cleared.
///
/// Construction goes through the active element arena (the per-frame
/// bump allocator). The active arena is published by an
/// [`crate::arena::ElementArenaScope`] in the host's paint path; in
/// tests, the thread-local fallback arena is used.
pub struct AnyElement(ArenaBox<dyn Element>);

impl AnyElement {
    pub fn new<E: Element>(element: E) -> Self {
        let boxed = with_element_arena(|arena| arena.alloc(|| element));
        AnyElement(boxed.map(|el| el as &mut dyn Element))
    }
}

impl Deref for AnyElement {
    type Target = dyn Element;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        &*self.0
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
    /// can zip them together. Children are arena-allocated; the slice
    /// is borrowed from the parent's storage and lives as long as the
    /// element does.
    fn children(&self) -> &[AnyElement] {
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

    /// Optional stable identifier for this element. When set, it scopes
    /// the element's slot in [`ElementStates`] so persistent state
    /// (scroll offset, hover timer, virtual list anchor) survives across
    /// frames. The walker exposes the id via `cx.element_id` during
    /// paint. Default: `None` — most chrome elements are stateless.
    fn id(&self) -> Option<ElementId> {
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

    /// Same as [`Element::text_color_override`] but receives the current
    /// frame's `hovered_hit_id` so refinement-only colour overrides
    /// (e.g. `.hover(|s| s.text_color(fg))`) can propagate to
    /// descendants. Default impl falls back to the stateless variant —
    /// only `Div` (which has refinement support) needs to override.
    /// Called by the walker AFTER `paint`, so it sees the same
    /// effective style the painter used.
    fn text_color_override_with_state(
        &self,
        _hovered_hit_id: Option<u64>,
    ) -> Option<crate::color::Color> {
        self.text_color_override()
    }

    /// Paint this element using `cx.bounds`. Children paint themselves
    /// via the walker; `paint` only emits primitives for `self`.
    fn paint(&self, cx: &mut PaintCtx<'_>);

    /// True if `(x, y)` (screen px) lies within the element's interactive area.
    fn hit_test(&self, x: f32, y: f32, bounds: [f32; 4]) -> bool {
        x >= bounds[0] && x < bounds[0] + bounds[2] && y >= bounds[1] && y < bounds[1] + bounds[3]
    }

    /// Whether this element should be considered by pointer hit-testing.
    ///
    /// Layout snapshots include all painted nodes for diagnostics, but event
    /// dispatch should only target elements that opted into pointer behavior
    /// (click/hover/cursor). Containers override this when their style carries
    /// handlers. Text defaults to non-interactive.
    fn accepts_pointer_events(&self) -> bool {
        false
    }

    /// Optional host-defined hit identifier for layout snapshots.
    fn hit_id(&self) -> Option<u64> {
        None
    }

    /// Marker: `true` when this element wants the walker to defer paint
    /// of its single child to *after* the main tree walk completes.
    /// Used by [`crate::elements::Deferred`] to escape z-order without
    /// the fixed `Layer` enum: deferred children are sorted by
    /// `deferred_priority()` ascending and painted in order, so they
    /// always sit above non-deferred siblings regardless of tree order.
    ///
    /// Walker contract: when this returns `true`, the wrapper itself
    /// does not paint — neither `paint()` nor a snapshot push runs for
    /// it, and the zero-size guard is bypassed. Each child is captured
    /// into the deferred queue with the current inheritance state and
    /// re-painted during drain. Authors that need a scrim alongside a
    /// deferred subtree should emit it from a sibling element, not
    /// from inside the deferred wrapper.
    fn is_deferred(&self) -> bool {
        false
    }

    /// Drain priority for deferred elements. Lower values paint earlier
    /// (i.e. further from the viewer). Read by the walker only when
    /// [`Element::is_deferred`] returns `true`. Default 0.
    fn deferred_priority(&self) -> u32 {
        0
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
    fn element_states_use_state_creates_default_then_persists() {
        #[derive(Default)]
        struct S {
            counter: u32,
        }
        let mut states = ElementStates::new();
        let id = ElementId(42);
        {
            let s = states.use_state::<S>(id);
            assert_eq!(s.counter, 0);
            s.counter = 5;
        }
        let s = states.use_state::<S>(id);
        assert_eq!(s.counter, 5, "state should persist across calls with same id");
    }

    #[test]
    fn element_states_isolates_by_type() {
        #[derive(Default)]
        struct A {
            v: u32,
        }
        #[derive(Default)]
        struct B {
            v: u32,
        }
        let mut states = ElementStates::new();
        states.use_state::<A>(ElementId(1)).v = 7;
        states.use_state::<B>(ElementId(1)).v = 11;
        assert_eq!(states.use_state::<A>(ElementId(1)).v, 7);
        assert_eq!(states.use_state::<B>(ElementId(1)).v, 11);
    }

    #[test]
    fn render_trait_produces_element() {
        use crate::elements::div;
        use crate::styled::Styled;
        struct ButtonView {
            label_count: u32,
        }
        impl Render for ButtonView {
            fn render(&mut self, _cx: &RenderCtx<'_>) -> impl IntoElement {
                self.label_count += 1;
                div().w(100.0).h(40.0)
            }
        }
        let theme = ResolvedTheme::default();
        let mut view = ButtonView { label_count: 0 };
        let cx = RenderCtx {
            theme: &theme,
            viewport: [800.0, 600.0],
            scale: 1.0,
        };
        // The trait method returns `impl IntoElement`. We exercise the
        // call to confirm the trait shape compiles and the view's
        // mutable state is reachable from inside `render`.
        let _ = view.render(&cx);
        assert_eq!(view.label_count, 1);
    }

    #[test]
    fn element_states_clear_id_drops_only_that_id() {
        #[derive(Default)]
        struct S {
            v: u32,
        }
        let mut states = ElementStates::new();
        states.use_state::<S>(ElementId(1)).v = 1;
        states.use_state::<S>(ElementId(2)).v = 2;
        states.clear_id(ElementId(1));
        // Id 1's slot is dropped → next use_state re-creates with default.
        assert_eq!(states.use_state::<S>(ElementId(1)).v, 0);
        // Id 2's slot is intact.
        assert_eq!(states.use_state::<S>(ElementId(2)).v, 2);
    }

    #[test]
    fn default_hit_test_is_aabb() {
        let d = Dummy;
        assert!(d.hit_test(5.0, 5.0, [0.0, 0.0, 10.0, 10.0]));
        assert!(!d.hit_test(15.0, 5.0, [0.0, 0.0, 10.0, 10.0]));
    }

    #[test]
    fn default_paint_transform_is_identity() {
        assert_eq!(Dummy.paint_transform(), (1.0, [0.0, 0.0]));
    }
}
