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
use crate::element::{AnyElement, Element, EventCtx, IntoElement, Layer, PaintCtx, UiEvent};
use crate::layout::to_taffy_style;
use crate::scene::SdfRect;
use crate::style::{Shadow, Style};
use crate::styled::Styled;
use smallvec::SmallVec;

/// Free constructor: `div()` reads better than `Div::new()` in chains.
pub fn div() -> Div {
    Div::new()
}

/// Styled container. Children are painted in insertion order.
///
/// Backed by a `SmallVec` with inline storage for the first 2 children:
/// most divs in chrome are leaves (1 text child) or wrapper rows (2
/// children — icon + label, prefix + value, etc.), so the children
/// vector lives entirely on the stack and never touches the allocator.
/// Containers with >2 children spill to a heap Vec transparently.
///
/// Each child is an [`AnyElement`] — bump-allocated in the active
/// element arena rather than `Box::new`'d. `Vec` of `AnyElement` still
/// pays one allocation when it spills past 2 children, but that's a
/// single 24-byte node header per spill (vs ~150 `Box::new`s for a
/// palette frame previously).
pub struct Div {
    style: Style,
    /// Optional refinement applied on top of `style` when the cursor is
    /// hovering over this element (i.e. when `cx.is_hovered(hit_id)`).
    /// Built by `.hover(|s| s.bg(...))`. Boxed because most divs don't
    /// have a hover style and we don't want to pay for a fat Style on
    /// every Div.
    hover_style: Option<Box<Style>>,
    children: SmallVec<[AnyElement; 2]>,
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
            hover_style: None,
            children: SmallVec::new(),
            layer_override: None,
        }
    }

    /// Apply a refinement on top of the base style when the cursor is
    /// hovering this element. The refinement is built by mutating a
    /// fresh `Style` in `f`; only fields the closure sets to `Some`
    /// will override. Requires the element to have a `hit_id` set
    /// (via [`Styled::hit_id`]) — without one, the framework can't
    /// identify "this element is hovered".
    ///
    /// ```ignore
    /// div().bg(theme.surface).hover(|s| s.bg(theme.accent_tint))
    /// ```
    pub fn hover(mut self, f: impl FnOnce(Style) -> Style) -> Self {
        let refinement = f(Style::new());
        self.hover_style = Some(Box::new(refinement));
        self
    }

    /// Append one child. Accepts anything convertible to an element —
    /// strings (`&str` / `String` / `SharedString`) become `Text`,
    /// existing elements pass through unchanged. Allocation goes to
    /// the active element arena (see [`crate::arena`]).
    ///
    /// `div().child("Foo")` and `div().child(text("Foo"))` are
    /// interchangeable; the former skips one wrap.
    pub fn child<C: IntoElement>(mut self, child: C) -> Self {
        self.children.push(AnyElement::new(child.into_element()));
        self
    }

    /// Append a pre-built [`AnyElement`] — useful when constructing a
    /// child via a code path that already owns the arena slot.
    pub fn child_any(mut self, child: AnyElement) -> Self {
        self.children.push(child);
        self
    }

    /// Extend with many children. Each yielded value is converted via
    /// [`IntoElement`] and arena-allocated.
    pub fn children_ext<I, C>(mut self, iter: I) -> Self
    where
        I: IntoIterator<Item = C>,
        C: IntoElement,
    {
        self.children
            .extend(iter.into_iter().map(|e| AnyElement::new(e.into_element())));
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

impl IntoElement for Div {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for Div {
    fn taffy_style(&self) -> taffy::Style {
        to_taffy_style(&self.style)
    }

    fn children(&self) -> &[AnyElement] {
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

    fn text_color_override(&self) -> Option<Color> {
        self.style.text_color
    }

    fn accepts_pointer_events(&self) -> bool {
        self.style.on_click.is_some()
            || self.style.on_hover.is_some()
            || self.style.cursor.is_some()
            || self.style.hit_id.is_some()
    }

    fn hit_id(&self) -> Option<u64> {
        self.style.hit_id
    }

    /// Fire stored click / hover handlers. Host code is responsible for
    /// hit-testing before calling this — we only see an event if it was
    /// already routed here. `on_hover(false)` must be delivered via
    /// `FocusLost` by the host's dispatch walker; this element doesn't
    /// track its own enter/leave state.
    fn on_event(&mut self, event: &UiEvent, cx: &mut EventCtx) -> bool {
        match event {
            UiEvent::PointerDown { .. } => {
                if let Some(cb) = self.style.on_click.clone() {
                    cb();
                    cx.request_redraw();
                    return true;
                }
            }
            UiEvent::PointerMove { .. } => {
                if let Some(cb) = self.style.on_hover.clone() {
                    cb(true);
                    cx.request_redraw();
                    return true;
                }
            }
            UiEvent::FocusLost => {
                if let Some(cb) = self.style.on_hover.clone() {
                    cb(false);
                    cx.request_redraw();
                }
            }
            _ => {}
        }
        false
    }

    fn paint(&self, cx: &mut PaintCtx<'_>) {
        // Resolve the effective style by overlaying refinements on top
        // of the base. Today only the hover refinement is applied; the
        // shape leaves room for `.active`, `.focus`, etc. Cloning
        // happens only when a refinement is active and matches state.
        let effective = self.effective_style(cx);

        if !has_visual(&effective) {
            return;
        }

        // Own opacity folded with whatever the walker inherited. The
        // walker has already baked inherited translate into cx.bounds;
        // we only add our own translate.
        let own_opacity = effective.opacity.unwrap_or(1.0).clamp(0.0, 1.0);
        let effective_opacity = (cx.inherited_opacity * own_opacity).clamp(0.0, 1.0);
        let own_translate = effective.translate.unwrap_or([0.0, 0.0]);

        let bg = effective.background.unwrap_or(TRANSPARENT);
        let border_c = effective.border_color.unwrap_or(TRANSPARENT);
        let border_w = effective.border_width.unwrap_or(0.0).max(0.0);
        let (shadow_blur, shadow_color, shadow_offset) = resolve_shadow(effective.shadow);

        let [x, y, w, h] = cx.bounds;
        // The rounded-box SDF is only well-defined when every radius is
        // ≤ half the shorter side. Without this, `.rounded_full()`
        // (radii = [9999.0; 4]) on a non-square box produces a positive
        // distance even at the centre and the fill vanishes. Clamp so
        // semantic sugar like "full = capsule" behaves correctly.
        let max_r = (w.min(h)).max(0.0) * 0.5;
        let raw_radii = effective.corner_radii.unwrap_or([0.0; 4]);
        let radii = [
            raw_radii[0].clamp(0.0, max_r),
            raw_radii[1].clamp(0.0, max_r),
            raw_radii[2].clamp(0.0, max_r),
            raw_radii[3].clamp(0.0, max_r),
        ];
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

impl Div {
    /// Compute the style that should drive paint on this frame: the
    /// base, with `hover_style` merged over top when the cursor is
    /// over this element. Returns a borrowed reference when no
    /// refinement applies (the common case) so we don't pay a Style
    /// clone for every static-styled element.
    fn effective_style<'a>(&'a self, cx: &PaintCtx<'_>) -> std::borrow::Cow<'a, Style> {
        let hovered = self
            .style
            .hit_id
            .is_some_and(|id| cx.is_hovered(id))
            && self.hover_style.is_some();
        if hovered {
            let mut merged = self.style.clone();
            if let Some(hov) = &self.hover_style {
                merged.merge(hov);
            }
            std::borrow::Cow::Owned(merged)
        } else {
            std::borrow::Cow::Borrowed(&self.style)
        }
    }
}

/// True iff the style would produce any visible pixels. Corner radii alone
/// don't emit anything — they only shape an existing fill/border/shadow — so
/// they aren't part of this check (a bare `div().rounded_md()` with no fill
/// would otherwise upload a no-op transparent SdfRect every frame).
fn has_visual(s: &Style) -> bool {
    s.background.is_some()
        || s.border_width.map_or(false, |w| w > 0.0)
        || s.shadow.is_some()
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

    fn make_pcx<'a>(
        theme: &'a ResolvedTheme,
        scene: &'a mut Scene,
        shaper: &'a mut crate::shaper::NullShaper,
        bounds: [f32; 4],
    ) -> PaintCtx<'a> {
        PaintCtx {
            theme,
            bounds,
            scene,
            text_shaper: shaper,
            scale: 1.0,
            element_id: None,
            inherited_opacity: 1.0,
            inherited_text_color: None,
            layer: Layer::Chrome,
            hovered_hit_id: None,
            states: None,
        }
    }

    #[test]
    fn paint_with_no_visual_emits_nothing() {
        let theme = ResolvedTheme::default();
        let mut scene = Scene::new();
        let mut shaper = crate::shaper::NullShaper;
        let mut pcx = make_pcx(&theme, &mut scene, &mut shaper, [0.0, 0.0, 100.0, 40.0]);
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

    #[test]
    fn hover_style_overlays_when_hit_id_matches() {
        let theme = ResolvedTheme::default();
        let mut scene = Scene::new();
        let mut shaper = crate::shaper::NullShaper;
        let mut pcx = make_pcx(&theme, &mut scene, &mut shaper, [0.0, 0.0, 100.0, 40.0]);
        pcx.hovered_hit_id = Some(7);
        div()
            .hit_id(7)
            .bg([1.0, 0.0, 0.0, 1.0])
            .hover(|s| s.bg([0.0, 1.0, 0.0, 1.0]))
            .paint(&mut pcx);
        let r = &scene.sdf_in_layer(Layer::Chrome)[0];
        assert_eq!(r.color, [0.0, 1.0, 0.0, 1.0]);
    }

    #[test]
    fn hover_style_skipped_when_not_hovered() {
        let theme = ResolvedTheme::default();
        let mut scene = Scene::new();
        let mut shaper = crate::shaper::NullShaper;
        let mut pcx = make_pcx(&theme, &mut scene, &mut shaper, [0.0, 0.0, 100.0, 40.0]);
        pcx.hovered_hit_id = Some(99); // different element
        div()
            .hit_id(7)
            .bg([1.0, 0.0, 0.0, 1.0])
            .hover(|s| s.bg([0.0, 1.0, 0.0, 1.0]))
            .paint(&mut pcx);
        let r = &scene.sdf_in_layer(Layer::Chrome)[0];
        assert_eq!(r.color, [1.0, 0.0, 0.0, 1.0]);
    }

    /// Regression for Codex P2: `.rounded_full()` sets radii to 9999 as
    /// sugar for "capsule". Before the clamp, the SDF saw radii larger
    /// than half the box and the fill disappeared. Each radius must
    /// clamp down to min(w, h) / 2 before the GPU instance is uploaded.
    #[test]
    fn rounded_full_clamps_to_capsule_radius() {
        let theme = ResolvedTheme::default();
        let mut scene = Scene::new();
        let mut shaper = crate::shaper::NullShaper;
        // 200 × 40 pill: max radius = 20.
        let mut pcx = make_pcx(&theme, &mut scene, &mut shaper, [0.0, 0.0, 200.0, 40.0]);
        div().bg([1.0, 0.0, 0.0, 1.0]).rounded_full().paint(&mut pcx);
        let r = &scene.sdf_in_layer(Layer::Chrome)[0];
        for c in r.radii {
            assert!(
                (c - 20.0).abs() < 1e-3,
                "corner radius must clamp to 20 (min(w,h)/2), got {c}"
            );
        }
    }

    /// Negative or NaN-ish sizes should clamp radii to 0 without panic.
    #[test]
    fn zero_size_clamps_radii_to_zero() {
        let theme = ResolvedTheme::default();
        let mut scene = Scene::new();
        let mut shaper = crate::shaper::NullShaper;
        let mut pcx = make_pcx(&theme, &mut scene, &mut shaper, [0.0, 0.0, 0.0, 0.0]);
        // `bg` forces emission; box has zero area so radii collapse to 0.
        div().bg([1.0, 0.0, 0.0, 1.0]).rounded_full().paint(&mut pcx);
        let r = &scene.sdf_in_layer(Layer::Chrome)[0];
        assert_eq!(r.radii, [0.0; 4]);
    }

    /// Regression for Codex P2: `text_color_override` must surface the
    /// style's text_color so the walker can propagate it to Text children.
    #[test]
    fn text_color_override_exposes_style_value() {
        let d = div().text_color([1.0, 0.0, 0.0, 1.0]);
        assert_eq!(
            <Div as Element>::text_color_override(&d),
            Some([1.0, 0.0, 0.0, 1.0])
        );
    }

    #[test]
    fn text_color_override_none_when_unset() {
        assert_eq!(<Div as Element>::text_color_override(&div()), None);
    }

    /// Regression for Codex P2: `.on_click(...)` and `.on_hover(...)`
    /// must actually fire when the element receives events. Before the
    /// fix, Div inherited the default no-op `on_event` and the builders
    /// just stashed closures into `Style` that nothing ever invoked.
    #[test]
    fn on_click_fires_on_pointer_down() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        let hits = Arc::new(AtomicUsize::new(0));
        let hc = hits.clone();
        let mut d = div().on_click(move || {
            hc.fetch_add(1, Ordering::Relaxed);
        });
        let mut ecx = EventCtx::new();
        let consumed = d.on_event(&UiEvent::PointerDown { x: 0.0, y: 0.0 }, &mut ecx);
        assert!(consumed);
        assert_eq!(hits.load(Ordering::Relaxed), 1);
        assert!(ecx.needs_redraw());
    }

    #[test]
    fn on_hover_fires_true_on_move_and_false_on_focus_lost() {
        use std::sync::atomic::{AtomicI32, Ordering};
        use std::sync::Arc;
        // +1 for hover-in, -1 for hover-out
        let state = Arc::new(AtomicI32::new(0));
        let s = state.clone();
        let mut d = div().on_hover(move |over| {
            s.fetch_add(if over { 1 } else { -1 }, Ordering::Relaxed);
        });
        let mut ecx = EventCtx::new();
        d.on_event(&UiEvent::PointerMove { x: 0.0, y: 0.0 }, &mut ecx);
        assert_eq!(state.load(Ordering::Relaxed), 1);
        d.on_event(&UiEvent::FocusLost, &mut ecx);
        assert_eq!(state.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn events_on_div_without_handlers_are_ignored() {
        let mut d = div();
        let mut ecx = EventCtx::new();
        let consumed = d.on_event(&UiEvent::PointerDown { x: 0.0, y: 0.0 }, &mut ecx);
        assert!(!consumed);
        assert!(!ecx.needs_redraw());
    }
}
