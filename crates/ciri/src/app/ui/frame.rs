use ciri_config::config::{StatusBarPosition, TabBarPosition};

use super::connection_status::ConnectionStatusComponent;
use super::context_menu::ContextMenuComponent;
use super::hints_bar::HintsBarComponent;
use super::info_box::InfoBoxComponent;
use super::overview::{self, OverviewComponent};
use super::palette::PaletteComponent;
use super::paste_dialog::PasteDialogComponent;
use super::tab_bar::TabBarComponent;
use super::top_bar::TopBarComponent;
use super::types::{
    UiAction, UiContext, UiContextMenuHit, UiOverviewHit, UiPaletteHit, UiPasteDialogHit, UiRect,
    UiScene, UiTopBarHit,
};
use crate::app::top_bar::TopBarLayout;
use crate::app::{App, OverviewActionHover, PasteButton, TopBarHoverRegion};

#[derive(Debug, Clone, Copy)]
pub(crate) struct ChromeRects {
    pub(crate) top_bar: UiRect,
    pub(crate) hints_bar: UiRect,
    pub(crate) side_tab_bar: Option<UiRect>,
}

pub(super) struct UiFrame {
    chrome: ChromeRects,
    top_bar: TopBarComponent,
    hints_bar: HintsBarComponent,
    side_tab_bar: Option<TabBarComponent>,
    overview: OverviewComponent,
    overview_bar: Option<overview::OverviewActionBarData>,
    overview_hover: Option<OverviewActionHover>,
    infobox: Option<InfoBoxComponent>,
    palette: Option<PaletteComponent>,
    connection_status: Option<ConnectionStatusComponent>,
    paste_dialog: Option<PasteDialogComponent>,
    context_menu: Option<ContextMenuComponent>,
}

pub(super) enum UiFrameHover {
    ContextMenu {
        hovered: Option<usize>,
    },
    PasteDialog {
        button: Option<PasteButton>,
    },
    Palette {
        // Hover index is no longer threaded through here — it's derived
        // at chrome-cache-hash time via `App::current_palette_hover()`,
        // and display reads it via `cx.is_hovered(hit_id)`. Only the
        // pointer cursor signal still needs to flow back to the host.
        pointer: bool,
    },
    TopBar {
        region: Option<TopBarHoverRegion>,
        tab: Option<u64>,
    },
    SideTab {
        tab: Option<u64>,
    },
    Overview {
        target: Option<(usize, u64)>,
        action_hover: Option<OverviewActionHover>,
    },
    None,
}

impl UiFrame {
    pub(super) fn capture(
        app: &App,
        cx: &UiContext<'_>,
        top_bar_layout: TopBarLayout,
        top_bar_h: f32,
        hints_bar_h: f32,
    ) -> Self {
        let top_bar = TopBarComponent::capture(app, top_bar_layout, cx);
        let hints_bar = HintsBarComponent::capture(app, cx);
        let side_tab_bar = match cx.config.tabbar.position {
            TabBarPosition::Left | TabBarPosition::Right => Some(TabBarComponent::capture(app, cx)),
            TabBarPosition::Integrated => None,
        };
        let overview_bar = if app.core.overview.active && app.core.overview.hovered_pane.is_some() {
            overview::overview_action_bar_data(app, app.core.overview.hovered_pane)
        } else {
            None
        };

        Self {
            chrome: chrome_rects(app, cx.viewport_w, cx.viewport_h, top_bar_h, hints_bar_h),
            top_bar,
            hints_bar,
            side_tab_bar,
            overview: OverviewComponent::capture(app, cx),
            overview_bar,
            overview_hover: app.core.overview_action_hover,
            infobox: InfoBoxComponent::capture(app, cx),
            palette: PaletteComponent::capture(app, cx),
            connection_status: ConnectionStatusComponent::capture(app, cx),
            paste_dialog: PasteDialogComponent::capture(app, cx),
            context_menu: ContextMenuComponent::capture(app, cx),
        }
    }

    pub(super) fn capture_current(app: &App, cx: &UiContext<'_>) -> Self {
        let top_bar_layout = app.top_bar_layout(
            cx.viewport_w,
            cx.viewport_h,
            cx.cell_w,
            cx.cell_h,
            cx.ui_shaper,
        );
        let top_bar_h = top_bar_layout.bar_height;
        Self::capture(app, cx, top_bar_layout, top_bar_h, app.hints_bar_height())
    }

