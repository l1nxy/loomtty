use super::frame::{UiFrame, UiFrameHover};
use super::types::{UiAction, UiHoverOutcome};
use crate::app::App;
use loom_app::app::{ContextMenuParent, ModalKind};
use winit::window::CursorIcon;

impl App {
    pub(crate) fn dispatch_ui_click(&mut self, mx: f32, my: f32) -> bool {
        let (action, consumed) = {
            let cx = self.ui_context();
            let frame = UiFrame::capture_current(self, &cx);
            frame.click(self, mx, my, &cx)
        };
        if let Some(action) = action {
            self.apply_ui_action(action);
        }
        consumed
    }

    /// Look up the chrome `hit_id` at `(mx, my)` for press-state
    /// styling. Returns `None` outside of press-friendly chrome.
    /// Called by `handle_mouse_pressed` to populate
    /// `App::active_hit_id` before dispatching the click.
    pub(crate) fn capture_active_press_hit_id(&self, mx: f32, my: f32) -> Option<u64> {
        let cx = self.ui_context();
        let frame = UiFrame::capture_current(self, &cx);
        frame.active_press_hit_id(mx, my, &cx)
    }

    /// Route a middle-mouse click. Currently the only middle-click handler
    /// is tab close — browsers and most tab-bearing apps treat MMB on a
    /// tab as "close this tab", so we mirror that. Modal overlays and the
    /// overview deliberately don't react to middle-click: they're focus
    /// surfaces where MMB has no meaning, and routing it there would swallow
    /// the event when the user expects it to pass through to the underlying
    /// tab strip (e.g. clicking through a dismissible tooltip).
    pub(crate) fn dispatch_ui_middle_click(&mut self, mx: f32, my: f32) -> bool {
        let (action, consumed) = {
            let cx = self.ui_context();
            let frame = UiFrame::capture_current(self, &cx);
            frame.middle_click(mx, my, &cx)
        };
        if let Some(action) = action {
            self.apply_ui_action(action);
        }
        consumed
    }

