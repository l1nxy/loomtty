use ciri_config::config::{StatusBarPosition, TabBarPosition};

use super::connection_status::ConnectionStatusComponent;
use super::context_menu::ContextMenuComponent;
use super::hints_bar::HintsBarComponent;
use super::info_box::InfoBoxComponent;
use super::overview::{self, OverviewComponent};
use super::palette::PaletteComponent;
use super::paste_dialog::PasteDialogComponent;
use super::settings_panel::SettingsPanelComponent;
use super::tab_bar::TabBarComponent;
use super::top_bar::TopBarComponent;
use super::types::{
    UiAction, UiContext, UiContextMenuHit, UiOverviewHit, UiPaletteHit, UiPasteDialogHit, UiRect,
    UiScene, UiSettingsHit, UiTopBarHit,
};
use crate::app::top_bar::TopBarLayout;
use crate::app::{App, PasteButton, TopBarHoverRegion};

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
    overview_bar: Option<overview::OverviewActionBarComponent>,
    infobox: Option<InfoBoxComponent>,
    palette: Option<PaletteComponent>,
    connection_status: Option<ConnectionStatusComponent>,
    paste_dialog: Option<PasteDialogComponent>,
    settings_panel: Option<SettingsPanelComponent>,
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
    Settings {
        // Same shape as `Palette`: the per-element hover styling is
        // already declarative via `.hover()` refinements driven by
        // `cx.is_hovered(hit_id)`. The hover handler only needs to
        // signal "cursor sits over an interactive element" so the host
        // flips the OS cursor to a pointer.
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
        // `action_hover` removed — derived at chrome-cache-hash time
        // via `App::current_overview_action_hover()` from the cursor +
        // bar geometry. The hit-test still computes which button the
        // cursor is on (used to decide cursor pointer style), but the
        // outcome doesn't need to thread back here.
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
        let chrome = chrome_rects(app, cx.viewport_w, cx.viewport_h, top_bar_h, hints_bar_h);
        // HintsBar takes its layout rect at capture time so its
        // `Render::render` can read everything from `self` — no
        // host-supplied rect during paint.
        let hints_bar = HintsBarComponent::capture(app, cx, chrome.hints_bar);
        let side_tab_bar = match cx.config.tabbar.position {
            TabBarPosition::Left | TabBarPosition::Right => chrome
                .side_tab_bar
                .map(|rect| TabBarComponent::capture(app, cx, rect)),
            TabBarPosition::Integrated => None,
        };
        let overview_bar = if app.core.overview.active && app.overview_hovered_pane.is_some() {
            overview::overview_action_bar_data(app, app.overview_hovered_pane)
                .map(|data| overview::OverviewActionBarComponent { data })
        } else {
            None
        };

        Self {
            chrome,
            top_bar,
            hints_bar,
            side_tab_bar,
            overview: OverviewComponent::capture(app, cx),
            overview_bar,
            infobox: InfoBoxComponent::capture(app, cx),
            palette: PaletteComponent::capture(app, cx),
            connection_status: ConnectionStatusComponent::capture(app, cx),
            paste_dialog: PasteDialogComponent::capture(app, cx),
            settings_panel: SettingsPanelComponent::capture(app, cx),
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

    /// Paint Base-layer chrome. Runs first; rects + glyphs from these
    /// components occupy the lower z-tier and are drawn fully (rects,
    /// then glyphs) before Overlay primitives. Caller records the per-
    /// stream lengths after this returns to know the layer split.
    pub(super) fn paint_base(&mut self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        self.top_bar.paint(self.chrome.top_bar, cx, scene);
        self.hints_bar.paint(cx, scene);
        if let (Some(tab_bar), Some(_rect)) = (&mut self.side_tab_bar, self.chrome.side_tab_bar) {
            tab_bar.paint(cx, scene);
        }

        if let Some(component) = &mut self.overview_bar {
            component.paint(cx, scene);
        }
        if let Some(component) = &mut self.infobox {
            component.paint(cx, scene);
        }
        if let Some(component) = &mut self.connection_status {
            component.paint(cx, scene);
        }
        if let Some(component) = &self.settings_panel {
            component.paint(cx, scene);
        }
    }

    /// Paint Overlay-layer chrome on top of Base. Multiple components
    /// can coexist here under the layered `ModalKind` invariant:
    /// `PendingPaste(CommandPalette/Search)` keeps both palette and
    /// paste_dialog alive — paste_dialog must paint AFTER palette so
    /// the confirmation visually covers the palette beneath, matching
    /// the click-priority order in `click` / `hover`. context_menu
    /// can't coexist with paste_dialog per `kept_set`, so its order
    /// vs paste_dialog is moot. The renderer draws Overlay rects
    /// after Base glyphs, so these popups cover everything underneath
    /// including labels.
    pub(super) fn paint_overlay(&mut self, cx: &UiContext<'_>, scene: &mut UiScene<'_>) {
        if let Some(component) = &mut self.palette {
            component.paint(cx, scene);
        }
        if let Some(component) = &self.paste_dialog {
            component.paint(cx, scene);
        }
        if let Some(component) = &mut self.context_menu {
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
        // `paste_dialog` runs ahead of `palette` because the layered
        // `ModalKind::PendingPaste(CommandPalette/Search)` case keeps
        // BOTH alive — the confirmation dialog must win clicks over
        // the palette underneath, otherwise Paste/Cancel buttons hit
        // the palette and either close it or move its selection.
        // `context_menu` stays first (theme-dropdown / right-click
        // menu can't coexist with paste_dialog per kept_set).
        if let Some(c) = &self.context_menu {
            return (c.click(mx, my, cx), true);
        }
        if let Some(c) = &self.paste_dialog {
            return (c.click(mx, my, cx), true);
        }
        if let Some(c) = &self.palette {
            return (c.click(mx, my, cx), true);
        }
        if let Some(c) = &self.settings_panel {
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
        // Modal precedence — same shape as `click` / `hover`. Without
        // this guard, middle-clicking a top/side tab THROUGH the
        // settings backdrop (or any modal that owns the surface)
        // would dispatch `ClosePaneTab` and close a pane behind the
        // panel. Consume the click when any modal is alive; tabs
        // are only reachable when the surface is otherwise idle.
        if self.context_menu.is_some()
            || self.paste_dialog.is_some()
            || self.palette.is_some()
            || self.settings_panel.is_some()
        {
            return (None, true);
        }

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
        // Same precedence as `click`: paste_dialog runs ahead of
        // palette so the layered `PendingPaste(CommandPalette/Search)`
        // case routes hover styling to the topmost dialog rather than
        // the palette underneath.
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

        if let Some(component) = &self.settings_panel {
            // Pointer cursor on any interactive hit (close button, theme
            // dropdown trigger, opacity steppers, "Open settings.toml"
            // link). Background body / outside fall back to default.
            let pointer = match component.hit_test(mx, my, cx) {
                UiSettingsHit::Close
                | UiSettingsHit::OpenToml
                | UiSettingsHit::ThemeDropdown
                | UiSettingsHit::PaneOpacityDec
                | UiSettingsHit::PaneOpacityInc => true,
                UiSettingsHit::Dialog | UiSettingsHit::None => false,
            };
            return UiFrameHover::Settings { pointer };
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
            let target = match hit {
                UiOverviewHit::Pane(ws_idx, pane_id)
                | UiOverviewHit::FocusPane(ws_idx, pane_id) => Some((ws_idx, pane_id)),
                UiOverviewHit::ClosePane(_) => app.overview_hovered_pane,
                UiOverviewHit::Background | UiOverviewHit::None => None,
            };
            return UiFrameHover::Overview { target };
        }

        UiFrameHover::None
    }

    /// Look up the chrome `hit_id` at `(mx, my)` for the purposes of
    /// `.active()` press-state styling. Only returns hit_ids for
    /// elements that benefit from press feedback — chrome that
    /// stays visible long enough between press and release for the
    /// `.active()` refinement to render frames.
    ///
    /// Currently covers:
    /// - Top-bar **pane tabs**: click focuses a pane without
    ///   dismissing the bar.
    /// - Top-bar **session label** and **workspace indicator**: click
    ///   opens the palette, but the bar stays painted under the
    ///   palette backdrop, so the press tint is visible at the edges.
    /// - Side **tab bar** rows: click focuses the pane without
    ///   dismissing the bar; same shape as the integrated tabs.
    ///
    /// Returns `None` for click-and-dismiss chrome (palette /
    /// paste_dialog / context_menu rows) so they don't flash an
    /// active style for the one frame between press and dismiss.
    /// Mode indicator is also skipped because it has no hover
    /// styling — adding `.active()` without `.hover()` would feel
    /// inconsistent with the rest of the bar.
    pub(super) fn active_press_hit_id(
        &self,
        mx: f32,
        my: f32,
        cx: &UiContext<'_>,
    ) -> Option<u64> {
        // Same precedence as `UiFrame::click` / `UiFrame::hover`:
        // Overlay-tier popups first, then Base-tier modals, then bars.
        // Without this gate `capture_active_press_hit_id` would record
        // a top-bar press even when a popup or modal owns the click —
        // the mouse-down bypasses dispatch order, so the cache hash
        // ends up referencing a hit_id underneath the modal and the
        // base-layer element renders a phantom `.active()` tint until
        // mouse-up clears it. Overlay-tier widgets manage their own
        // press affordances internally; we don't expose hit_ids here.
        if self.context_menu.is_some() || self.palette.is_some() {
            return None;
        }
        // Settings panel opacity steppers — the only press-friendly
        // controls in any of the Base-tier modals (panel stays open
        // after each nudge so the press state has a frame to render).
        // Close button and "Open settings.toml" dismiss the panel
        // immediately, so per-frame press tint isn't worth threading.
        // Returning early when `settings_panel` is visible also
        // suppresses spurious top-bar press tint that would otherwise
        // bleed through the panel's translucent backdrop.
        if let Some(panel) = &self.settings_panel {
            return match panel.hit_test(mx, my, cx) {
                UiSettingsHit::PaneOpacityDec => Some(super::settings_panel::HIT_OPACITY_DEC),
                UiSettingsHit::PaneOpacityInc => Some(super::settings_panel::HIT_OPACITY_INC),
                _ => None,
            };
        }
        // Paste dialog has Paste / Cancel buttons but they dismiss
        // the dialog on click, so press tint per-frame isn't worth
        // threading either — and we still need to suppress top-bar
        // tint so the dialog backdrop doesn't bleed press through it.
        if self.paste_dialog.is_some() {
            return None;
        }
        if self.chrome.top_bar.contains(mx, my) {
            match self.top_bar.hit_test(mx, my, cx) {
                Some(UiTopBarHit::PaneTab(pane_id)) => {
                    return Some(super::top_bar::pane_tab_hit_id(pane_id));
                }
                Some(UiTopBarHit::Session) => return Some(super::top_bar::HIT_SESSION),
                Some(UiTopBarHit::Workspace) => return Some(super::top_bar::HIT_WORKSPACE),
                _ => {}
            }
        }
        if let (Some(tab_bar), Some(rect)) = (&self.side_tab_bar, self.chrome.side_tab_bar)
            && rect.contains(mx, my)
            && let Some(UiAction::FocusPaneTab(pane_id)) = tab_bar.hit(rect, mx, my, cx)
        {
            return Some(super::tab_bar::pane_tab_hit_id(pane_id));
        }
        None
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