    pub(super) fn paint(&mut self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        self.top_bar.paint(self.chrome.top_bar, cx, scene);
        self.hints_bar.paint(self.chrome.hints_bar, cx, scene);
        if let (Some(tab_bar), Some(rect)) = (&self.side_tab_bar, self.chrome.side_tab_bar) {
            tab_bar.paint(rect, cx, scene);
        }

        // Modal / overlay layers position themselves absolutely and are
        // painted after chrome so they sit on top.
        if let Some(d) = &self.overview_bar {
            overview::paint_overview_action_bar(d, self.overview_hover, cx, scene);
        }
        if let Some(component) = &self.infobox {
            component.paint(cx, scene);
        }
        if let Some(component) = &mut self.palette {
            component.paint(cx, scene);
        }
        if let Some(component) = &self.connection_status {
            component.paint(cx, scene);
        }
        if let Some(component) = &self.paste_dialog {
            component.paint(cx, scene);
        }
        if let Some(component) = &self.context_menu {
            component.paint(cx, scene);
        }
    }

    pub(super) fn click(
        &self,
        app: &App,
        mx: f32,
        my: f32,
        cx: &UiContext<'_>,
    ) -> (Option<UiAction>, bool) {
        // Components are checked in reverse paint order: topmost first.
        if let Some(c) = &self.context_menu {
            return (c.click(mx, my, cx), true);
        }
        if let Some(c) = &self.paste_dialog {
            return (c.click(mx, my, cx), true);
        }
        if let Some(c) = &self.palette {
            return (c.click(mx, my, cx), true);
        }

        if self.chrome.top_bar.contains(mx, my) {
            return (self.top_bar.click(mx, my, cx), true);
        }

        if let (Some(tab_bar), Some(rect)) = (&self.side_tab_bar, self.chrome.side_tab_bar)
            && rect.contains(mx, my)
        {
            return (tab_bar.hit(rect, mx, my, cx), true);
        }

        if app.core.overview.active {
            let action = match self.overview.hit_test(app, mx, my) {
                UiOverviewHit::Pane(ws_idx, pane_id) => {
                    Some(UiAction::FocusOverviewPane(ws_idx, pane_id))
                }
                UiOverviewHit::FocusPane(ws_idx, pane_id) => {
                    Some(UiAction::FocusOverviewPane(ws_idx, pane_id))
                }
                UiOverviewHit::ClosePane(pane_id) => Some(UiAction::CloseOverviewPane(pane_id)),
                UiOverviewHit::Background => Some(UiAction::StartOverviewDrag),
                UiOverviewHit::None => None,
            };
            return (action, true);
        }

        (None, false)
    }

    pub(super) fn middle_click(
        &self,
        mx: f32,
        my: f32,
        cx: &UiContext<'_>,
    ) -> (Option<UiAction>, bool) {
        if self.chrome.top_bar.contains(mx, my) {
            let action = match self.top_bar.hit_test(mx, my, cx) {
                Some(UiTopBarHit::PaneTab(id)) => Some(UiAction::ClosePaneTab(id)),
                _ => None,
            };
            return (action, true);
        }

        if let (Some(tab_bar), Some(rect)) = (&self.side_tab_bar, self.chrome.side_tab_bar)
            && rect.contains(mx, my)
        {
            let action = match tab_bar.hit(rect, mx, my, cx) {
                Some(UiAction::FocusPaneTab(id)) => Some(UiAction::ClosePaneTab(id)),
                _ => None,
            };
            return (action, true);
        }

        (None, false)
    }

