//! Style descriptor — a bag of `Option<T>` fields per property.
//!
//! `None` means "inherit / use default"; `Some` means "explicitly set by
//! the builder". Kept as plain data so styles can be diffed, hashed, and
//! (eventually) cross an FFI boundary to a Lua plugin binding.
//!
//! Layout-ish fields (flex, padding, width) mirror the subset of Taffy we
//! intend to use; integration with `taffy` lands with the renderer PR.

use crate::color::Color;
use ciri_motion::Transition;
use std::sync::Arc;

// ── Enums: mirror the CSS / Tailwind / Taffy semantics used by the builder ──

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Display {
    #[default]
    Block,
    Flex,
    None,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FlexDirection {
    #[default]
    Row,
    Column,
    RowReverse,
    ColumnReverse,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AlignItems {
    #[default]
    Stretch,
    FlexStart,
    FlexEnd,
    Center,
    Baseline,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum JustifyContent {
    #[default]
    FlexStart,
    FlexEnd,
    Center,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Position {
    #[default]
    Relative,
    Absolute,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CursorStyle {
    #[default]
    Default,
    Pointer,
    Text,
    ResizeH,
    ResizeV,
}

/// Length unit. `Px` = logical pixels, `Percent` = `[0.0, 1.0]` of parent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Length {
    Px(f32),
    Percent(f32),
    Auto,
}

impl Default for Length {
    fn default() -> Self {
        Self::Auto
    }
}

/// Preset shadow intensities — resolved to concrete blur/offset/color at paint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shadow {
    Sm,
    Md,
    Lg,
}

// ── Event handler types ──

/// Shared event handler. `Arc` so `Style` can be `Clone` even when the
/// handler captures state — element trees are cheaply duplicated for
/// retained-cache diffing.
pub type ClickHandler = Arc<dyn Fn() + Send + Sync + 'static>;
pub type HoverHandler = Arc<dyn Fn(bool) + Send + Sync + 'static>;

// ── Style struct ──

/// All style properties. Every field is `Option<T>` — `None` means "unset".
///
/// The builder methods on [`crate::Styled`] flip individual fields to
/// `Some`; layout and paint read the resulting struct and fold in defaults
/// from the theme.
#[derive(Clone, Default)]
pub struct Style {
    // ── Layout ──
    pub display: Option<Display>,
    pub flex_direction: Option<FlexDirection>,
    pub flex_grow: Option<f32>,
    pub flex_shrink: Option<f32>,
    pub align_items: Option<AlignItems>,
    pub justify_content: Option<JustifyContent>,
    pub gap: Option<f32>,
    /// Padding: `[top, right, bottom, left]`.
    pub padding: Option<[f32; 4]>,
    pub margin: Option<[f32; 4]>,
    pub position: Option<Position>,
    /// Inset: `[top, right, bottom, left]`.
    pub inset: [Option<Length>; 4],
    pub width: Option<Length>,
    pub height: Option<Length>,
    pub min_width: Option<Length>,
    pub max_width: Option<Length>,
    pub min_height: Option<Length>,
    pub max_height: Option<Length>,

    // ── Visual — all animatable ──
    pub background: Option<Color>,
    pub text_color: Option<Color>,
    pub opacity: Option<f32>,
    /// Per-corner radii: `[tl, tr, br, bl]`.
    pub corner_radii: Option<[f32; 4]>,
    pub border_width: Option<f32>,
    pub border_color: Option<Color>,
    pub shadow: Option<Shadow>,
    /// Translate in logical px `(x, y)`.
    pub translate: Option<[f32; 2]>,

    // ── Transitions (CSS-style: tween on value change) ──
    pub transition_background: Option<Transition>,
    pub transition_opacity: Option<Transition>,
    pub transition_text_color: Option<Transition>,
    pub transition_border_color: Option<Transition>,
    pub transition_translate: Option<Transition>,

    // ── Interaction ──
    pub cursor: Option<CursorStyle>,
    /// Host-defined identifier emitted into layout snapshots for pointer
    /// dispatch. `ciri-ui` treats it as opaque; applications map it to their
    /// own action enum.
    pub hit_id: Option<u64>,
    pub on_click: Option<ClickHandler>,
    pub on_hover: Option<HoverHandler>,
}

impl Style {
    pub fn new() -> Self {
        Self::default()
    }

    /// Merge `other` on top of `self`: every `Some` in `other` overrides.
    ///
    /// Semantics are **last-Some-wins** on a per-field basis — the same
    /// mental model as a CSS-style override layer. This is *not* the same
    /// as the paint walker's cascading semantics, which:
    ///   - multiplies `opacity` through the ancestor chain, and
    ///   - adds `translate` through the ancestor chain.
    ///
    /// `merge` is appropriate for stacking style overrides (e.g. "base
    /// style + theme tweak + caller override"). It is **not** correct for
    /// simulating nested-parent effects on `opacity` or `translate` — use
    /// the walker's `inherited_opacity` / `inherited_translate` fields for
    /// those.
    pub fn merge(&mut self, other: &Style) {
        macro_rules! take_some {
            ($field:ident) => {
                if other.$field.is_some() {
                    self.$field = other.$field.clone();
                }
            };
        }
        take_some!(display);
        take_some!(flex_direction);
        take_some!(flex_grow);
        take_some!(flex_shrink);
        take_some!(align_items);
        take_some!(justify_content);
        take_some!(gap);
        take_some!(padding);
        take_some!(margin);
        take_some!(position);
        for (dst, src) in self.inset.iter_mut().zip(other.inset.iter()) {
            if src.is_some() {
                *dst = *src;
            }
        }
        take_some!(width);
        take_some!(height);
        take_some!(min_width);
        take_some!(max_width);
        take_some!(min_height);
        take_some!(max_height);
        take_some!(background);
        take_some!(text_color);
        take_some!(opacity);
        take_some!(corner_radii);
        take_some!(border_width);
        take_some!(border_color);
        take_some!(shadow);
        take_some!(translate);
        take_some!(transition_background);
        take_some!(transition_opacity);
        take_some!(transition_text_color);
        take_some!(transition_border_color);
        take_some!(transition_translate);
        take_some!(cursor);
        take_some!(hit_id);
        take_some!(on_click);
        take_some!(on_hover);
    }
}

impl std::fmt::Debug for Style {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Style")
            .field("display", &self.display)
            .field("flex_direction", &self.flex_direction)
            .field("background", &self.background)
            .field("text_color", &self.text_color)
            .field("opacity", &self.opacity)
            .field("corner_radii", &self.corner_radii)
            .field("border_width", &self.border_width)
            .field("gap", &self.gap)
            .field("padding", &self.padding)
            .field("position", &self.position)
            .field("inset", &self.inset)
            .field("width", &self.width)
            .field("height", &self.height)
            .field("hit_id", &self.hit_id)
            .field("has_on_click", &self.on_click.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_overrides_some() {
        let mut a = Style {
            background: Some([1.0, 0.0, 0.0, 1.0]),
            opacity: Some(0.5),
            ..Default::default()
        };
        let b = Style {
            background: Some([0.0, 1.0, 0.0, 1.0]),
            ..Default::default()
        };
        a.merge(&b);
        assert_eq!(a.background, Some([0.0, 1.0, 0.0, 1.0]));
        assert_eq!(a.opacity, Some(0.5));
    }

    #[test]
    fn default_is_empty() {
        let s = Style::default();
        assert!(s.background.is_none());
        assert!(s.padding.is_none());
    }
}
