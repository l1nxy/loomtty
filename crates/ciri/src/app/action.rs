//! Action dispatch: maps `Action` enum variants to App method calls.

use ciri_config::config::CiriConfig;
use ciri_input::action::Action;
use ciri_input::keybind::{BindingSet, KeybindMap};
use ciri_input::leader::InputHandler;
use ciri_protocol::message::*;

use super::App;

impl App {
    /// Build unified BindingSet from all config sources and install it.
    pub(crate) fn rebuild_binding_set(input: &mut InputHandler, config: &CiriConfig) {
        input.set_binding_set(BindingSet::from_legacy(
            &input.keybinds,
            &input.direct_keybinds,
            &input.mode_keybinds,
            &KeybindMap::from_overview_config(&config.keys.overview_bindings),
            &config.keys.search_bindings,
            &config.keys.palette_bindings,
            &config.keys.paste_confirm_bindings,
        ));
    }

    pub fn handle_action(&mut self, action: Action) {
        match action {
            Action::NewColumnRight => {
                self.send(ClientMessage::CreatePane);
            }
            Action::NewWorkspaceBelow => {
                self.send(ClientMessage::SplitDown);
            }
            Action::ClosePane => {
                if let Some(pane_id) = self.core.workspaces.active_mut().active_pane_id() {
                    self.send(ClientMessage::ClosePane { pane_id });
                }
            }
            Action::FocusLeft => {
                self.send(ClientMessage::FocusLeft);
            }
            Action::FocusRight => {
                self.send(ClientMessage::FocusRight);
            }
            Action::FocusDown => {
                self.send(ClientMessage::FocusDown);
            }
            Action::FocusUp => {
                self.send(ClientMessage::FocusUp);
            }
            Action::MovePaneLeft => {
                self.send(ClientMessage::MovePaneLeft);
            }
            Action::MovePaneRight => {
                self.send(ClientMessage::MovePaneRight);
            }
            Action::CyclePresetWidth | Action::CyclePresetWidthReverse => {
                let reverse = matches!(action, Action::CyclePresetWidthReverse);
                let presets = self.preset_widths();
                if let Some(w) = self
                    .core
                    .workspaces
                    .active_mut()
                    .cycle_preset_width(&presets, reverse)
                {
                    let (proportion, fixed_px) = match w {
                        ciri_layout::column::ColumnWidth::Proportion(p) => (p, None),
                        ciri_layout::column::ColumnWidth::Fixed(px) => {
                            let inner_vw =
                                self.core.workspaces.active().inner_viewport_width() as f64;
                            let p = if inner_vw > 0.0 { px / inner_vw } else { 0.5 };
                            (p, Some(px))
                        }
                    };
                    self.send(ClientMessage::SetColumnWidth {
                        proportion,
                        fixed_px,
                    });
                }
                self.snap_all_col_widths();
                self.animate_to_active();
            }
            Action::ColumnWidthOneThird => {
                self.send(ClientMessage::SetColumnWidth {
                    proportion: 1.0 / 3.0,
                    fixed_px: None,
                });
            }
            Action::ColumnWidthHalf => {
                self.send(ClientMessage::SetColumnWidth {
                    proportion: 0.5,
                    fixed_px: None,
                });
            }
            Action::ColumnWidthTwoThirds => {
                self.send(ClientMessage::SetColumnWidth {
                    proportion: 2.0 / 3.0,
                    fixed_px: None,
                });
            }
            Action::ColumnWidthFull => {
                self.send(ClientMessage::SetColumnWidth {
                    proportion: 1.0,
                    fixed_px: None,
                });
            }
            Action::ColumnWidthIncrease => {
                let ws = self.core.workspaces.active();
                let inner_vw = ws.inner_viewport_width();
                if let Some(col) = ws.columns.get(ws.active_column_idx) {
                    let current = col.proportion(inner_vw);
                    self.send(ClientMessage::SetColumnWidth {
                        proportion: current + 0.05,
                        fixed_px: None,
                    });
                }
            }
            Action::ColumnWidthDecrease => {
                let ws = self.core.workspaces.active();
                let inner_vw = ws.inner_viewport_width();
                if let Some(col) = ws.columns.get(ws.active_column_idx) {
                    let current = col.proportion(inner_vw);
                    self.send(ClientMessage::SetColumnWidth {
                        proportion: (current - 0.05).max(0.05),
                        fixed_px: None,
                    });
                }
            }
            Action::TileHeightIncrease => self.adjust_active_tile_height(1),
            Action::TileHeightDecrease => self.adjust_active_tile_height(-1),
            Action::EqualizeAdjacentColumns => {
                self.send(ClientMessage::EqualizeColumnSplit);
            }
            Action::ToggleBroadcast => {
                self.core.broadcast_mode = !self.core.broadcast_mode;
                log::info!("broadcast mode: {}", self.core.broadcast_mode);
            }
            Action::ConsumeIntoColumn => {
                self.send(ClientMessage::ConsumeIntoColumn);
            }
            Action::ExpelFromColumn => {
                self.send(ClientMessage::ExpelFromColumn);
            }
            Action::ExitOverview => self.exit_overview(),
            Action::SwitchWorkspace(idx) => {
                self.send(ClientMessage::SwitchWorkspace { workspace_idx: idx });
            }
            Action::ToggleOverview => self.toggle_overview(),
            Action::SendLeaderKey => {
                if let Some(pid) = self.core.workspaces.active_mut().active_pane_id() {
                    self.send(ClientMessage::Input {
                        pane_id: pid,
                        data: vec![0x17],
                        input_seq: 0,
                    });
                }
            }
            Action::ScrollPageUp => {
                let rows = self
                    .core
                    .pane_grids
                    .values()
                    .next()
                    .map(|g| g.rows as usize)
                    .unwrap_or(24);
                self.scroll_active_up(rows);
            }
            Action::ScrollPageDown => {
                let rows = self
                    .core
                    .pane_grids
                    .values()
                    .next()
                    .map(|g| g.rows as usize)
                    .unwrap_or(24);
                self.scroll_active_down(rows);
            }
            Action::ScrollHalfPageUp => {
                let rows = self
                    .core
                    .pane_grids
                    .values()
                    .next()
                    .map(|g| (g.rows as usize) / 2)
                    .unwrap_or(12);
                self.scroll_active_up(rows.max(1));
            }
            Action::ScrollHalfPageDown => {
                let rows = self
                    .core
                    .pane_grids
                    .values()
                    .next()
                    .map(|g| (g.rows as usize) / 2)
                    .unwrap_or(12);
                self.scroll_active_down(rows.max(1));
            }
            Action::ScrollLineUp => {
                self.scroll_active_up(1);
            }
            Action::ScrollLineDown => {
                self.scroll_active_down(1);
            }
            Action::ScrollTop => {
                if let Some(pid) = self.core.workspaces.active().active_pane_id()
                    && let Some(grid) = self.core.pane_grids.get_mut(&pid)
                {
                    grid.scroll_up(grid.max_scroll_offset());
                    self.invalidate_pane_cache(pid);
                }
            }
            Action::ScrollBottom => {
                self.scroll_active_to_bottom();
            }
            Action::Detach => {
                self.send(ClientMessage::Detach);
                self.core.should_exit = true;
            }
            Action::ToggleCommandPalette => {
                if self.core.command_palette.is_some() {
                    self.core.command_palette = None;
                } else {
                    // Mirror the close-others discipline `ToggleSettings`
                    // applies in the inverse direction: the palette is
                    // an Overlay-tier modal and must own input focus.
                    // Without this, opening the palette over an open
                    // settings panel leaves the panel collecting clicks
                    // through the popup since `UiFrame::click` walks
                    // settings_panel before palette in `paint_base`
                    // ordering.
                    self.core.settings_panel_visible = false;
                    self.core.context_menu.visible = false;
                    self.open_command_palette();
                }
            }
            Action::ToggleSessionPalette => {
                if self.core.command_palette.is_some() {
                    self.core.command_palette = None;
                } else {
                    self.core.settings_panel_visible = false;
                    self.core.context_menu.visible = false;
                    self.open_session_palette();
                }
            }
            Action::EnterMode(_) => {
                // State transition handled by InputHandler::process_key
            }
            Action::ToggleLock => {
                self.core.input.toggle_lock();
            }
            Action::ToggleSettings => {
                // Close every other modal-ish overlay so the settings
                // panel cleanly owns input focus. Without this, search /
                // context_menu / palette can co-exist with settings and
                // produce confusing keyboard-routing or z-order results
                // (see resize-mode + context_menu bleed-through, fixed
                // earlier the same way).
                self.core.command_palette = None;
                self.core.search_state = None;
                self.core.context_menu.visible = false;
                self.core.settings_panel_visible = !self.core.settings_panel_visible;
            }
            Action::NextSession => {
                self.cycle_session(1);
            }
            Action::PrevSession => {
                self.cycle_session(-1);
            }
            Action::NewSession => {
                let existing: Vec<String> = self
                    .core
                    .cached_local_sessions
                    .iter()
                    .map(|s| s.name.clone())
                    .collect();
                let name = ciri_session::names::unique_name(&existing);
                self.send(ClientMessage::SwitchSession { session_name: name });
            }
            // ── Search ──
            Action::OpenSearch => {
                self.open_search();
            }
            Action::CloseSearch => {
                self.close_search_restore_scroll();
            }
            Action::SearchNextMatch => {
                if self
                    .core
                    .search_state
                    .as_ref()
                    .is_some_and(|s| s.query.is_empty())
                {
                    // Empty query: just exit search
                    self.core.search_state = None;
                } else {
                    self.jump_to_match(false);
                }
            }
            Action::SearchPrevMatch => {
                self.jump_to_match(true);
            }

            // ── Command palette ──
            Action::CloseCommandPalette => {
                // In remote input mode, Escape returns to the normal palette
                if self
                    .core
                    .command_palette
                    .as_ref()
                    .is_some_and(|p| p.remote_input_mode)
                {
                    let sessions_only = self
                        .core
                        .command_palette
                        .as_ref()
                        .is_some_and(|p| p.sessions_only);
                    if sessions_only {
                        self.open_session_palette();
                    } else {
                        self.open_command_palette();
                    }
                } else {
                    self.core.command_palette = None;
                }
            }
            Action::PaletteUp => {
                if let Some(palette) = &mut self.core.command_palette {
                    palette.move_selection(-1, true);
                }
            }
            Action::PaletteDown => {
                if let Some(palette) = &mut self.core.command_palette {
                    palette.move_selection(1, true);
                }
            }
            Action::PaletteConfirm => {
                self.execute_palette_selection();
            }

            // ── Clipboard ──
            Action::ClipboardCopy => {
                self.handle_clipboard_copy();
            }
            Action::ClipboardPaste => {
                self.handle_clipboard_paste();
            }

            // ── Paste confirmation ──
            Action::ConfirmPaste => {
                self.confirm_pending_paste();
            }
            Action::DismissPasteConfirm => {
                self.core.pending_paste = None;
            }

            // ── Text input ──
            Action::TextInput => {
                // Handled in keyboard.rs handle_text_input(); should not reach here.
                log::debug!("TextInput action reached handle_action (unexpected)");
            }
            Action::TextBackspace => {
                let _ = self.pop_text_from_overlay_input();
            }

            // ── Key table management (handled by InputHandler internally) ──
            Action::ActivateKeyTable(_) | Action::DeactivateKeyTable => {
                // State transitions already handled in process_key_v2.
            }
        }
    }

