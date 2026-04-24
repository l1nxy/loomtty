#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(dead_code)]
pub(crate) enum Axis {
    Horizontal,
    Vertical,
}

/// How much room an element wants along a given axis.
///
/// The container is the authority on the *cross* axis — elements always
/// stretch to fit the cross axis given by their parent.
#[derive(Debug, Clone, Copy)]
pub(crate) enum SizeHint {
    /// Exactly this many pixels.
    Fixed(f32),
    /// Take all remaining space after fixed siblings have been allocated.
    /// When multiple siblings ask to Fill, space is split equally.
    Fill,
}
