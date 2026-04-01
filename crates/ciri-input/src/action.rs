/// Actions triggered by keybindings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// New column to the right in the current workspace.
    NewColumnRight,
    /// New workspace below with a new pane.
    NewWorkspaceBelow,
    ClosePane,
    /// h/l: navigate columns within the current workspace.
    FocusLeft,
    FocusRight,
    /// j/k: navigate between workspaces.
    FocusDown,
    FocusUp,
    /// Shift+H/L: move column position within the workspace.
    MovePaneLeft,
    MovePaneRight,
    /// Cycle forward through configured width presets.
    CyclePresetWidth,
    /// Cycle backward through configured width presets.
    CyclePresetWidthReverse,
    /// Set column to one-third viewport width.
    ColumnWidthOneThird,
    /// Set column to half viewport width.
    ColumnWidthHalf,
    /// Set column to two-thirds viewport width.
    ColumnWidthTwoThirds,
    /// Set column to full viewport width.
    ColumnWidthFull,
    /// Increase the active column's width proportion.
    ColumnWidthIncrease,
    /// Decrease the active column's width proportion.
    ColumnWidthDecrease,
    /// Equalize active column and its right neighbor to the same width.
    EqualizeAdjacentColumns,
    /// Consume the right neighbor column's active pane into the current column as a new tile.
    ConsumeIntoColumn,
    /// Expel the current column's active tile into a new column to the right.
    ExpelFromColumn,
    /// Toggle broadcast mode: send input to all panes in current workspace.
    ToggleBroadcast,
    /// Switch to workspace by index (0-8).
    SwitchWorkspace(usize),
    ToggleOverview,
    /// Exit overview mode (return to normal).
    ExitOverview,
    SendLeaderKey,
    ScrollPageUp,
    ScrollPageDown,
    ScrollHalfPageUp,
    ScrollHalfPageDown,
    ScrollLineUp,
    ScrollLineDown,
    ScrollTop,
    ScrollBottom,
    /// Detach from session (close client, server keeps running).
    Detach,
    /// Toggle the command palette overlay.
    ToggleCommandPalette,
    /// Enter a named key table (e.g. "resize").
    EnterMode(String),
    /// Toggle locked mode (all keys pass through to terminal).
    ToggleLock,
    /// Cycle to the next background connection slot (local ↔ remote).
    NextSlot,

    // ── Search mode ──
    /// Open the search bar.
    OpenSearch,
    /// Close the search bar and restore scroll.
    CloseSearch,
    /// Jump to the next search match.
    SearchNextMatch,
    /// Jump to the previous search match.
    SearchPrevMatch,

    // ── Command palette ──
    /// Close the command palette.
    CloseCommandPalette,
    /// Move selection up in the palette.
    PaletteUp,
    /// Move selection down in the palette.
    PaletteDown,
    /// Execute the selected palette entry.
    PaletteConfirm,

    // ── Clipboard ──
    /// Copy the current selection to clipboard.
    ClipboardCopy,
    /// Paste from clipboard.
    ClipboardPaste,

    // ── Paste confirmation ──
    /// Confirm a pending paste operation.
    ConfirmPaste,
    /// Dismiss the paste confirmation dialog.
    DismissPasteConfirm,

    // ── Text input (for search/palette query) ──
    /// Forward the key character to the active text input buffer.
    TextInput,
    /// Delete the last character from the active text input buffer.
    TextBackspace,

    // ── Key table management ──
    /// Push a named key table onto the stack (e.g. "resize").
    ActivateKeyTable(String),
    /// Pop the current key table from the stack.
    DeactivateKeyTable,
}

impl Action {
    /// Whether this action should keep sticky leader mode active.
    /// Navigation and width adjustment are repeatable; everything else
    /// (new pane, close, detach, mode toggles) exits leader mode.
    pub fn is_repeatable(&self) -> bool {
        matches!(
            self,
            Action::FocusLeft
                | Action::FocusRight
                | Action::FocusUp
                | Action::FocusDown
                | Action::MovePaneLeft
                | Action::MovePaneRight
                | Action::CyclePresetWidth
                | Action::CyclePresetWidthReverse
                | Action::ColumnWidthOneThird
                | Action::ColumnWidthHalf
                | Action::ColumnWidthTwoThirds
                | Action::ColumnWidthFull
                | Action::ColumnWidthIncrease
                | Action::ColumnWidthDecrease
                | Action::EqualizeAdjacentColumns
                | Action::ScrollPageUp
                | Action::ScrollPageDown
                | Action::ScrollHalfPageUp
                | Action::ScrollHalfPageDown
                | Action::ScrollLineUp
                | Action::ScrollLineDown
                | Action::ScrollTop
                | Action::ScrollBottom
                | Action::SwitchWorkspace(_)
        )
    }
}

