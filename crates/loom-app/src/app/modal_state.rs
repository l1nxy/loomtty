use super::{AppModel, PendingPasteTarget};

/// Modal-tier UI states that compete for input focus and z-order.
///
/// Variants carry payload where the close-peers behaviour depends on
/// a relationship the bare kind can't express:
///
/// * `PendingPaste(target)` — terminal-target dialogs are peers of
///   palette/settings/etc; palette/search-target dialogs are
///   *overlays* layered on top of an underlying palette/search and
///   must NOT close the underlying state.
///
/// * `ContextMenu(parent)` — a free-standing right-click menu is a
///   peer of settings; the theme-dropdown reuses `context_menu` as a
///   submodal anchored under settings, and must NOT close its parent
///   panel.
///
/// Adding a new variant: pick the kept set in `kept_set` (which
/// modal fields survive). The structural test exercises every variant
/// and asserts the per-field clear/preserve behaviour matches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalKind {
    /// Close every modal. Used by paths that go dark without opening
    /// another modal (Focused(false), reconnect, slot switch,
    /// overview entry, ESC out of settings).
    None,
    CommandPalette,
    SessionPalette,
    Settings,
    /// Read-only keybindings help overlay.
    Help,
    PendingPaste(PendingPasteTarget),
    ContextMenu(ContextMenuParent),
    Search,
}

/// Whether a `context_menu` is opening as a free-standing modal or
/// as a submodal nested under another modal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextMenuParent {
    /// Pane right-click menu — peer of every other modal.
    Standalone,
    /// Theme dropdown anchored under the settings panel — submodal
    /// of `Settings`. Opening it must not close the panel.
    OverSettings,
}

/// Discriminator for "which AppModel modal field is this." Used by
/// `kept_set` to express which fields survive a `ModalKind` open
/// without iterating string-typed names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalField {
    Palette,
    Settings,
    Help,
    PendingPaste,
    ContextMenu,
    Search,
}

/// The set of modal fields that survive when entering `keep`. Every
/// other field is cleared by the helper. This is the single source
/// of truth for the close-peers contract; both the AppModel-level
/// helper (modal Option/bool fields) and the App-level wrapper
/// (`search_state` via the scroll-restore path) consult it.
pub fn kept_set(keep: ModalKind) -> &'static [ModalField] {
    use ContextMenuParent as P;
    use ModalField as F;
    use PendingPasteTarget as T;
    match keep {
        ModalKind::None => &[],
        ModalKind::CommandPalette | ModalKind::SessionPalette => &[F::Palette],
        ModalKind::Settings => &[F::Settings],
        ModalKind::Help => &[F::Help],
        ModalKind::PendingPaste(T::Terminal) => &[F::PendingPaste],
        ModalKind::PendingPaste(T::CommandPalette) => &[F::PendingPaste, F::Palette],
        ModalKind::PendingPaste(T::Search) => &[F::PendingPaste, F::Search],
        ModalKind::ContextMenu(P::Standalone) => &[F::ContextMenu],
        ModalKind::ContextMenu(P::OverSettings) => &[F::ContextMenu, F::Settings],
        ModalKind::Search => &[F::Search],
    }
}

