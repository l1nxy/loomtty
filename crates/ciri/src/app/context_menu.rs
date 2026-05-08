use super::{App, ContextMenu, ContextMenuAction, ContextMenuItem};
use ciri_protocol::message::ClientMessage;

impl App {
    pub(crate) fn handle_context_menu_click(&mut self, idx: usize) {
        let target_pane_id = self.core.context_menu.target_pane_id;
        if let Some(item) = self.core.context_menu.items.get(idx).cloned()
            && item.enabled
        {
            if let Some(pane_id) = target_pane_id {
                self.send(ClientMessage::FocusPane { pane_id });
                self.remember_workspace_pane(self.core.workspaces.active_workspace_idx, pane_id);
            }
            match &item.action {
                ContextMenuAction::Copy => {
                    if let Some(text) = self.extract_selected_text()
                        && let Some(cb) = &mut self.clipboard
                    {
                        let _ = cb.set_text(&text);
                    }
                }
                ContextMenuAction::Paste => {
                    if let Some(cb) = &mut self.clipboard
                        && let Ok(text) = cb.get_text()
                    {
                        let threshold = self.core.config.terminal.paste_warn_threshold;
                        if let Some(info) = super::paste_guard::check_paste_size(&text, threshold) {
                            let preview = if text.len() > 200 {
                                format!("{}...", &text[..text.floor_char_boundary(200)])
                            } else {
                                text.clone()
                            };
                            let preview = preview.replace('\n', " \\n ").replace('\r', "");
                            self.core.pending_paste = Some(super::PendingPaste {
                                info,
                                preview,
                                target: super::PendingPasteTarget::Terminal,
                            });
                        } else if let Some(pid) = self.core.workspaces.active_mut().active_pane_id()
                        {
                            let bracketed = self.core.pane_grids.get(&pid).is_some_and(|g| {
                                g.mode_flags & ciri_protocol::message::MODE_BRACKETED_PASTE != 0
                            });
                            let mut data =
                                Vec::with_capacity(text.len() + if bracketed { 12 } else { 0 });
                            if bracketed {
                                data.extend_from_slice(b"\x1b[200~");
                            }
                            data.extend_from_slice(text.as_bytes());
                            if bracketed {
                                data.extend_from_slice(b"\x1b[201~");
                            }
                            self.send(ClientMessage::Input {
                                pane_id: pid,
                                data,
                                input_seq: 0,
                            });
                        }
                    }
                }
                ContextMenuAction::SelectAll => {
                    if let Some(pid) =
                        target_pane_id.or(self.core.workspaces.active().active_pane_id())
                        && let Some(grid) = self.core.pane_grids.get(&pid)
                    {
                        let total = grid.total_lines();
                        let cols = grid.cols;
                        self.core.selection = Some(super::Selection {
                            pane_id: pid,
                            start: (0, 0),
                            end: (cols.saturating_sub(1), total.saturating_sub(1)),
                            active: false,
                        });
                    }
                }
                ContextMenuAction::Search => {
                    // Route through `AppModel::open_search_for_pane`
                    // so the close-others discipline (palette /
                    // settings panel / paste dialog) lands on this
                    // path too — inlining the construction would
                    // diverge from the keyboard `OpenSearch` entry
                    // and silently leave peer modals open.
                    if let Some(pane_id) =
                        target_pane_id.or(self.core.workspaces.active().active_pane_id())
                    {
                        self.core.open_search_for_pane(pane_id);
                    }
                }
                ContextMenuAction::OpenLink(url) => self.open_url(url),
                ContextMenuAction::CopyLink(url) => {
                    if let Some(cb) = &mut self.clipboard {
                        let _ = cb.set_text(url);
                    }
                }
                ContextMenuAction::SplitRight => self.send(ClientMessage::CreatePane),
                ContextMenuAction::SplitDown => self.send(ClientMessage::SplitDown),
                ContextMenuAction::ClosePane => {
                    if let Some(pane_id) =
                        target_pane_id.or(self.core.workspaces.active_mut().active_pane_id())
                    {
                        self.send(ClientMessage::ClosePane { pane_id });
                    }
                }
                ContextMenuAction::SetThemePreset(name) => {
                    self.apply_theme_preset(name.clone());
                }
            }
        }
        self.core.context_menu.visible = false;
    }