impl Action {
    /// Parse an action from its string name (used for config-driven keybindings).
    pub fn from_name(s: &str) -> Option<Action> {
        parse_named_action(s).or_else(|| parse_dynamic_action(s))
    }

    /// Return all actions with human-readable labels for the command palette.
    /// Excludes internal-only actions (SendLeaderKey, ExitOverview).
    pub fn all_with_labels() -> Vec<(Action, &'static str)> {
        vec![
            (Action::NewColumnRight, "New Column Right"),
            (Action::NewWorkspaceBelow, "New Workspace Below"),
            (Action::ClosePane, "Close Pane"),
            (Action::FocusLeft, "Focus Left"),
            (Action::FocusRight, "Focus Right"),
            (Action::FocusUp, "Focus Up"),
            (Action::FocusDown, "Focus Down"),
            (Action::MovePaneLeft, "Move Pane Left"),
            (Action::MovePaneRight, "Move Pane Right"),
            (Action::CyclePresetWidth, "Cycle Column Width"),
            (
                Action::CyclePresetWidthReverse,
                "Cycle Column Width (Reverse)",
            ),
            (Action::ColumnWidthOneThird, "Column Width 1/3"),
            (Action::ColumnWidthHalf, "Column Width 1/2"),
            (Action::ColumnWidthTwoThirds, "Column Width 2/3"),
            (Action::ColumnWidthFull, "Column Width Full"),
            (Action::ColumnWidthIncrease, "Increase Column Width"),
            (Action::ColumnWidthDecrease, "Decrease Column Width"),
            (Action::EqualizeAdjacentColumns, "Equalize Adjacent Columns"),
            (Action::ConsumeIntoColumn, "Consume Into Column"),
            (Action::ExpelFromColumn, "Expel From Column"),
            (Action::ToggleBroadcast, "Toggle Broadcast"),
            (Action::ToggleOverview, "Toggle Overview"),
            (Action::ScrollPageUp, "Scroll Page Up"),
            (Action::ScrollPageDown, "Scroll Page Down"),
            (Action::ScrollHalfPageUp, "Scroll Half Page Up"),
            (Action::ScrollHalfPageDown, "Scroll Half Page Down"),
            (Action::ScrollLineUp, "Scroll Line Up"),
            (Action::ScrollLineDown, "Scroll Line Down"),
            (Action::ScrollTop, "Scroll to Top"),
            (Action::ScrollBottom, "Scroll to Bottom"),
            (Action::Detach, "Detach"),
            (Action::ToggleCommandPalette, "Toggle Command Palette"),
            (Action::ToggleLock, "Toggle Lock"),
            (Action::NextSlot, "Next Connection Slot"),
            (Action::OpenSearch, "Open Search"),
            (Action::CloseSearch, "Close Search"),
            (Action::ClipboardCopy, "Copy"),
            (Action::ClipboardPaste, "Paste"),
        ]
    }
}

