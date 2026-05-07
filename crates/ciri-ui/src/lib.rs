//! Declarative UI toolkit for ciri's chrome and plugin-authored widgets.
//!
//! Design in one line: Rust-native Tailwind-shaped builder (gpui-inspired)
//! on top of a small retained `Element` tree, with style-property animations
//! supplied by [`ciri_motion`].
//!
//! This crate is foundation-only at the moment: it defines the types and
//! traits (`Style`, `Styled`, `Element`, `ResolvedTheme`, `Div`, `Text`)
//! but does not yet emit GPU primitives or run a Taffy layout pass —
//! those land with the renderer integration in a follow-up change.
//!
//! ```ignore
//! use ciri_ui::{div, text, Styled};
//! let _tree = div()
//!     .flex_col()
//!     .items_center()
//!     .gap_1()
//!     .p_2()
//!     .rounded_md()
//!     .child(text("Connecting"))
//!     .child(text("…"));
//! ```

pub mod color;
pub mod arena;
pub mod components;
pub mod element;
pub mod elements;
pub mod fluent;
pub mod layout;
pub mod scene;
pub mod shaper;
pub mod shared_string;
pub mod style;
pub mod styled;
pub mod theme;

pub use arena::{
    Arena, ArenaBox, ElementArenaScope, clear_fallback_element_arena, with_element_arena,
};
pub use color::Color;
pub use element::{
    AnchorCorner, AnchorPlacement, AnyElement, Element, ElementId, ElementStates, EventCtx,
    IntoElement, PaintCtx, Render, RenderCtx, UiCtx, UiEvent,
};
pub use elements::{Anchored, Deferred, Div, Text, anchored, deferred, div, text, uniform_list};
pub use fluent::FluentBuilder;
pub use layout::{
    LayoutNode, LayoutSnapshot, NodeContext, PaintOutput, layout_tree_into_retained, paint_tree,
    paint_tree_into, paint_tree_into_retained, paint_tree_into_with, paint_tree_into_with_layout,
    paint_tree_with_layout,
};
pub use scene::{GlyphInstance, Scene, SdfRect};
pub use shaper::{NullShaper, TextShaper};
pub use shared_string::SharedString;
pub use style::{
    AlignItems, CursorStyle, Display, FlexDirection, JustifyContent, Length, Shadow, Style,
};
pub use styled::Styled;
pub use components::{Banner, Severity};
pub use theme::{ElevationIndex, RadiusScale, ResolvedTheme, SpaceScale, TypeScale};

pub use ciri_motion::Transition;
