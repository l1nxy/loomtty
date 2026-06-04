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

use crate::color::{Color, TRANSPARENT, mul_alpha};
use crate::element::{AnyElement, Element, EventCtx, IntoElement, PaintCtx, UiEvent};
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
    /// Optional refinement applied on top of `style` (and on top of
    /// `hover_style` when both match) while a mouse button is held
    /// after pressing on this element — i.e. when
    /// `cx.is_active(hit_id)`. Built by `.active(|s| s.bg(...))`.
    /// Same boxed-Option shape as `hover_style` so divs without
    /// active feedback pay no extra storage.
    active_style: Option<Box<Style>>,
    /// Optional refinement applied unconditionally when [`Div::disabled`]
    /// is called with `disabled=true`. The merge layers it on top of
    /// hover / active in [`Div::effective_style`].
    disabled_style: Option<Box<Style>>,
    /// Whether this Div is currently disabled. Set by
    /// [`Div::disabled`]. Gates `Element::hit_id` and
    /// `Element::accepts_pointer_events` at trait-method time so the
    /// inertness is order-independent — `.disabled(true).hit_id(7)`
    /// is just as inert as `.hit_id(7).disabled(true)`. Without this
    /// flag the eager `style.hit_id = None` clear could be silently
    /// undone by a subsequent builder call (codex caught it on Step 41
    /// review).
    disabled: bool,
    children: SmallVec<[AnyElement; 2]>,
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
            active_style: None,
            disabled_style: None,
            disabled: false,
            children: SmallVec::new(),
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

    /// Apply a refinement on top of the base style (and on top of any
    /// `.hover()` refinement) while the host reports this element as
    /// the active press target. Same builder shape as `.hover()`;
    /// requires `hit_id` set so the framework can identify "this
    /// element is the press target". Without `.hit_id()`, the
    /// refinement silently no-ops.
    ///
    /// Sticky-on-drag semantics are owned by the host: the framework
    /// only checks `cx.is_active(hit_id)`. Whether the active state
    /// persists across cursor moves depends on the host keeping
    /// [`PaintCtx::active_hit_id`] set from mouse-down to mouse-up.
    ///
    /// ```ignore
    /// div().bg(rest).hover(|s| s.bg(hov)).active(|s| s.bg(pressed))
    /// ```
    pub fn active(mut self, f: impl FnOnce(Style) -> Style) -> Self {
        let refinement = f(Style::new());
        self.active_style = Some(Box::new(refinement));
        self
    }

    /// Mark this Div disabled-or-not, and supply the visual refinement
    /// for the disabled state. When `disabled` is `true`:
    ///
    /// 1. The refinement (`f(Style::new())`) is layered on top of the
    ///    base `style` at paint time — same merge plumbing used by
    ///    `.hover()` / `.active()`.
    /// 2. `Element::hit_id` returns `None` so the layout walker won't
    ///    surface this element for hit-test, hover, or active.
    /// 3. `Element::accepts_pointer_events` returns `false` so clicks
    ///    fall through to whatever sits underneath (e.g. a disabled
    ///    context-menu row falls through to the panel's own hit_id,
    ///    which is a click-on-menu no-op).
    ///
    /// Both gates are applied at the `Element` trait level (not at
    /// builder time), so chain order doesn't matter:
    /// `.disabled(true, ...).hit_id(7)` is just as inert as
    /// `.hit_id(7).disabled(true, ...)`. The base `style.hit_id` is
    /// preserved on the Div itself — only the trait-method projection
    /// gates it — so future hover/active refinements that reference
    /// it would still match if `disabled` were toggled false later.
    ///
    /// When `disabled` is `false` this method is a no-op (the
    /// refinement closure isn't even invoked), so callers can wire it
    /// unconditionally:
    ///
    /// ```ignore
    /// div().bg(rest).hit_id(7).cursor_pointer().hover(|s| s.bg(hov))
    ///     .disabled(item.disabled, |s| s.text_color(dim))
    /// ```
    pub fn disabled(mut self, disabled: bool, f: impl FnOnce(Style) -> Style) -> Self {
        self.disabled = disabled;
        if disabled {
            let refinement = f(Style::new());
            self.disabled_style = Some(Box::new(refinement));
        }
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

    fn type_id(&self) -> &'static str {
        "loom.div"
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

    /// Refinement-aware variant: surfaces the effective text colour
    /// after considering `.hover()` / `.active()` / `.disabled()`
    /// refinements. Specificity order matches `effective_style`:
    /// disabled > active > hover > base. The walker calls this so
    /// refinement-state text colours propagate to descendant Text
    /// nodes.
    fn text_color_override_with_state(
        &self,
        hovered_hit_id: Option<u64>,
        active_hit_id: Option<u64>,
    ) -> Option<Color> {
        // Disabled is the most specific. When set, its `text_color`
        // (if any) wins; otherwise fall through to the base `style`.
        // Hover / active are skipped here because they're suppressed
        // for disabled elements (see `effective_style`).
        if self.disabled {
            return self
                .disabled_style
                .as_ref()
                .and_then(|d| d.text_color)
                .or(self.style.text_color);
        }
        let hit_id = self.style.hit_id;
        let active =
            hit_id.is_some_and(|id| active_hit_id == Some(id)) && self.active_style.is_some();
        if active {
            return self
                .active_style
                .as_ref()
                .and_then(|act| act.text_color)
                .or_else(|| {
                    self.hover_style
                        .as_ref()
                        .filter(|_| hit_id.is_some_and(|id| hovered_hit_id == Some(id)))
                        .and_then(|hov| hov.text_color)
                })
                .or(self.style.text_color);
        }
        let hovered =
            hit_id.is_some_and(|id| hovered_hit_id == Some(id)) && self.hover_style.is_some();
        if hovered {
            // Refinement wins when set; fall back to base.
            self.hover_style
                .as_ref()
                .and_then(|hov| hov.text_color)
                .or(self.style.text_color)
        } else {
            self.style.text_color
        }
    }

    fn background_override(&self) -> Option<Color> {
        // Transparent fills don't impose a backdrop on descendants —
        // text inside `bg(TRANSPARENT)` should see the ancestor's bg
        // (which is what the user actually sees through the hole),
        // not get its glyph correction zeroed out.
        self.style.background.filter(|c| c[3] > 0.0)
    }

    /// Refinement-aware variant: surfaces the effective bg after
    /// `.hover()` / `.active()` / `.disabled()` refinements. Mirrors
    /// `text_color_override_with_state` so descendant Text glyphs get
    /// the bg the user is actually looking at — including transient
    /// hover/press tints — for the linear-correction shader.
    ///
    /// Same transparent-fall-through rule as `background_override`:
    /// a refinement that resolves to `alpha=0` is treated as "no
    /// override" so the cascade keeps looking up the tree.
    fn background_override_with_state(
        &self,
        hovered_hit_id: Option<u64>,
        active_hit_id: Option<u64>,
    ) -> Option<Color> {
        if self.disabled {
            return self
                .disabled_style
                .as_ref()
                .and_then(|d| d.background)
                .or(self.style.background)
                .filter(|c| c[3] > 0.0);
        }
        let hit_id = self.style.hit_id;
        let active =
            hit_id.is_some_and(|id| active_hit_id == Some(id)) && self.active_style.is_some();
        if active {
            return self
                .active_style
                .as_ref()
                .and_then(|act| act.background)
                .or_else(|| {
                    self.hover_style
                        .as_ref()
                        .filter(|_| hit_id.is_some_and(|id| hovered_hit_id == Some(id)))
                        .and_then(|hov| hov.background)
                })
                .or(self.style.background)
                .filter(|c| c[3] > 0.0);
        }
        let hovered =
            hit_id.is_some_and(|id| hovered_hit_id == Some(id)) && self.hover_style.is_some();
        if hovered {
            self.hover_style
                .as_ref()
                .and_then(|hov| hov.background)
                .or(self.style.background)
                .filter(|c| c[3] > 0.0)
        } else {
            self.style.background.filter(|c| c[3] > 0.0)
        }
    }

    fn accepts_pointer_events(&self) -> bool {
        // Disabled elements drop out of hit-testing entirely so clicks
        // pass through to whatever sits underneath (e.g. a disabled
        // context-menu row → click hits the panel and is a no-op).
        if self.disabled {
            return false;
        }
        self.style.on_click.is_some()
            || self.style.on_hover.is_some()
            || self.style.cursor.is_some()
            || self.style.hit_id.is_some()
    }

    fn hit_id(&self) -> Option<u64> {
        if self.disabled {
            None
        } else {
            self.style.hit_id
        }
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
    /// over this element, then `active_style` merged on top of that
    /// when a button is currently held on this element, then
    /// `disabled_style` merged on top of those when the caller passed
    /// `disabled=true`. Disabled wins over active wins over hover —
    /// disabled is the most specific because it overrides interaction
    /// state entirely, active is more specific than hover (CSS
    /// semantics: press is more specific than hover). Returns a
    /// borrowed reference when no refinement applies (the common
    /// case) so we don't pay a Style clone for every static-styled
    /// element.
    fn effective_style<'a>(&'a self, cx: &PaintCtx<'_>) -> std::borrow::Cow<'a, Style> {
        let hit_id = self.style.hit_id;
        // Disabled gates out hover/active to keep paint consistent
        // with the trait-level `Element::hit_id` projection — a
        // disabled element shouldn't light up its hover style even
        // if the cursor is technically over it (see Finding 2 from
        // the codex review of Step 41).
        let hovered = !self.disabled
            && hit_id.is_some_and(|id| cx.is_hovered(id))
            && self.hover_style.is_some();
        let active = !self.disabled
            && hit_id.is_some_and(|id| cx.is_active(id))
            && self.active_style.is_some();
        let disabled = self.disabled_style.is_some();
        if !hovered && !active && !disabled {
            return std::borrow::Cow::Borrowed(&self.style);
        }
        let mut merged = self.style.clone();
        if hovered && let Some(hov) = &self.hover_style {
            merged.merge(hov);
        }
        if active && let Some(act) = &self.active_style {
            merged.merge(act);
        }
        if let Some(dis) = &self.disabled_style {
            merged.merge(dis);
        }
        std::borrow::Cow::Owned(merged)
    }
}

/// True iff the style would produce any visible pixels. Corner radii alone
/// don't emit anything — they only shape an existing fill/border/shadow — so
/// they aren't part of this check (a bare `div().rounded_md()` with no fill
/// would otherwise upload a no-op transparent SdfRect every frame).
fn has_visual(s: &Style) -> bool {
    s.background.is_some() || s.border_width.map_or(false, |w| w > 0.0) || s.shadow.is_some()
}

/// Map the semantic `Shadow` enum to concrete (blur, color, offset).
///
/// Values lean Linear/VS Code rather than Tailwind: wider blur with
/// lower alpha gives a soft falloff that reads as "floating chrome"
/// rather than a hard drop shadow stuck on a pane.
fn resolve_shadow(s: Option<Shadow>) -> (f32, Color, [f32; 2]) {
    match s {
        Some(Shadow::Sm) => (6.0, [0.0, 0.0, 0.0, 0.14], [0.0, 2.0]),
        Some(Shadow::Md) => (16.0, [0.0, 0.0, 0.0, 0.20], [0.0, 4.0]),
        Some(Shadow::Lg) => (28.0, [0.0, 0.0, 0.0, 0.26], [0.0, 8.0]),
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
            inherited_bg: None,
            hovered_hit_id: None,
            active_hit_id: None,
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
        let rects: Vec<_> = scene.sdf_rects_iter().collect();
        let r = rects[0];
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
        let rects: Vec<_> = scene.sdf_rects_iter().collect();
        let r = rects[0];
        assert_eq!(r.color, [1.0, 0.0, 0.0, 1.0]);
    }

    #[test]
    fn active_style_overlays_when_hit_id_matches() {
        const REST: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
        const ACT: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
        let theme = ResolvedTheme::default();
        let mut scene = Scene::new();
        let mut shaper = crate::shaper::NullShaper;
        let mut pcx = make_pcx(&theme, &mut scene, &mut shaper, [0.0, 0.0, 100.0, 40.0]);
        pcx.active_hit_id = Some(7);
        div()
            .hit_id(7)
            .bg(REST)
            .active(|s| s.bg(ACT))
            .paint(&mut pcx);
        let rects: Vec<_> = scene.sdf_rects_iter().collect();
        assert_eq!(rects[0].color, ACT, "active refinement wins");
    }

    #[test]
    fn active_style_overrides_hover_when_both_match() {
        const REST: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
        const HOV: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
        const ACT: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
        let theme = ResolvedTheme::default();
        let mut scene = Scene::new();
        let mut shaper = crate::shaper::NullShaper;
        let mut pcx = make_pcx(&theme, &mut scene, &mut shaper, [0.0, 0.0, 100.0, 40.0]);
        // Cursor over AND button held — active is more specific than hover
        // and must dominate the merged style.
        pcx.hovered_hit_id = Some(7);
        pcx.active_hit_id = Some(7);
        div()
            .hit_id(7)
            .bg(REST)
            .hover(|s| s.bg(HOV))
            .active(|s| s.bg(ACT))
            .paint(&mut pcx);
        let rects: Vec<_> = scene.sdf_rects_iter().collect();
        assert_eq!(rects[0].color, ACT, "active beats hover when both match");
    }

    #[test]
    fn active_style_skipped_when_not_active() {
        const REST: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
        const ACT: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
        let theme = ResolvedTheme::default();
        let mut scene = Scene::new();
        let mut shaper = crate::shaper::NullShaper;
        let mut pcx = make_pcx(&theme, &mut scene, &mut shaper, [0.0, 0.0, 100.0, 40.0]);
        pcx.active_hit_id = Some(99); // pressed on a different element
        div()
            .hit_id(7)
            .bg(REST)
            .active(|s| s.bg(ACT))
            .paint(&mut pcx);
        let rects: Vec<_> = scene.sdf_rects_iter().collect();
        assert_eq!(rects[0].color, REST, "no active when hit_id mismatches");
    }

    /// Mirror of `hover_style` requiring `hit_id`: a Div with
    /// `.active(|s| ...)` but no `.hit_id()` set silently no-ops
    /// regardless of whether `pcx.active_hit_id` is `Some`. Without
    /// an identity, the framework can't tell "is this element the
    /// press target" — so the refinement never matches.
    #[test]
    fn active_style_noop_when_no_hit_id_set() {
        const REST: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
        const ACT: [f32; 4] = [0.0, 0.0, 1.0, 1.0];
        let theme = ResolvedTheme::default();
        let mut scene = Scene::new();
        let mut shaper = crate::shaper::NullShaper;
        let mut pcx = make_pcx(&theme, &mut scene, &mut shaper, [0.0, 0.0, 100.0, 40.0]);
        pcx.active_hit_id = Some(7); // host has *some* press target,
        // but this Div didn't opt in via `.hit_id(...)`.
        div().bg(REST).active(|s| s.bg(ACT)).paint(&mut pcx);
        let rects: Vec<_> = scene.sdf_rects_iter().collect();
        assert_eq!(rects[0].color, REST, "no hit_id ⇒ no active match");
    }

    #[test]
    fn disabled_refinement_overlays_when_flag_true() {
        const REST: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
        const DIS: [f32; 4] = [0.5, 0.5, 0.5, 1.0];
        let theme = ResolvedTheme::default();
        let mut scene = Scene::new();
        let mut shaper = crate::shaper::NullShaper;
        let mut pcx = make_pcx(&theme, &mut scene, &mut shaper, [0.0, 0.0, 100.0, 40.0]);
        div().bg(REST).disabled(true, |s| s.bg(DIS)).paint(&mut pcx);
        let rects: Vec<_> = scene.sdf_rects_iter().collect();
        assert_eq!(rects[0].color, DIS, "disabled refinement should apply");
    }

    #[test]
    fn disabled_refinement_skipped_when_flag_false() {
        const REST: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
        const DIS: [f32; 4] = [0.5, 0.5, 0.5, 1.0];
        let theme = ResolvedTheme::default();
        let mut scene = Scene::new();
        let mut shaper = crate::shaper::NullShaper;
        let mut pcx = make_pcx(&theme, &mut scene, &mut shaper, [0.0, 0.0, 100.0, 40.0]);
        div()
            .bg(REST)
            .disabled(false, |s| s.bg(DIS))
            .paint(&mut pcx);
        let rects: Vec<_> = scene.sdf_rects_iter().collect();
        assert_eq!(rects[0].color, REST, "disabled=false ⇒ no refinement");
    }

    /// `.disabled(true, ...)` MUST make the element interactive-inert
    /// at the Element trait surface — `hit_id()` returns None, and
    /// `accepts_pointer_events()` returns false even though the base
    /// `style.hit_id` / `style.cursor` are still set.
    #[test]
    fn disabled_gates_hit_id_and_pointer_events() {
        let d = div()
            .hit_id(7)
            .cursor_pointer()
            .disabled(true, |s| s.text_color([0.5; 4]));
        assert_eq!(<Div as Element>::hit_id(&d), None, "hit_id gated");
        assert!(
            !<Div as Element>::accepts_pointer_events(&d),
            "disabled ⇒ no pointer events",
        );
    }

    /// Order-independence: `.disabled(true, ...)` followed by
    /// `.hit_id(7).cursor_pointer()` MUST still leave the element
    /// inert. The trait-level gate (Step 41 codex review fix) means
    /// the chain order doesn't matter — `disabled` is checked at
    /// `Element::hit_id` / `accepts_pointer_events` projection time,
    /// not eagerly cleared at builder time.
    #[test]
    fn disabled_inertness_is_order_independent() {
        let d = div()
            .disabled(true, |s| s.text_color([0.5; 4]))
            .hit_id(7)
            .cursor_pointer();
        assert_eq!(
            <Div as Element>::hit_id(&d),
            None,
            "disabled wins regardless of chain order",
        );
        assert!(
            !<Div as Element>::accepts_pointer_events(&d),
            "disabled wins regardless of chain order",
        );
    }

    /// A Div with `.hover()` set but `.disabled(true)` applied should
    /// not light up the hover refinement even when the cursor sits on
    /// it — because `.disabled()` cleared hit_id, the cursor's
    /// `hovered_hit_id` can never match.
    #[test]
    fn disabled_suppresses_hover_match() {
        const REST: [f32; 4] = [1.0, 0.0, 0.0, 1.0];
        const HOV: [f32; 4] = [0.0, 1.0, 0.0, 1.0];
        const DIS: [f32; 4] = [0.5, 0.5, 0.5, 1.0];
        let theme = ResolvedTheme::default();
        let mut scene = Scene::new();
        let mut shaper = crate::shaper::NullShaper;
        let mut pcx = make_pcx(&theme, &mut scene, &mut shaper, [0.0, 0.0, 100.0, 40.0]);
        // Cursor IS on hit_id 7, but disabled cleared it, so no hover.
        pcx.hovered_hit_id = Some(7);
        div()
            .bg(REST)
            .hit_id(7)
            .hover(|s| s.bg(HOV))
            .disabled(true, |s| s.bg(DIS))
            .paint(&mut pcx);
        let rects: Vec<_> = scene.sdf_rects_iter().collect();
        assert_eq!(
            rects[0].color, DIS,
            "disabled wins, hover is suppressed via cleared hit_id",
        );
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
        div()
            .bg([1.0, 0.0, 0.0, 1.0])
            .rounded_full()
            .paint(&mut pcx);
        let rects: Vec<_> = scene.sdf_rects_iter().collect();
        let r = rects[0];
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
        div()
            .bg([1.0, 0.0, 0.0, 1.0])
            .rounded_full()
            .paint(&mut pcx);
        let rects: Vec<_> = scene.sdf_rects_iter().collect();
        let r = rects[0];
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
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
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
        use std::sync::Arc;
        use std::sync::atomic::{AtomicI32, Ordering};
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