    // ── Search helpers (used by handle_action and keyboard.rs) ──

    pub(super) fn open_search(&mut self) {
        self.core.open_search();
    }

    pub(super) fn close_search_restore_scroll(&mut self) {
        let Some(search) = &self.core.search_state else {
            return;
        };
        let pane_id = search.pane_id;
        let orig = search.original_scroll_offset;
        if let Some(grid) = self.core.pane_grids.get_mut(&pane_id) {
            grid.scroll_offset = orig;
            grid.dirty = true;
            self.invalidate_pane_cache(pane_id);
        }
        self.core.search_state = None;
    }

    /// Grow (`direction > 0`) or shrink (`direction < 0`) the active tile's
    /// height by ~5% of the column's inner height, pairing it with its
    /// downstairs neighbour (or upstairs when the active tile is at the bottom).
    fn adjust_active_tile_height(&mut self, direction: i32) {
        let ws = self.core.workspaces.active();
        let col_idx = ws.active_column_idx;
        let Some(col) = ws.columns.get(col_idx) else {
            return;
        };
        if col.tiles.len() < 2 {
            return;
        }
        let active_tile_idx = col.active_tile_idx;
        let (top_tile_idx, delta_sign) = if active_tile_idx + 1 < col.tiles.len() {
            (active_tile_idx, direction as f32)
        } else {
            (active_tile_idx - 1, -(direction as f32))
        };
        let step = (ws.inner_height() * 0.05).max(10.0);
        let delta_y = delta_sign * step;

        let column_idx = col_idx;
        self.core
            .workspaces
            .active_mut()
            .resize_tile_pair(column_idx, top_tile_idx, delta_y);

        let ws = self.core.workspaces.active();
        if let Some(col) = ws.columns.get(column_idx) {
            let bot_idx = top_tile_idx + 1;
            if bot_idx < col.tiles.len() {
                let top_weight = col.tiles[top_tile_idx].height.weight() as f64;
                let bottom_weight = col.tiles[bot_idx].height.weight() as f64;
                self.send(ClientMessage::SetTileWeights {
                    column_idx,
                    top_tile_idx,
                    top_weight,
                    bottom_weight,
                });
            }
        }
        self.schedule_redraw();
    }