    pub(crate) fn apply_ui_action(&mut self, action: UiAction) {
        match action {
            UiAction::OpenSessionPalette => self.open_session_palette(),
            UiAction::ToggleOverview => {
                self.handle_action(loom_input::action::Action::ToggleOverview);
            }
            UiAction::CycleWorkspace => {
                let workspace_count = self.core.workspaces.workspaces.len();
                if workspace_count > 0 {
                    let next_idx =
                        (self.core.workspaces.active_workspace_idx + 1) % workspace_count;
                    self.core.workspaces.active_workspace_idx = next_idx;
                    if let Some(&pane_id) = self.core.workspace_last_pane_ids.get(&next_idx)
                        && self.focus_workspace_pane_local(next_idx, pane_id)
                    {
                        self.send(loom_protocol::message::ClientMessage::FocusPane { pane_id });
                    }
                    self.animate_to_active();
                    self.send(loom_protocol::message::ClientMessage::SwitchWorkspace {
                        workspace_idx: next_idx,
                    });
                }
            }
            UiAction::FocusPaneTab(pane_id) => {
                // Re-clicking the already-focused tab is a no-op:
                // skip the `FocusPane` network round-trip and the
                // animate_to_active call so the active-tab `.active()`
                // refinement (Steps 38/40) doesn't generate spurious
                // server traffic per click of the focused tab.
                if self.core.workspaces.active().active_pane_id() == Some(pane_id) {
                    return;
                }
                let mut target: Option<(usize, usize, usize)> = None;
                for (ws_idx, ws) in self.core.workspaces.workspaces.iter().enumerate() {
                    for (col_idx, col) in ws.columns.iter().enumerate() {
                        if col.contains_pane(pane_id) {
                            let tile_idx = col
                                .tiles
                                .iter()
                                .position(|t| t.pane_id == pane_id)
                                .unwrap_or(0);
                            target = Some((ws_idx, col_idx, tile_idx));
                            break;
                        }
                    }
                    if target.is_some() {
                        break;
                    }
                }
                if let Some((ws_idx, col_idx, tile_idx)) = target {
                    self.core.workspaces.active_workspace_idx = ws_idx;
                    let ws = self.core.workspaces.active_mut();
                    ws.active_column_idx = col_idx;
                    if col_idx < ws.columns.len() {
                        ws.columns[col_idx].active_tile_idx =
                            tile_idx.min(ws.columns[col_idx].tiles.len().saturating_sub(1));
                    }
                    self.remember_workspace_pane(ws_idx, pane_id);
                    self.send(loom_protocol::message::ClientMessage::FocusPane { pane_id });
                    self.animate_to_active();
                }
            }
            UiAction::ClosePaneTab(pane_id) => {
                self.send(loom_protocol::message::ClientMessage::ClosePane { pane_id });
            }
            UiAction::ExecutePaletteEntry(entry_idx) => {
                // SectionHeaders should not be clickable (hit_test returns Panel),
                // but guard defensively.
                let is_selectable = self
                    .core
                    .command_palette
                    .as_ref()
                    .and_then(|p| p.entries.get(entry_idx))
                    .is_some_and(|e| e.kind.is_selectable());
                if !is_selectable {
                    return;
                }
                // Mirror the keyboard-enter `keep_open` table in
                // `execute_palette_selection`. DirectConnect now fires an
                // async query and relies on the palette staying open as the
                // cancel anchor — closing it here would make the query-
                // result handler's cancellation gate discard the result.
                let keep_open = self
                    .core
                    .command_palette
                    .as_ref()
                    .and_then(|p| p.entries.get(entry_idx))
                    .is_some_and(|e| {
                        matches!(
                            e.kind,
                            super::super::PaletteEntryKind::RemoteHost { .. }
                                | super::super::PaletteEntryKind::DirectConnect { .. }
                                | super::super::PaletteEntryKind::ConnectRemotePrompt
                        )
                    });
                if let Some(palette) = &mut self.core.command_palette
                    && let Some(pos) = palette.filtered.iter().position(|&idx| idx == entry_idx)
                {
                    palette.selected_idx = pos;
                }
                self.execute_palette_entry(entry_idx);
                // Skip the gate when the executed action ALREADY
                // closed the palette by opening a new modal (mirrors
                // `execute_palette_selection`'s guard — without this,
                // selecting "Settings…" flashes the panel open then
                // immediately back closed).
                if !keep_open && self.core.command_palette.is_some() {
                    self.enter_modal_close_peers(ModalKind::None);
                }
            }
            UiAction::ClosePalette => self.enter_modal_close_peers(ModalKind::None),
            UiAction::ExecuteContextMenuEntry(idx) => {
                self.handle_context_menu_click(idx);
            }
            UiAction::CloseContextMenu => self.core.context_menu.visible = false,
            UiAction::ConfirmPaste => self.confirm_pending_paste(),
            UiAction::CancelPaste => self.core.pending_paste = None,
            UiAction::FocusOverviewPane(ws_idx, pane_id) => {
                self.focus_overview_target(ws_idx, pane_id);
            }
            UiAction::CloseOverviewPane(pane_id) => {
                self.overview_hovered_pane = None;
                self.send(loom_protocol::message::ClientMessage::ClosePane { pane_id });
            }
            UiAction::StartOverviewDrag => {
                self.core.overview.dragging = true;
                self.core.overview.drag_last_pos = self.last_mouse_pos;
            }
            UiAction::CloseSettings => {
                // Routes through the close-peers gate so the theme
                // dropdown (which reuses `context_menu`) and any
                // future settings submodal are torn down with the
                // panel — no per-child cleanup list to maintain.
                self.enter_modal_close_peers(ModalKind::None);
            }
            UiAction::OpenSettingsDropdown(field) => {
                self.open_settings_enum_dropdown(field);
            }
            UiAction::SelectSettingsCategory(cat) => {
                self.select_settings_category(cat);
            }
            UiAction::SettingsControl(payload) => {
                self.apply_settings_control(payload.into_dispatch());
            }
            UiAction::SettingsNoOp => {}
            UiAction::OpenSettingsToml => {
                // v1 escape hatch: open the user's settings.toml so
                // they can persist any change the panel doesn't yet
                // write back. Reuses the existing `open_url`-via-
                // `open_file_path` flow so OS-specific editor
                // resolution ($EDITOR / code / cursor / system handler)
                // is shared with the link-click path.
                //
                // We deliberately do NOT seed the file when missing —
                // that round-tripped through the config-file watcher
                // and clobbered any in-memory live preview the user
                // just made. Instead, fall back to opening the parent
                // directory so the user can create the file themselves
                // without losing their preview state.
                // Use the trusted-path opener — `config_path()` is
                // first-party and safe to hand to the OS handler. The
                // generic `open_url` path refuses file paths without
                // `$EDITOR` set (see `open_file_path` safety gate),
                // which fails for any GUI launch context that doesn't
                // export `$EDITOR` to child processes (common on macOS
                // and many Linux desktops).
                let path = loom_config::config::config_path();
                let to_open: Option<std::path::PathBuf> = if path.exists() {
                    Some(path.clone())
                } else if let Some(parent) = path.parent()
                    && parent.exists()
                {
                    log::info!(
                        "settings.toml does not yet exist; opening parent directory {}",
                        parent.display(),
                    );
                    Some(parent.to_path_buf())
                } else {
                    log::warn!(
                        "settings.toml path unavailable: {} (parent does not exist either)",
                        path.display(),
                    );
                    None
                };
                if let Some(target) = to_open
                    && let Err(e) = crate::app::open::open_trusted_path(&target)
                {
                    log::warn!("failed to open settings location {}: {e}", target.display(),);
                }
                // Same shape as `UiAction::CloseSettings` — the theme
                // dropdown could be alive as a submodal at the moment
                // this fires (defense-in-depth; hit-test order may
                // already prevent it). Route through the gate so any
                // submodal is torn down with the panel rather than
                // floating orphaned over the terminal.
                self.enter_modal_close_peers(ModalKind::None);
            }
        }
    }

