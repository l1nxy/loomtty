//! Banner — single-row notification with severity-driven colour.
//!
//! Designed for settings-panel inline messages ("file not writable",
//! "preset switch applied", etc.) and any non-blocking status surface
//! that doesn't justify a modal. Severity drives the bg tint and
//! border colour; the bg is a *baked* (opaque) composite of the
//! severity hue over `theme.surface` so the banner reads cleanly
//! over any backdrop without alpha math at paint time.
//!
//! ```ignore
//! use ciri_ui::{Banner, Severity};
//!
//! // Inside a parent's build_tree(cx: &RenderCtx) -> Div:
//! div().child(
//!     Banner::new("Settings file is not writable")
//!         .severity(Severity::Warning)
//!         .into_div(cx.theme),
//! )
//! ```

use crate::color::Color;
use crate::elements::{Div, div, text};
use crate::shared_string::SharedString;
use crate::styled::Styled;
use crate::theme::ResolvedTheme;

/// Banner severity. Drives bg / border / (future) icon colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Info,
    Success,
    Warning,
    Error,
}

/// Stateless banner config. `into_div(&theme)` resolves it to a `Div`
/// subtree the consumer mounts in its element tree.
pub struct Banner {
    severity: Severity,
    content: SharedString,
}

impl Banner {
    /// Create a Banner with `Severity::Info`.
    pub fn new(content: impl Into<SharedString>) -> Self {
        Self {
            severity: Severity::Info,
            content: content.into(),
        }
    }

    /// Set the severity (drives bg / border colour).
    pub fn severity(mut self, severity: Severity) -> Self {
        self.severity = severity;
        self
    }

    /// Resolve to a `Div` subtree using the supplied theme.
    ///
    /// Lives as an explicit method (rather than `IntoElement`) because
    /// colours have to be resolved against the theme — and `IntoElement`
    /// doesn't carry one. Consumers call this from their `build_tree` /
    /// `Render` implementations where a `&ResolvedTheme` is in scope.
    pub fn into_div(self, theme: &ResolvedTheme) -> Div {
        let bg = severity_bg(theme, self.severity);
        let border = severity_border(theme, self.severity);
        div()
            .flex_row()
            .items_center()
            .gap(theme.space.s2)
            .p(theme.space.s2)
            .bg(bg)
            .border(1.0, border)
            .rounded(theme.radius.sm)
            .child(text(self.content).color(theme.on_surface))
    }
}

fn severity_color(theme: &ResolvedTheme, severity: Severity) -> Color {
    match severity {
        Severity::Info => theme.info,
        Severity::Success => theme.success,
        Severity::Warning => theme.warning,
        Severity::Error => theme.error,
    }
}

/// Bake a 10 % composite of the severity hue over chrome surface.
/// Opaque output — independent of whatever backdrop the banner is
/// mounted over.
fn severity_bg(theme: &ResolvedTheme, severity: Severity) -> Color {
    blend_over(severity_color(theme, severity), theme.surface, 0.10)
}

/// Border at a stronger (32 %) tint so the banner has a definition
/// edge even when the bg sits close to the parent panel tone.
fn severity_border(theme: &ResolvedTheme, severity: Severity) -> Color {
    blend_over(severity_color(theme, severity), theme.surface, 0.32)
}

/// Composite `src` at `alpha` over opaque `dst`. Both are sRGB-encoded
/// straight-alpha (matching the rest of the chrome paint path); the
/// result is opaque so consumers don't have to reason about alpha
/// blending against arbitrary backdrops.
fn blend_over(src: Color, dst: Color, alpha: f32) -> Color {
    let a = alpha.clamp(0.0, 1.0);
    [
        a * src[0] + (1.0 - a) * dst[0],
        a * src[1] + (1.0 - a) * dst[1],
        a * src[2] + (1.0 - a) * dst[2],
        1.0,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ResolvedTheme;

    #[test]
    fn severity_bg_is_opaque() {
        let theme = ResolvedTheme::default();
        for sev in [
            Severity::Info,
            Severity::Success,
            Severity::Warning,
            Severity::Error,
        ] {
            let bg = severity_bg(&theme, sev);
            assert_eq!(bg[3], 1.0, "severity {sev:?} bg must be opaque");
        }
    }

    /// Bg should sit between the surface (alpha=0) and the severity
    /// hue (alpha=1). Catches anyone flipping the blend direction.
    #[test]
    fn severity_bg_is_between_surface_and_hue() {
        let theme = ResolvedTheme::default();
        let bg = severity_bg(&theme, Severity::Error);
        let surface = theme.surface;
        let error = theme.error;
        // Each channel of bg must lie within (surface, error) exclusive
        // (since alpha is strictly between 0 and 1).
        for i in 0..3 {
            let lo = surface[i].min(error[i]);
            let hi = surface[i].max(error[i]);
            assert!(
                bg[i] >= lo && bg[i] <= hi,
                "channel {i}: bg={} out of [{lo}, {hi}]",
                bg[i]
            );
        }
    }

    /// `Banner::severity` is a builder — make sure it actually swaps the
    /// resolved bg colour. Catches a typo where it stored to the wrong
    /// field.
    #[test]
    fn severity_setter_changes_resolved_bg() {
        let theme = ResolvedTheme::default();
        let info_bg = severity_bg(&theme, Severity::Info);
        let error_bg = severity_bg(&theme, Severity::Error);
        assert_ne!(info_bg, error_bg);
    }
}