impl AppModel {
    /// Close every modal-tier UI state except those in `kept_set(keep)`.
    /// Pure data layer: touches modal Option/bool fields only.
    /// `search_state` is App-level (the App wrapper handles it through
    /// `close_search_restore_scroll`).
    pub fn enter_modal_close_peers_core(&mut self, keep: ModalKind) {
        let kept = kept_set(keep);
        if !kept.contains(&ModalField::Palette) {
            self.command_palette = None;
        }
        if !kept.contains(&ModalField::Settings) {
            self.settings_panel_visible = false;
        }
        if !kept.contains(&ModalField::Help) {
            self.help_visible = false;
        }
        if !kept.contains(&ModalField::PendingPaste) {
            self.pending_paste = None;
        }
        if !kept.contains(&ModalField::ContextMenu) {
            self.context_menu.visible = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{CommandPaletteState, ContextMenu, PendingPaste, PendingPasteTarget};
    use crate::paste_guard::PasteInfo;
    use loom_config::config::LoomConfig;

    fn fixture() -> AppModel {
        let mut m = AppModel::new(LoomConfig::default(), "test");
        m.command_palette = Some(CommandPaletteState {
            query: String::new(),
            entries: Vec::new(),
            filtered: Vec::new(),
            selected_idx: 0,
            sessions_only: false,
            remote_loading: None,
            remote_error: None,
            remote_input_mode: false,
        });
        m.settings_panel_visible = true;
        m.help_visible = true;
        m.context_menu = ContextMenu {
            visible: true,
            x: 0.0,
            y: 0.0,
            target_pane_id: None,
            items: Vec::new(),
        };
        m.pending_paste = Some(PendingPaste {
            info: PasteInfo {
                text: String::new(),
                size: 0,
                line_count: 0,
            },
            preview: String::new(),
            target: PendingPasteTarget::Terminal,
        });
        m
    }

    /// Pin the close-peers contract for KNOWN variants and KNOWN
    /// modal fields. Each variant clears every peer (a field NOT in
    /// `kept_set`) and preserves every kept field.
    ///
    /// What this DOES NOT catch (Rust has no struct reflection):
    /// adding a brand-new modal field without updating `kept_set`,
    /// the helper, or the fixture passes silently. The honest
    /// invariant: "for the fields and variants present in this
    /// module, the helper behaves correctly." The mitigation is
    /// that the helper, the enum, `kept_set`, and this test all sit
    /// in one ~100-line file: a developer adding a modal must touch
    /// this module, and PR review covers the rest.
    #[test]
    fn enter_modal_close_peers_core_clears_peers_and_preserves_kept() {
        let cases: &[ModalKind] = &[
            ModalKind::None,
            ModalKind::CommandPalette,
            ModalKind::SessionPalette,
            ModalKind::Settings,
            ModalKind::Help,
            ModalKind::PendingPaste(PendingPasteTarget::Terminal),
            ModalKind::PendingPaste(PendingPasteTarget::CommandPalette),
            ModalKind::PendingPaste(PendingPasteTarget::Search),
            ModalKind::ContextMenu(ContextMenuParent::Standalone),
            ModalKind::ContextMenu(ContextMenuParent::OverSettings),
            ModalKind::Search,
        ];
        for &keep in cases {
            let mut m = fixture();
            m.enter_modal_close_peers_core(keep);
            let kept = kept_set(keep);

            assert_eq!(
                m.command_palette.is_some(),
                kept.contains(&ModalField::Palette),
                "{keep:?}: command_palette",
            );
            assert_eq!(
                m.settings_panel_visible,
                kept.contains(&ModalField::Settings),
                "{keep:?}: settings_panel_visible",
            );
            assert_eq!(
                m.help_visible,
                kept.contains(&ModalField::Help),
                "{keep:?}: help_visible",
            );
            assert_eq!(
                m.context_menu.visible,
                kept.contains(&ModalField::ContextMenu),
                "{keep:?}: context_menu.visible",
            );
            assert_eq!(
                m.pending_paste.is_some(),
                kept.contains(&ModalField::PendingPaste),
                "{keep:?}: pending_paste",
            );
        }
    }

    /// Specific layered cases: an overlay-target paste preserves the
    /// underlying palette/search, and a settings-anchored
    /// context_menu preserves settings. Without this assertion the
    /// general test could degenerate (e.g. `kept_set` accidentally
    /// returning `[]` for these variants would still pass the
    /// general "kept fields preserved" check vacuously).
    #[test]
    fn layered_kinds_preserve_their_underlying_modal() {
        // PendingPaste over CommandPalette must keep the palette
        // alive (the user is pasting INTO it).
        let mut m = fixture();
        m.enter_modal_close_peers_core(ModalKind::PendingPaste(PendingPasteTarget::CommandPalette));
        assert!(
            m.command_palette.is_some(),
            "paste-over-palette must NOT close the underlying palette",
        );
        assert!(m.pending_paste.is_some());

        // ContextMenu(OverSettings) must keep settings alive (theme
        // dropdown is a child of the panel).
        let mut m = fixture();
        m.enter_modal_close_peers_core(ModalKind::ContextMenu(ContextMenuParent::OverSettings));
        assert!(
            m.settings_panel_visible,
            "theme dropdown must NOT close its parent settings panel",
        );
        assert!(m.context_menu.visible);
    }
}