    pub(super) fn hover(&self, app: &App, mx: f32, my: f32, cx: &UiContext<'_>) -> UiFrameHover {
        if let Some(component) = &self.context_menu {
            let hovered = match component.hit_test(mx, my, cx) {
                UiContextMenuHit::Entry(idx) => Some(idx),
                UiContextMenuHit::Menu | UiContextMenuHit::None => None,
            };
            return UiFrameHover::ContextMenu { hovered };
        }

        if let Some(component) = &self.paste_dialog {
            let button = match component.hit_test(mx, my, cx) {
                UiPasteDialogHit::Paste => Some(PasteButton::Paste),
                UiPasteDialogHit::Cancel => Some(PasteButton::Cancel),
                UiPasteDialogHit::Dialog | UiPasteDialogHit::None => None,
            };
            return UiFrameHover::PasteDialog { button };
        }

        if let Some(component) = &self.palette {
            let pointer = matches!(component.hit_test(mx, my, cx), UiPaletteHit::Entry(_));
            return UiFrameHover::Palette { pointer };
        }

        if self.chrome.top_bar.contains(mx, my) {
            let (region, tab) = match self.top_bar.hit_test(mx, my, cx) {
                Some(UiTopBarHit::Session) => (Some(TopBarHoverRegion::Session), None),
                Some(UiTopBarHit::Workspace) => (Some(TopBarHoverRegion::Workspace), None),
                Some(UiTopBarHit::Mode) => (Some(TopBarHoverRegion::Mode), None),
                Some(UiTopBarHit::PaneTab(pane_id)) => (None, Some(pane_id)),
                Some(UiTopBarHit::Background) | None => (None, None),
            };
            return UiFrameHover::TopBar { region, tab };
        }

        if let (Some(tab_bar), Some(rect)) = (&self.side_tab_bar, self.chrome.side_tab_bar)
            && rect.contains(mx, my)
        {
            let tab = match tab_bar.hit(rect, mx, my, cx) {
                Some(UiAction::FocusPaneTab(id)) => Some(id),
                _ => None,
            };
            return UiFrameHover::SideTab { tab };
        }

        if app.core.overview.active {
            let hit = self.overview.hit_test(app, mx, my);
            let action_hover = match &hit {
                UiOverviewHit::ClosePane(_) => Some(OverviewActionHover::Close),
                UiOverviewHit::FocusPane(_, _) => Some(OverviewActionHover::Focus),
                _ => None,
            };
            let target = match hit {
                UiOverviewHit::Pane(ws_idx, pane_id)
                | UiOverviewHit::FocusPane(ws_idx, pane_id) => Some((ws_idx, pane_id)),
                UiOverviewHit::ClosePane(_) => app.core.overview.hovered_pane,
                UiOverviewHit::Background | UiOverviewHit::None => None,
            };
            return UiFrameHover::Overview {
                target,
                action_hover,
            };
        }

        UiFrameHover::None
    }
}

pub(crate) fn chrome_rects(
    app: &App,
    vw: f32,
    vh: f32,
    top_bar_h: f32,
    hints_bar_h: f32,
) -> ChromeRects {
    let top_bar = match app.core.config.statusbar.position {
        StatusBarPosition::Top => UiRect::new(0.0, 0.0, vw, top_bar_h),
        StatusBarPosition::Bottom => UiRect::new(0.0, (vh - top_bar_h).max(0.0), vw, top_bar_h),
    };
    let hints_bar = match app.core.config.statusbar.position {
        StatusBarPosition::Top => UiRect::new(0.0, (vh - hints_bar_h).max(0.0), vw, hints_bar_h),
        StatusBarPosition::Bottom => UiRect::new(
            0.0,
            (vh - top_bar_h - hints_bar_h).max(0.0),
            vw,
            hints_bar_h,
        ),
    };
    let side_tab_bar = match app.core.config.tabbar.position {
        TabBarPosition::Integrated => None,
        TabBarPosition::Left | TabBarPosition::Right => {
            let w = app.core.config.tabbar.width;
            let x = match app.core.config.tabbar.position {
                TabBarPosition::Left => 0.0,
                TabBarPosition::Right => (vw - w).max(0.0),
                TabBarPosition::Integrated => unreachable!(),
            };
            let y = match app.core.config.statusbar.position {
                StatusBarPosition::Top => top_bar_h,
                StatusBarPosition::Bottom => 0.0,
            };
            let h = (vh - top_bar_h - hints_bar_h).max(0.0);
            Some(UiRect::new(x, y, w, h))
        }
    };

    ChromeRects {
        top_bar,
        hints_bar,
        side_tab_bar,
    }
}
