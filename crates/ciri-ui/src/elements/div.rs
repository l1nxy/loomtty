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
use crate::element::{Element, EventCtx, Layer, PaintCtx, UiEvent};
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

    fn text_color_override(&self) -> Option<Color> {
        self.style.text_color
    }

    fn accepts_pointer_events(&self) -> bool {
        self.style.on_click.is_some()
            || self.style.on_hover.is_some()
            || self.style.cursor.is_some()
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
        let (shadow_blur, shadow_color, shadow_offset) = resolve_shadow(self.style.shadow);

        let [x, y, w, h] = cx.bounds;
        // The rounded-box SDF is only well-defined when every radius is
        // ≤ half the shorter side. Without this, `.rounded_full()`
        // (radii = [9999.0; 4]) on a non-square box produces a positive
        // distance even at the centre and the fill vanishes. Clamp so
        // semantic sugar like "full = capsule" behaves correctly.
        let max_r = (w.min(h)).max(0.0) * 0.5;
        let raw_radii = self.style.corner_radii.unwrap_or([0.0; 4]);
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