    pub(crate) fn update_search_results(&mut self) {
        let Some(search) = &mut self.core.search_state else {
            return;
        };
        let pane_id = search.pane_id;
        let query = search.query.clone();

        if let Some(grid) = self.core.pane_grids.get(&pane_id) {
            let raw_matches = grid.search(&query);
            search.matches = raw_matches
                .into_iter()
                .map(|(row, sc, ec)| super::SearchMatch {
                    buffer_row: row,
                    start_col: sc,
                    end_col: ec,
                })
                .collect();
            search.current_match_idx = 0;

            if !search.matches.is_empty() {
                let viewport_top = grid.viewport_top();
                let idx = search
                    .matches
                    .iter()
                    .position(|m| m.buffer_row >= viewport_top)
                    .unwrap_or(0);
                search.current_match_idx = idx;
                self.scroll_to_match(idx);
            }
        }
    }

    pub(super) fn jump_to_match(&mut self, reverse: bool) {
        let Some(search) = &mut self.core.search_state else {
            return;
        };
        if search.matches.is_empty() {
            return;
        }
        if reverse {
            search.current_match_idx = if search.current_match_idx == 0 {
                search.matches.len() - 1
            } else {
                search.current_match_idx - 1
            };
        } else {
            search.current_match_idx = (search.current_match_idx + 1) % search.matches.len();
        }
        let idx = search.current_match_idx;
        self.scroll_to_match(idx);
    }