    /// Spawn the `context_menu` popup as a settings-anchored dropdown
    /// for an enum-typed field. Items dispatch
    /// `ContextMenuAction::SetSettingsEnum { field_id, value }`; the
    /// theme preset reuses the same path (its `apply_theme_preset`
    /// side-effect chain runs in the dispatcher).
    fn open_settings_enum_dropdown(&mut self, field: super::settings_panel::schema::SettingsField) {
        // Anchor BELOW the cursor so the popup doesn't visually
        // overlap the trigger row's value text. Same 44 px offset as
        // the v1 path — slightly larger than `Dropdown::ROW_H` (40),
        // which gives `anchored()` room to edge-flip upward when the
        // bottom of the viewport would clip.
        //
        // X anchors on the field-control column's horizontal center
        // (not the cursor click X) so the popup's midpoint matches
        // the trigger's midpoint — `context_menu.rs::capture` picks
        // `AnchorCorner::TopCenter` when `settings_panel_visible`, so
        // this point is treated as the top edge midpoint of the popup.
        // Falls back to viewport center if the panel can't be captured
        // (shouldn't happen — the dropdown is only reachable through
        // the panel — but keep the fallback to avoid a panic in tests).
        const POPUP_OFFSET_Y: f32 = 44.0;
        let cursor_y = self
            .last_mouse_pos
            .map(|(_, y)| y)
            .unwrap_or_else(|| self.command_palette_viewport_size().1 * 0.5);
        let trigger_x = {
            let cx = self.ui_context();
            super::settings_panel::SettingsPanelComponent::capture(self, &cx)
                .map(|c| c.control_center_x())
                .unwrap_or_else(|| self.command_palette_viewport_size().0 * 0.5)
        };
        let mx = trigger_x;
        let my = cursor_y + POPUP_OFFSET_Y;
        let current = super::settings_panel::schema::read_enum(field, &self.core.config);
        let variants = super::settings_panel::schema::enum_variants(field);
        let field_id = field.id();
        let items = variants
            .into_iter()
            .map(|name| {
                let active = name == current;
                let label = if active {
                    format!("\u{2713} {}", name)
                } else {
                    format!("  {}", name)
                };
                crate::app::ContextMenuItem {
                    label,
                    action: crate::app::ContextMenuAction::SetSettingsEnum {
                        field_id,
                        value: name,
                    },
                    enabled: true,
                }
            })
            .collect();
        self.enter_modal_close_peers(ModalKind::ContextMenu(ContextMenuParent::OverSettings));
        self.core.context_menu = crate::app::ContextMenu {
            visible: true,
            x: mx,
            y: my,
            target_pane_id: None,
            items,
        };
    }