fn parse_named_action(name: &str) -> Option<Action> {
    match name {
        "new_column_right" => Some(Action::NewColumnRight),
        "split_down" | "new_row_below" | "new_workspace_below" => Some(Action::NewWorkspaceBelow),
        "close_pane" => Some(Action::ClosePane),
        "focus_left" => Some(Action::FocusLeft),
        "focus_right" => Some(Action::FocusRight),
        "focus_down" => Some(Action::FocusDown),
        "focus_up" => Some(Action::FocusUp),
        "move_pane_left" => Some(Action::MovePaneLeft),
        "move_pane_right" => Some(Action::MovePaneRight),
        "cycle_preset_width" => Some(Action::CyclePresetWidth),
        "cycle_preset_width_reverse" => Some(Action::CyclePresetWidthReverse),
        "column_width_one_third" => Some(Action::ColumnWidthOneThird),
        "column_width_half" => Some(Action::ColumnWidthHalf),
        "column_width_two_thirds" => Some(Action::ColumnWidthTwoThirds),
        "column_width_full" => Some(Action::ColumnWidthFull),
        "column_width_increase" => Some(Action::ColumnWidthIncrease),
        "column_width_decrease" => Some(Action::ColumnWidthDecrease),
        "equalize_adjacent_columns" => Some(Action::EqualizeAdjacentColumns),
        "consume_into_column" => Some(Action::ConsumeIntoColumn),
        "expel_from_column" => Some(Action::ExpelFromColumn),
        "toggle_broadcast" => Some(Action::ToggleBroadcast),
        "toggle_overview" => Some(Action::ToggleOverview),
        "exit_overview" => Some(Action::ExitOverview),
        "send_leader_key" => Some(Action::SendLeaderKey),
        "scroll_page_up" => Some(Action::ScrollPageUp),
        "scroll_page_down" => Some(Action::ScrollPageDown),
        "scroll_half_page_up" => Some(Action::ScrollHalfPageUp),
        "scroll_half_page_down" => Some(Action::ScrollHalfPageDown),
        "scroll_line_up" => Some(Action::ScrollLineUp),
        "scroll_line_down" => Some(Action::ScrollLineDown),
        "scroll_top" => Some(Action::ScrollTop),
        "scroll_bottom" => Some(Action::ScrollBottom),
        "detach" => Some(Action::Detach),
        "toggle_command_palette" => Some(Action::ToggleCommandPalette),
        "toggle_lock" => Some(Action::ToggleLock),
        "next_slot" => Some(Action::NextSlot),
        "open_search" => Some(Action::OpenSearch),
        "close_search" => Some(Action::CloseSearch),
        "search_next_match" => Some(Action::SearchNextMatch),
        "search_prev_match" => Some(Action::SearchPrevMatch),
        "close_command_palette" => Some(Action::CloseCommandPalette),
        "palette_up" => Some(Action::PaletteUp),
        "palette_down" => Some(Action::PaletteDown),
        "palette_confirm" => Some(Action::PaletteConfirm),
        "clipboard_copy" => Some(Action::ClipboardCopy),
        "clipboard_paste" => Some(Action::ClipboardPaste),
        "confirm_paste" => Some(Action::ConfirmPaste),
        "dismiss_paste_confirm" => Some(Action::DismissPasteConfirm),
        "text_input" => Some(Action::TextInput),
        "text_backspace" => Some(Action::TextBackspace),
        "deactivate_key_table" => Some(Action::DeactivateKeyTable),
        _ => None,
    }
}

fn parse_dynamic_action(name: &str) -> Option<Action> {
    parse_switch_workspace(name)
        .or_else(|| parse_enter_mode(name))
        .or_else(|| parse_activate_key_table(name))
}

fn parse_switch_workspace(name: &str) -> Option<Action> {
    let rest = name.strip_prefix("switch_workspace_")?;
    let workspace = rest.parse::<usize>().ok()?;
    Some(Action::SwitchWorkspace(workspace))
}

fn parse_enter_mode(name: &str) -> Option<Action> {
    let mode = name.strip_prefix("enter_mode:")?;
    if mode.is_empty() {
        return None;
    }
    Some(Action::EnterMode(mode.to_string()))
}

fn parse_activate_key_table(name: &str) -> Option<Action> {
    let table = name.strip_prefix("activate_key_table:")?;
    if table.is_empty() {
        return None;
    }
    Some(Action::ActivateKeyTable(table.to_string()))
}

#[cfg(test)]
mod tests {
    use super::Action;

    #[test]
    fn parses_named_actions_and_aliases() {
        assert_eq!(
            Action::from_name("new_column_right"),
            Some(Action::NewColumnRight)
        );
        assert_eq!(
            Action::from_name("new_workspace_below"),
            Some(Action::NewWorkspaceBelow)
        );
        assert_eq!(
            Action::from_name("split_down"),
            Some(Action::NewWorkspaceBelow)
        );
        assert_eq!(
            Action::from_name("new_row_below"),
            Some(Action::NewWorkspaceBelow)
        );
        assert_eq!(Action::from_name("toggle_lock"), Some(Action::ToggleLock));
    }

    #[test]
    fn parses_dynamic_actions() {
        assert_eq!(
            Action::from_name("switch_workspace_7"),
            Some(Action::SwitchWorkspace(7))
        );
        assert_eq!(
            Action::from_name("enter_mode:resize"),
            Some(Action::EnterMode("resize".into()))
        );
    }

    #[test]
    fn rejects_invalid_dynamic_actions() {
        assert_eq!(Action::from_name("switch_workspace_x"), None);
        assert_eq!(Action::from_name("enter_mode:"), None);
        assert_eq!(Action::from_name("unknown_action"), None);
    }
}
