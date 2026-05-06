//! `Styled` — the Tailwind-in-Rust chainable builder trait.
//!
//! Method names track gpui/Tailwind so the surface is learnable at a glance:
//! `.flex_col()`, `.items_center()`, `.p_2()`, `.rounded_md()`, `.bg(...)`,
//! `.text_color(...)`, `.on_click(|| ...)`, and so on.
//!
//! Every method takes `self` by value and returns `Self`, matching gpui's
//! builder ergonomics. Implementors need only provide a `&mut Style`.

use std::sync::Arc;

use crate::color::Color;
use crate::style::{
    AlignItems, CursorStyle, Display, FlexDirection, JustifyContent, Length, Position, Shadow,
    Style,
};
use ciri_motion::Transition;

/// The Tailwind-in-Rust builder surface.
///
/// Types that own a `Style` implement this trait and inherit ~40 chainable
/// methods "for free". In practice only `Div` implements this today, but
/// any future container (`Stack`, `Grid`, plugin-defined) can do the same.
pub trait Styled: Sized {
    /// Access to the underlying `Style`. Implementors must return a stable
    /// reference (no shortcuts like "clone out, mutate, put back").
    fn style(&mut self) -> &mut Style;

    // ── Layout ───────────────────────────────────────────────────────────

    fn flex(mut self) -> Self {
        self.style().display = Some(Display::Flex);
        self
    }

    fn flex_col(mut self) -> Self {
        self.style().display = Some(Display::Flex);
        self.style().flex_direction = Some(FlexDirection::Column);
        self
    }

    fn flex_row(mut self) -> Self {
        self.style().display = Some(Display::Flex);
        self.style().flex_direction = Some(FlexDirection::Row);
        self
    }

    fn flex_1(mut self) -> Self {
        self.style().flex_grow = Some(1.0);
        self
    }

    fn flex_none(mut self) -> Self {
        self.style().flex_grow = Some(0.0);
        self.style().flex_shrink = Some(0.0);
        self
    }

    fn items_start(mut self) -> Self {
        self.style().align_items = Some(AlignItems::FlexStart);
        self
    }

    fn items_center(mut self) -> Self {
        self.style().align_items = Some(AlignItems::Center);
        self
    }

    fn items_end(mut self) -> Self {
        self.style().align_items = Some(AlignItems::FlexEnd);
        self
    }

    fn justify_start(mut self) -> Self {
        self.style().justify_content = Some(JustifyContent::FlexStart);
        self
    }

    fn justify_center(mut self) -> Self {
        self.style().justify_content = Some(JustifyContent::Center);
        self
    }

    fn justify_end(mut self) -> Self {
        self.style().justify_content = Some(JustifyContent::FlexEnd);
        self
    }

    fn justify_between(mut self) -> Self {
        self.style().justify_content = Some(JustifyContent::SpaceBetween);
        self
    }

    // ── Spacing (Tailwind scale: 1=4px, 2=8px, 3=12px, 4=16px, 6=24, 8=32) ─

    fn gap(mut self, px: f32) -> Self {
        self.style().gap = Some(px);
        self
    }

    fn gap_1(self) -> Self {
        self.gap(4.0)
    }

    fn gap_2(self) -> Self {
        self.gap(8.0)
    }

    fn p(mut self, px: f32) -> Self {
        self.style().padding = Some([px; 4]);
        self
    }

    fn p_1(self) -> Self {
        self.p(4.0)
    }

    fn p_2(self) -> Self {
        self.p(8.0)
    }

    fn p_3(self) -> Self {
        self.p(12.0)
    }

    fn p_4(self) -> Self {
        self.p(16.0)
    }

    fn px(mut self, px: f32) -> Self {
        let pad = self.style().padding.get_or_insert([0.0; 4]);
        pad[1] = px;
        pad[3] = px;
        self
    }

    fn py(mut self, px: f32) -> Self {
        let pad = self.style().padding.get_or_insert([0.0; 4]);
        pad[0] = px;
        pad[2] = px;
        self
    }

    fn pt(mut self, px: f32) -> Self {
        self.style().padding.get_or_insert([0.0; 4])[0] = px;
        self
    }

    fn pb(mut self, px: f32) -> Self {
        self.style().padding.get_or_insert([0.0; 4])[2] = px;
        self
    }

    fn pl(mut self, px: f32) -> Self {
        self.style().padding.get_or_insert([0.0; 4])[3] = px;
        self
    }

    fn pr(mut self, px: f32) -> Self {
        self.style().padding.get_or_insert([0.0; 4])[1] = px;
        self
    }

    // ── Positioning ─────────────────────────────────────────────────────

    fn relative(mut self) -> Self {
        self.style().position = Some(Position::Relative);
        self
    }

    fn absolute(mut self) -> Self {
        self.style().position = Some(Position::Absolute);
        self
    }

    fn top(mut self, px: f32) -> Self {
        self.style().inset[0] = Some(Length::Px(px));
        self
    }

    fn right(mut self, px: f32) -> Self {
        self.style().inset[1] = Some(Length::Px(px));
        self
    }

    fn bottom(mut self, px: f32) -> Self {
        self.style().inset[2] = Some(Length::Px(px));
        self
    }

    fn left(mut self, px: f32) -> Self {
        self.style().inset[3] = Some(Length::Px(px));
        self
    }

    // ── Sizing ───────────────────────────────────────────────────────────

    fn w(mut self, px: f32) -> Self {
        self.style().width = Some(Length::Px(px));
        self
    }