    pub(crate) fn dispatch_ui_hover(&mut self, mx: f32, my: f32) -> UiHoverOutcome {
        let hover = {
            let cx = self.ui_context();
            let frame = UiFrame::capture_current(self, &cx);
            frame.hover(self, mx, my, &cx)
        };

        match hover {
            UiFrameHover::ContextMenu { hovered } => {
                // Hover index is derived at cache-hash time from
                // `App::current_context_menu_hover()`; display reads it
                // declaratively via `cx.is_hovered(hit_id)`. Same shape
                // as the palette hover handler — set redraw
                // unconditionally and let the cache hit/miss decide.
                UiHoverOutcome {
                    handled: true,
                    cursor: if hovered.is_some() {
                        CursorIcon::Pointer
                    } else {
                        CursorIcon::Default
                    },
                    needs_redraw: true,
                }
            }
            UiFrameHover::PasteDialog { button } => {
                // Hover derived at cache-hash time via
                // `App::current_paste_dialog_hover()`; display reads it
                // declaratively. Same shape as the palette /
                // context_menu handlers — set redraw unconditionally.
                UiHoverOutcome {
                    handled: true,
                    cursor: if button.is_some() {
                        CursorIcon::Pointer
                    } else {
                        CursorIcon::Default
                    },
                    needs_redraw: true,
                }
            }
            UiFrameHover::Palette { pointer } => {
                // Hover index is derived from `last_mouse_pos` at cache-
                // hash time (`App::current_palette_hover`); display pulls
                // it from the walker's `cx.is_hovered(hit_id)` and the
                // declarative `.hover()` style. Nothing model-side to
                // mutate here — request a redraw unconditionally so the
                // cache hash gets recomputed; the chrome cache hit/miss
                // path makes this cheap when the row hasn't changed.
                UiHoverOutcome {
                    handled: true,
                    cursor: if pointer {
                        CursorIcon::Pointer
                    } else {
                        CursorIcon::Default
                    },
                    needs_redraw: true,
                }
            }
            UiFrameHover::Settings { pointer } => {
                // Same shape as Palette: declarative `.hover()` styles
                // on close button / dropdown trigger / steppers /
                // "Open settings.toml" link rely on `cx.is_hovered(...)`
                // refreshing each frame. Request redraw unconditionally
                // and let the chrome cache hash dedupe.
                UiHoverOutcome {
                    handled: true,
                    cursor: if pointer {
                        CursorIcon::Pointer
                    } else {
                        CursorIcon::Default
                    },
                    needs_redraw: true,
                }
            }
            UiFrameHover::TopBar { region, tab } => {
                // Both `region` and `tab` are derived at chrome-cache-
                // hash time now (Step 28). The handler keeps the
                // pointer-cursor signal but stops storing the hover
                // values; cache-key recomputation handles repaint
                // gating, same shape as the palette / context_menu /
                // paste_dialog handlers.
                UiHoverOutcome {
                    handled: true,
                    cursor: if region.is_some() || tab.is_some() {
                        CursorIcon::Pointer
                    } else {
                        CursorIcon::Default
                    },
                    needs_redraw: true,
                }
            }
            UiFrameHover::SideTab { tab } => UiHoverOutcome {
                handled: true,
                cursor: if tab.is_some() {
                    CursorIcon::Pointer
                } else {
                    CursorIcon::Default
                },
                needs_redraw: true,
            },
            UiFrameHover::Overview { target } => {
                // `action_hover` is derived at hash time (Step 26);
                // `target` (which overview tile is hovered) still
                // needs storage on `App.overview_hovered_pane`
                // because the overview action bar geometry depends
                // on it (the bar is anchored to whichever tile the
                // cursor is on).
                self.overview_hovered_pane = target;
                UiHoverOutcome {
                    handled: true,
                    cursor: if target.is_some() {
                        CursorIcon::Pointer
                    } else {
                        CursorIcon::Default
                    },
                    needs_redraw: true,
                }
            }
            UiFrameHover::None => UiHoverOutcome {
                handled: false,
                cursor: CursorIcon::Default,
                needs_redraw: false,
            },
        }
    }
}
