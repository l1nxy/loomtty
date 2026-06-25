//! Style-property-level animation engine for `loom-ui`.
//!
//! Sits on top of `loom-anim` (re-uses `Spring`, `SpringParams`,
//! `EasingCurve`) and adds CSS-style transitions on animatable style
//! properties plus explicit keyframe-style tweens.
//!
//! Not a replacement for `loom-anim` — that crate drives pane-compositor
//! scalars (view offset, zoom, focus opacity) that will migrate here
//! incrementally. While both exist, they share Spring/Easing via re-export.

pub mod anim_prop;
pub mod interp;
pub mod ticker;
pub mod transition;

pub use anim_prop::AnimProp;
pub use interp::Lerp;
pub use loom_anim::easing::EasingCurve;
pub use loom_anim::spring::{Spring, SpringParams};
pub use ticker::Ticker;
pub use transition::Transition;