    fn h(mut self, px: f32) -> Self {
        self.style().height = Some(Length::Px(px));
        self
    }

    fn w_full(mut self) -> Self {
        self.style().width = Some(Length::Percent(1.0));
        self
    }

    fn h_full(mut self) -> Self {
        self.style().height = Some(Length::Percent(1.0));
        self
    }

    fn min_w(mut self, px: f32) -> Self {
        self.style().min_width = Some(Length::Px(px));
        self
    }

    fn max_w(mut self, px: f32) -> Self {
        self.style().max_width = Some(Length::Px(px));
        self
    }

    // ── Visual ───────────────────────────────────────────────────────────

    fn bg(mut self, color: Color) -> Self {
        self.style().background = Some(color);
        self
    }

    fn text_color(mut self, color: Color) -> Self {
        self.style().text_color = Some(color);
        self
    }

    fn opacity(mut self, v: f32) -> Self {
        self.style().opacity = Some(v);
        self
    }

    // ── Corners ──────────────────────────────────────────────────────────

    fn rounded(mut self, px: f32) -> Self {
        self.style().corner_radii = Some([px; 4]);
        self
    }

    fn rounded_sm(self) -> Self {
        self.rounded(6.0)
    }

    fn rounded_md(self) -> Self {
        self.rounded(10.0)
    }

    fn rounded_lg(self) -> Self {
        self.rounded(14.0)
    }

    fn rounded_full(self) -> Self {
        self.rounded(9999.0)
    }

    /// Per-corner radii in CSS order: `[top-left, top-right, bottom-right, bottom-left]`.
    fn rounded_each(mut self, radii: [f32; 4]) -> Self {
        self.style().corner_radii = Some(radii);
        self
    }

    /// Round only the top two corners (e.g. tabs that sit on a baseline).
    fn rounded_t(self, px: f32) -> Self {
        self.rounded_each([px, px, 0.0, 0.0])
    }

    /// Round only the bottom two corners.
    fn rounded_b(self, px: f32) -> Self {
        self.rounded_each([0.0, 0.0, px, px])
    }

    // ── Borders ──────────────────────────────────────────────────────────

    fn border(mut self, width: f32, color: Color) -> Self {
        self.style().border_width = Some(width);
        self.style().border_color = Some(color);
        self
    }

    fn border_color(mut self, color: Color) -> Self {
        self.style().border_color = Some(color);
        self
    }

    fn border_width(mut self, width: f32) -> Self {
        self.style().border_width = Some(width);
        self
    }

    // ── Shadow ───────────────────────────────────────────────────────────

    fn shadow(mut self, s: Shadow) -> Self {
        self.style().shadow = Some(s);
        self
    }

    fn shadow_sm(self) -> Self {
        self.shadow(Shadow::Sm)
    }

    fn shadow_md(self) -> Self {
        self.shadow(Shadow::Md)
    }

    fn shadow_lg(self) -> Self {
        self.shadow(Shadow::Lg)
    }

    // ── Transforms ───────────────────────────────────────────────────────

    fn translate(mut self, x: f32, y: f32) -> Self {
        self.style().translate = Some([x, y]);
        self
    }

    // ── Transitions ──────────────────────────────────────────────────────

    fn transition_bg(mut self, t: Transition) -> Self {
        self.style().transition_background = Some(t);
        self
    }

    fn transition_opacity(mut self, t: Transition) -> Self {
        self.style().transition_opacity = Some(t);
        self
    }

    fn transition_text_color(mut self, t: Transition) -> Self {
        self.style().transition_text_color = Some(t);
        self
    }

    fn transition_border_color(mut self, t: Transition) -> Self {
        self.style().transition_border_color = Some(t);
        self
    }

    fn transition_translate(mut self, t: Transition) -> Self {
        self.style().transition_translate = Some(t);
        self
    }

    // ── Interaction ──────────────────────────────────────────────────────

    fn cursor(mut self, c: CursorStyle) -> Self {
        self.style().cursor = Some(c);
        self
    }

    fn cursor_pointer(self) -> Self {
        self.cursor(CursorStyle::Pointer)
    }

    /// Attach an opaque host-defined hit identifier to this element.
    ///
    /// The layout snapshot copies this value into `LayoutNode::hit_id`; host
    /// code can then map it to an application action without duplicating
    /// geometry outside the UI tree.
    fn hit_id(mut self, id: u64) -> Self {
        self.style().hit_id = Some(id);
        self
    }

    /// Attach a click handler to this element.
    ///
    /// **Not yet dispatched in production.** The current client's real
    /// event dispatcher (`app::ui::dispatch_ui_click`) drives clicks
    /// through per-component `capture()` / `hit_test()` methods on the
    /// legacy chrome widgets — it does not walk the ciri-ui Element tree
    /// to invoke `Element::on_event`. Handlers attached via this builder
    /// will fire for in-crate tests (see `Div`'s `on_click_fires_on_pointer_down`
    /// regression) but not for real clicks on migrated chrome yet. Wiring
    /// the walker into the host dispatcher lands with a future migration.
    fn on_click(mut self, f: impl Fn() + Send + Sync + 'static) -> Self {
        self.style().on_click = Some(Arc::new(f));
        self
    }

    /// Attach a hover handler to this element.
    ///
    /// **Not yet dispatched in production** — see `on_click` for details.
    fn on_hover(mut self, f: impl Fn(bool) + Send + Sync + 'static) -> Self {
        self.style().on_hover = Some(Arc::new(f));
        self
    }
}
