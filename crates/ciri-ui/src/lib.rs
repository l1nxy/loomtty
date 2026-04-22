//! Declarative UI toolkit for ciri's chrome and plugin-authored widgets.
//!
//! Design in one line: Rust-native Tailwind-shaped builder (gpui-inspired)
//! on top of a small retained `Element` tree, with style-property animations
//! supplied by [`ciri_motion`].
//!
//! This crate is foundation-only at the moment: it defines the types and
//! traits (`Style`, `Styled`, `Element`, `Layer`, `ResolvedTheme`, `Div`,
//! `Text`) but does not yet emit GPU primitives or run a Taffy layout pass —
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
pub mod element;
pub mod elements;
pub mod layout;
pub mod scene;
pub mod shaper;
pub mod style;
pub mod styled;
pub mod theme;

pub use color::Color;
pub use element::{Element, ElementId, EventCtx, Layer, PaintCtx, UiCtx, UiEvent};
pub use elements::{div, text, Div, Text};
pub use layout::{paint_tree, paint_tree_into};
pub use scene::{GlyphInstance, Scene, SdfRect};
pub use shaper::{NullShaper, TextShaper};
pub use style::{
    AlignItems, CursorStyle, Display, FlexDirection, JustifyContent, Length, Shadow, Style,
};
pub use styled::Styled;
pub use theme::{RadiusScale, ResolvedTheme, SpaceScale, TypeScale};

pub use ciri_motion::Transition;
