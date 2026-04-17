//! Style-property-level animation engine for `ciri-ui`.
//!
//! Sits on top of `ciri-anim` (re-uses `Spring`, `SpringParams`,
//! `EasingCurve`) and adds CSS-style transitions on animatable style
//! properties plus explicit keyframe-style tweens.
//!
//! Not a replacement for `ciri-anim` — that crate drives pane-compositor
//! scalars (view offset, zoom, focus opacity) that will migrate here
//! incrementally. While both exist, they share Spring/Easing via re-export.

pub mod anim_prop;
pub mod interp;
pub mod ticker;
pub mod transition;

pub use anim_prop::AnimProp;
pub use ciri_anim::easing::EasingCurve;
pub use ciri_anim::spring::{Spring, SpringParams};
pub use interp::Lerp;
pub use ticker::Ticker;
pub use transition::Transition;
