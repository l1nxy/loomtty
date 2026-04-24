use ciri_layout::geometry::Rect as GeoRect;
use ciri_ui::{Layer, Styled, div, text};

use super::types::{UiContext, UiScene};
use crate::app::SearchState;
use crate::app::ciri_ui_bridge::paint_ui_tree;

pub(crate) fn paint_search_bar(
    search: &SearchState,
    pane_rect: GeoRect,
    cx: &UiContext<'_>,
    scene: &mut UiScene<'_>,
) {
    let border_w = cx.config.appearance.border_width;
    let padding = cx.config.appearance.padding;
    let bar_height = cx.cell_h + 4.0;
    let bar_y = pane_rect.y + pane_rect.h - border_w - bar_height;
    let bar_x = pane_rect.x + border_w;
    let bar_w = pane_rect.w - border_w * 2.0;

    let match_info = if search.matches.is_empty() {
        if search.query.is_empty() {
            String::new()
        } else {
            " [no matches]".to_string()
        }
    } else {
        format!(
            " [{}/{}]",
            search.current_match_idx + 1,
            search.matches.len()
        )
    };
    let bar_text = format!(" Search: {}{}", search.query, match_info);

    let root = div().w(cx.viewport_w).h(cx.viewport_h).child(
        div()
            .in_layer(Layer::Overlay)
            .absolute()
            .left(bar_x)
            .top(bar_y)
            .w(bar_w)
            .h(bar_height)
            .bg([0.15, 0.15, 0.2, 0.95])
            .child(
                div()
                    .absolute()
                    .left(padding)
                    .top(2.0)
                    .w((bar_w - padding * 2.0).max(0.0))
                    .h(cx.cell_h)
                    .child(text(bar_text).color([1.0, 1.0, 1.0, 1.0])),
            ),
    );
    paint_ui_tree(&root, cx, scene);
}