    /// Apply a chrome theme preset live: mutate `core.config.theme`,
    /// resolve the preset to fill any unset fields, refresh the cached
    /// terminal colour table + chrome `ResolvedTheme`, and request a
    /// repaint. Mirrors the relevant subset of `reload_config`'s post-
    /// load pipeline (font / image checks aren't needed here — only the
    /// theme palette changes).
    ///
    /// v1: in-memory only. The change is NOT written back to
    /// `settings.toml`; users persist via the panel's
    /// "Open settings.toml" link. The Banner makes that explicit.
    pub(crate) fn apply_theme_preset(&mut self, preset: String) {
        // Replace `theme` so the previous preset's fallback fills don't
        // leak into the new preset (e.g. switching dracula → ciri_dark
        // shouldn't keep dracula's accent if the user hasn't overridden
        // it). `resolve_preset` repopulates from the new preset's TOML.
        self.core.config.theme = ciri_config::theme::ThemeConfig {
            preset,
            ..ciri_config::theme::ThemeConfig::default()
        };
        self.core.config.theme.resolve_preset();
        self.cached_color_table = ciri_render::terminal::ColorTable::new(&self.core.config);
        self.cached_resolved_theme.reload(&self.core.config.theme);
        // Pane tile caches embed colour values derived from the old
        // theme — without clearing them the new chrome shows but the
        // pane area renders stale until some other event dirties it.
        // Mirrors `reload_config`'s post-load pipeline.
        self.clear_render_caches();
        self.schedule_redraw();
    }

    pub(crate) fn open_context_menu(&mut self, mx: f32, my: f32) {
        // Settings panel owns the modal layer while visible — refuse to
        // spawn a pane-context menu underneath / over it. Right-click on
        // the settings backdrop is a no-op (the panel's hit_test
        // explicitly returns None for it; this guard makes mouse-down
        // routing match).
        if self.core.settings_panel_visible {
            return;
        }
        if self.core.context_menu.visible {
            self.core.context_menu.visible = false;
            self.schedule_redraw();
            return;
        }

        if let Some((pane_id, col, row)) = self.pixel_to_viewport_cell(mx, my)
            && self.pane_prefers_mouse_passthrough(pane_id)
        {
            self.core.selection = None;
            self.send_lossy(ClientMessage::MouseInput {
                pane_id,
                button: 2,
                col,
                row,
                pressed: true,
                modifiers: 0,
            });
            return;
        }

        let mut items = Vec::new();
        let has_selection = self
            .core
            .selection
            .as_ref()
            .is_some_and(|s| s.start != s.end);

        items.push(ContextMenuItem {
            label: "Copy".to_string(),
            action: ContextMenuAction::Copy,
            enabled: has_selection,
        });
        items.push(ContextMenuItem {
            label: "Paste".to_string(),
            action: ContextMenuAction::Paste,
            enabled: true,
        });
        items.push(ContextMenuItem {
            label: "Select All".to_string(),
            action: ContextMenuAction::SelectAll,
            enabled: true,
        });
        items.push(ContextMenuItem {
            label: "Search".to_string(),
            action: ContextMenuAction::Search,
            enabled: true,
        });

        if let Some((pane_id, col, buf_row)) = self.pixel_to_cell(mx, my)
            && let Some(grid) = self.core.pane_grids.get(&pane_id)
            && let Some(link) = grid.link_at(col, buf_row)
        {
            items.push(ContextMenuItem {
                label: "Open Link".to_string(),
                action: ContextMenuAction::OpenLink(link.url.clone()),
                enabled: true,
            });
            items.push(ContextMenuItem {
                label: "Copy Link".to_string(),
                action: ContextMenuAction::CopyLink(link.url),
                enabled: true,
            });
        }

        items.push(ContextMenuItem {
            label: "\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}\u{2500}"
                .to_string(),
            action: ContextMenuAction::Copy,
            enabled: false,
        });
        items.push(ContextMenuItem {
            label: "Split Right".to_string(),
            action: ContextMenuAction::SplitRight,
            enabled: true,
        });
        items.push(ContextMenuItem {
            label: "Split Down".to_string(),
            action: ContextMenuAction::SplitDown,
            enabled: true,
        });
        items.push(ContextMenuItem {
            label: "Close Pane".to_string(),
            action: ContextMenuAction::ClosePane,
            enabled: true,
        });

        self.core.context_menu = ContextMenu {
            visible: true,
            x: mx,
            y: my,
            target_pane_id: self.pixel_to_cell(mx, my).map(|(pane_id, _, _)| pane_id),
            items,
        };

        self.schedule_redraw();
    }
}