    fn scroll_to_match(&mut self, match_idx: usize) {
        let Some(search) = &self.core.search_state else {
            return;
        };
        let Some(m) = search.matches.get(match_idx) else {
            return;
        };
        let pane_id = search.pane_id;
        let target_row = m.buffer_row;

        if let Some(grid) = self.core.pane_grids.get_mut(&pane_id) {
            let total = grid.buffer_len();
            let rows = grid.rows as usize;
            let desired_top = target_row.saturating_sub(rows / 2);
            let max_scroll = total.saturating_sub(rows);
            let new_offset = max_scroll.saturating_sub(desired_top);
            grid.scroll_offset = new_offset.min(max_scroll);
            grid.dirty = true;
            self.invalidate_pane_cache(pane_id);
        }
    }

    // ── Palette helpers ──

    pub(super) fn execute_palette_selection(&mut self) {
        let Some(palette) = &self.core.command_palette else {
            return;
        };

        // Remote input mode: parse query as user@host[:port] and trigger
        // the async auto-connect flow. The palette stays open in loading
        // state — the query result handler closes it on success or sets
        // `remote_error` on failure.
        if palette.remote_input_mode {
            let query = palette.query.trim().to_string();
            if query.is_empty() {
                return;
            }
            self.connect_remote_from_input(&query);
            return;
        }

        // Don't execute non-selectable entries (section headers)
        let entry = palette
            .filtered
            .get(palette.selected_idx)
            .and_then(|&idx| palette.entries.get(idx));
        let Some(entry) = entry else { return };
        if !entry.kind.is_selectable() {
            return;
        }

        // DirectConnect now fires an async query (auto-connect flow), so
        // keep the palette open in loading state until the result handler
        // closes it on success or surfaces a remote_error.
        let keep_open = matches!(
            entry.kind,
            super::PaletteEntryKind::RemoteHost { .. }
                | super::PaletteEntryKind::DirectConnect { .. }
                | super::PaletteEntryKind::ConnectRemotePrompt
        );
        if let Some(&entry_idx) = palette.filtered.get(palette.selected_idx) {
            self.execute_palette_entry(entry_idx);
        }
        if !keep_open {
            self.core.command_palette = None;
        }
    }
}
