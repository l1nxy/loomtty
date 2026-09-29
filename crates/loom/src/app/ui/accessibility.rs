//! Semantic controls for the GPU overlays, using the same layout and actions
//! as pointer interaction. No native objects or renderer ownership live here.
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};

use super::settings_panel::hit::{ButtonRole, ChromeOp, SettingsHit};
use super::settings_panel::schema::{self, FieldKind};
use super::types::UiAction;
use super::{context_menu, palette, paste_dialog, settings_panel};
use crate::app::App;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Role {
    Button,
    Checkbox,
    Popup,
    TextField,
    StaticText,
    Scrollbar,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Value {
    None,
    Text(String),
    Bool(bool),
    Number { value: usize, max: usize },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Edit {
    Palette,
    Search,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Scroll {
    Settings,
    ContextMenu,
    Palette,
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Node {
    pub id: u64,
    pub identity: String,
    pub label: String,
    pub help: String,
    pub bounds: [f32; 4],
    pub role: Role,
    pub value: Value,
    pub enabled: bool,
    pub selected: bool,
    pub default_focus: bool,
    pub press: Option<UiAction>,
    pub edit: Option<Edit>,
    pub scroll: Option<Scroll>,
}
impl Node {
    pub(super) fn new(id: u64, label: impl Into<String>, bounds: [f32; 4], role: Role) -> Self {
        Self {
            id,
            identity: String::new(),
            label: label.into(),
            help: String::new(),
            bounds,
            role,
            value: Value::None,
            enabled: true,
            selected: false,
            default_focus: false,
            press: None,
            edit: None,
            scroll: None,
        }
    }
    pub fn same_target(&self, other: &Self) -> bool {
        self.id == other.id
            && self.identity == other.identity
            && self.label == other.label
            && self.role == other.role
            && self.press == other.press
            && self.edit == other.edit
            && self.scroll == other.scroll
    }
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Model {
    pub label: String,
    pub identity: u64,
    pub nodes: Vec<Node>,
    pub dismiss: Option<UiAction>,
}
#[derive(Clone, Debug)]
pub(crate) enum Operation {
    Press,
    Focus,
    Text(String),
    Number(usize),
    Step(i32),
    Cancel,
}
fn digest(text: &str) -> u64 {
    let mut h = DefaultHasher::new();
    text.hash(&mut h);
    h.finish()
}
fn bounds(layout: &loom_ui::LayoutSnapshot) -> HashMap<u64, [f32; 4]> {
    layout
        .nodes()
        .iter()
        .filter_map(|n| n.hit_id.map(|id| (id, n.bounds)))
        .collect()
}
fn scrollbar(
    id: u64,
    rect: [f32; 4],
    label: &str,
    value: usize,
    max: usize,
    target: Scroll,
) -> Node {
    let mut node = Node::new(id, label, rect, Role::Scrollbar);
    node.value = Value::Number { value, max };
    node.scroll = Some(target);
    node
}
impl Model {
    pub fn cache_key(app: &App) -> u64 {
        let cx = app.ui_context();
        let mut h = DefaultHasher::new();
        app.ui_scene_hash(
            cx.viewport_w,
            cx.viewport_h,
            cx.cell_w,
            cx.cell_h,
            cx.ui_line_h,
        )
        .hash(&mut h);
        if let Some(search) = &app.core.search_state {
            (
                search.pane_id,
                &search.query,
                search.matches.len(),
                search.current_match_idx,
            )
                .hash(&mut h);
        }
        // These identities can change without changing the painted label or
        // paste preview. Never keep an AX action for an old command/payload.
        if let Some(paste) = &app.core.pending_paste {
            paste.info.text.hash(&mut h);
        }
        if let Some(palette) = &app.core.command_palette {
            for entry in &palette.entries {
                format!("{:?}", entry.kind).hash(&mut h);
            }
        }
        if app.core.context_menu.visible {
            format!("{:?}", app.core.context_menu.items).hash(&mut h);
        }
        h.finish()
    }
    pub fn capture(app: &App) -> Option<Self> {
        let cx = app.ui_context();
        // Match the topmost modal in UiFrame::click. Its covered parent must
        // not remain reachable through the accessibility tree.
        if app.core.help_visible {
            return super::help_overlay::HelpOverlayComponent::capture(app, &cx)
                .map(|help| help.accessibility(&cx));
        }
        if let Some(component) = context_menu::ContextMenuComponent::capture(app, &cx) {
            let layout = bounds(&component.accessibility_layout(&cx));
            let menu = &app.core.context_menu;
            let identity = digest(&format!(
                "context:{:?}:{:?}:{:?}",
                menu.target_pane_id, app.core.settings_panel_visible, menu.items
            ));
            let mut model = Self {
                label: "Menu".into(),
                identity,
                nodes: vec![],
                dismiss: Some(UiAction::CloseContextMenu),
            };
            for (index, item) in menu.items.iter().enumerate() {
                let id = context_menu::HIT_ENTRY_BASE + index as u64;
                if let Some(rect) = layout.get(&id) {
                    let mut node = Node::new(id, &item.label, *rect, Role::Button);
                    node.identity = format!("{:?}", item.action);
                    node.enabled = item.enabled;
                    node.press = Some(UiAction::ExecuteContextMenuEntry(index));
                    model.nodes.push(node);
                }
            }
            if let Some(rect) = layout.get(&context_menu::HIT_SCROLLBAR_TRACK) {
                model.nodes.push(scrollbar(
                    context_menu::HIT_SCROLLBAR_TRACK,
                    *rect,
                    "Menu scroll position",
                    app.core.context_menu_scroll_offset,
                    component.max_scroll_offset(),
                    Scroll::ContextMenu,
                ));
            }
            return Some(model);
        }
        if let Some(component) = paste_dialog::PasteDialogComponent::capture(app, &cx) {
            let layout = bounds(&component.accessibility_layout(&cx));
            let pending = app.core.pending_paste.as_ref()?;
            let identity = digest(&format!(
                "paste:{:?}:{:?}:{}",
                pending.target,
                app.core.workspaces.active().active_pane_id(),
                pending.info.text
            ));
            let mut model = Self {
                label: "Confirm paste".into(),
                identity,
                nodes: vec![],
                dismiss: Some(UiAction::CancelPaste),
            };
            if let Some(rect) = layout.get(&paste_dialog::HIT_DIALOG) {
                let mut node = Node::new(
                    10,
                    format!(
                        "Paste {} bytes, {} lines?",
                        pending.info.size, pending.info.line_count
                    ),
                    *rect,
                    Role::StaticText,
                );
                node.value = Value::Text(pending.preview.clone());
                model.nodes.push(node);
            }
            for (id, label, action) in [
                (paste_dialog::HIT_PASTE, "Paste", UiAction::ConfirmPaste),
                (paste_dialog::HIT_CANCEL, "Cancel", UiAction::CancelPaste),
            ] {
                if let Some(rect) = layout.get(&id) {
                    let mut node = Node::new(id, label, *rect, Role::Button);
                    node.press = Some(action);
                    model.nodes.push(node);
                }
            }
            return Some(model);
        }
        if let Some(component) = palette::PaletteComponent::capture(app, &cx) {
            let layout = bounds(&component.accessibility_layout(&cx));
            let state = app.core.command_palette.as_ref()?;
            let mut model = Self {
                label: if state.remote_input_mode {
                    "Connect to host"
                } else {
                    "Command palette"
                }
                .into(),
                identity: digest(if state.remote_input_mode {
                    "palette-remote"
                } else {
                    "palette"
                }),
                nodes: vec![],
                dismiss: Some(UiAction::ClosePalette),
            };
            if let Some(rect) = layout.get(&palette::HIT_QUERY) {
                let mut node = Node::new(
                    palette::HIT_QUERY,
                    if state.remote_input_mode {
                        "SSH host"
                    } else {
                        "Filter commands and sessions"
                    },
                    *rect,
                    Role::TextField,
                );
                node.value = Value::Text(state.query.clone());
                node.edit = Some(Edit::Palette);
                node.default_focus = true;
                node.help = "Type to filter. Use arrow keys to choose a result and Return to run it. Escape closes the palette.".into();
                model.nodes.push(node);
            }
            for (position, &index) in state.filtered.iter().enumerate() {
                let id = palette::HIT_ENTRY_BASE + index as u64;
                if let Some(rect) = layout.get(&id) {
                    let entry = &state.entries[index];
                    let mut node = Node::new(id, &entry.label, *rect, Role::Button);
                    node.identity = format!("{:?}", entry.kind);
                    node.selected = position == state.selected_idx;
                    node.press = Some(UiAction::ExecutePaletteEntry(index));
                    node.enabled = entry.kind.is_selectable();
                    model.nodes.push(node);
                }
            }
            if let Some(rect) = layout.get(&palette::HIT_SCROLL) {
                model.nodes.push(scrollbar(
                    palette::HIT_SCROLL,
                    *rect,
                    "Palette result position",
                    state.selected_idx,
                    state.filtered.len().saturating_sub(1),
                    Scroll::Palette,
                ));
            }
            if let Some(message) = state
                .remote_loading
                .as_ref()
                .map(|host| format!("Loading sessions from {host}"))
                .or_else(|| {
                    state
                        .remote_error
                        .as_ref()
                        .map(|(host, error)| format!("{host}: {error}"))
                })
                .or_else(|| {
                    (state.filtered.is_empty()
                        && !state.query.is_empty()
                        && !state.remote_input_mode)
                        .then(|| "No matches".into())
                })
                && let Some(rect) = layout.get(&palette::HIT_PANEL)
            {
                model
                    .nodes
                    .push(Node::new(20, message, *rect, Role::StaticText));
            }
            return Some(model);
        }
        if let Some(component) = settings_panel::SettingsPanelComponent::capture(app, &cx) {
            let layout = component.accessibility_layout(&cx);
            let mut model = Self {
                label: "Settings".into(),
                identity: digest("settings"),
                nodes: vec![],
                dismiss: Some(UiAction::CloseSettings),
            };
            let mut seen = std::collections::HashSet::new();
            for element in layout.nodes() {
                let Some(id) = element.hit_id.filter(|id| seen.insert(*id)) else {
                    continue;
                };
                let Some(hit) = settings_panel::hit::decode(id) else {
                    continue;
                };
                let mut node = match hit {
                    SettingsHit::Chrome(ChromeOp::Dialog) => continue,
                    SettingsHit::Chrome(ChromeOp::Scroll) => {
                        model.nodes.push(scrollbar(
                            id,
                            element.bounds,
                            "Settings scroll position",
                            app.core.settings_scroll_offset,
                            component.max_scroll_offset(),
                            Scroll::Settings,
                        ));
                        continue;
                    }
                    SettingsHit::Chrome(ChromeOp::Close) => {
                        Node::new(id, "Close settings", element.bounds, Role::Button)
                    }
                    SettingsHit::Chrome(ChromeOp::OpenToml) => {
                        Node::new(id, "Open settings.toml", element.bounds, Role::Button)
                    }
                    SettingsHit::Sidebar(category) => {
                        let mut n = Node::new(id, category.label(), element.bounds, Role::Button);
                        n.selected = category == app.core.settings_category;
                        n
                    }
                    SettingsHit::Field { field, role } => {
                        let meta = schema::meta(field);
                        let role_type = match role {
                            ButtonRole::SwitchToggle => Role::Checkbox,
                            ButtonRole::DropdownOpen => Role::Popup,
                            _ => Role::Button,
                        };
                        let label = match role {
                            ButtonRole::StepperDec => format!("Decrease {}", meta.label),
                            ButtonRole::StepperInc => format!("Increase {}", meta.label),
                            _ => meta.label.into(),
                        };
                        let mut n = Node::new(id, label, element.bounds, role_type);
                        n.help = meta.description.into();
                        n.value = if matches!(meta.kind, FieldKind::Bool) {
                            Value::Bool(schema::read_bool(field, &app.core.config))
                        } else {
                            Value::Text(schema::display_value(field, &app.core.config))
                        };
                        n.enabled = match (role, &meta.kind) {
                            (ButtonRole::StepperDec, FieldKind::Int { min, .. }) => {
                                schema::read_int(field, &app.core.config) > *min
                            }
                            (ButtonRole::StepperInc, FieldKind::Int { max, .. }) => {
                                schema::read_int(field, &app.core.config) < *max
                            }
                            (ButtonRole::StepperDec, FieldKind::Float { min, .. }) => {
                                schema::read_float(field, &app.core.config) > *min
                            }
                            (ButtonRole::StepperInc, FieldKind::Float { max, .. }) => {
                                schema::read_float(field, &app.core.config) < *max
                            }
                            _ => true,
                        };
                        n
                    }
                };
                node.press = Some(settings_panel::action_for(Some(hit)));
                model.nodes.push(node);
            }
            return Some(model);
        }
        if let Some(search) = &app.core.search_state {
            let tiles: Vec<_> = app
                .core
                .workspaces
                .visible_tiles_2d(
                    app.core.anim_mgr.view_offset_x.value() as f32,
                    app.core.anim_mgr.view_offset_y.value() as f32,
                )
                .into_iter()
                .map(|(pane, mut rect, active)| {
                    rect.x += app.content_origin_x();
                    rect.y += app.content_origin_y();
                    (pane, rect, active)
                })
                .collect();
            let component = app.search_bar_component(&tiles)?;
            let mut node = Node::new(
                1,
                "Search terminal output",
                component.bounds(),
                Role::TextField,
            );
            node.value = Value::Text(search.query.clone());
            node.edit = Some(Edit::Search);
            node.default_focus = true;
            node.help = format!(
                "{} matches. Return moves to the next match; Shift Return moves to the previous match. Escape closes search.",
                search.matches.len()
            );
            return Some(Self {
                label: "Find in terminal".into(),
                identity: digest(&format!("search:{}", search.pane_id)),
                nodes: vec![node],
                dismiss: Some(UiAction::CloseSearch),
            });
        }
        if app.core.overview.active {
            let zoom = app.core.anim_mgr.overview_zoom.value() as f32;
            let vox = app.core.anim_mgr.view_offset_x.value() as f32;
            let voy = app.core.anim_mgr.view_offset_y.value() as f32;
            let mut nodes = Vec::new();
            for (pane, mut rect, active) in app.overview_visible_tiles(zoom, vox, voy) {
                let Some(workspace) = app
                    .core
                    .workspaces
                    .workspaces
                    .iter()
                    .position(|ws| ws.all_pane_ids().contains(&pane))
                else {
                    continue;
                };
                rect.x += app.content_origin_x();
                rect.y += app.content_origin_y();
                let rect = app.transformed_tile_rect(rect, zoom, cx.viewport_w, cx.viewport_h);
                let title = app
                    .core
                    .pane_grids
                    .get(&pane)
                    .map(|grid| {
                        if grid.password_input {
                            "Password input"
                        } else {
                            grid.title.as_str()
                        }
                    })
                    .unwrap_or("");
                let mut node = Node::new(
                    pane,
                    format!("Workspace {}, terminal {pane}, {title}", workspace + 1),
                    [rect.x, rect.y, rect.w, rect.h],
                    Role::Button,
                );
                node.press = Some(UiAction::FocusOverviewPane(workspace, pane));
                node.selected = active;
                node.enabled = app.core.connected && app.core.pending_session_name.is_none();
                nodes.push(node);
            }
            return Some(Self {
                label: "Terminal overview".into(),
                identity: digest("overview"),
                nodes,
                dismiss: Some(UiAction::ToggleOverview),
            });
        }
        let layout = app.top_bar_layout(
            cx.viewport_w,
            cx.viewport_h,
            cx.cell_w,
            cx.cell_h,
            cx.ui_shaper,
        );
        let rects = super::frame::chrome_rects(
            app,
            cx.viewport_w,
            cx.viewport_h,
            layout.bar_height,
            app.hints_bar_height(),
        );
        let top = super::top_bar::TopBarComponent::capture(app, layout, &cx);
        let mut nodes = top.accessibility(&cx);
        if let Some(rect) = rects.side_tab_bar {
            nodes.extend(
                super::tab_bar::TabBarComponent::capture(app, &cx, rect).accessibility(&cx),
            );
        }
        if let Some(status) = super::connection_status::ConnectionStatusComponent::capture(app, &cx)
        {
            nodes.push(status.accessibility(app));
        }
        for node in &mut nodes {
            if matches!(node.press, Some(UiAction::FocusPaneTab(_))) {
                node.enabled = app.core.connected && app.core.pending_session_name.is_none();
            }
        }
        Some(Self {
            label: "Terminal panes and controls".into(),
            identity: digest("chrome"),
            nodes,
            dismiss: None,
        })
    }
    pub fn apply(
        app: &mut App,
        model_identity: u64,
        expected: &Node,
        operation: Operation,
    ) -> bool {
        let Some(fresh) = Self::capture(app).filter(|m| m.identity == model_identity) else {
            return false;
        };
        let Some(node) = fresh
            .nodes
            .iter()
            .find(|n| n.same_target(expected) && n.enabled)
        else {
            return false;
        };
        if matches!(operation, Operation::Text(_)) && node.value != expected.value {
            return false;
        }
        match operation {
            Operation::Press => {
                let Some(action) = &node.press else {
                    return false;
                };
                if matches!(action, UiAction::OpenSettingsDropdown(_)) {
                    app.last_mouse_pos = Some((
                        node.bounds[0] + node.bounds[2] / 2.0,
                        node.bounds[1] + node.bounds[3] / 2.0,
                    ));
                }
                app.apply_ui_action(action.clone());
            }
            Operation::Text(text) => {
                if text.len() > 16 * 1024 {
                    return false;
                }
                match node.edit {
                    Some(Edit::Palette) => {
                        if let Some(palette) = &mut app.core.command_palette {
                            palette.query = App::sanitize_overlay_input(&text).unwrap_or_default();
                            app.filter_palette();
                        }
                    }
                    Some(Edit::Search) => {
                        if let Some(search) = &mut app.core.search_state {
                            search.query = App::sanitize_overlay_input(&text).unwrap_or_default();
                            app.update_search_results();
                        }
                    }
                    None => return false,
                }
            }
            Operation::Step(step) => {
                let Value::Number { value, max } = node.value else {
                    return false;
                };
                let target = value.saturating_add_signed(step as isize).min(max);
                Self::scroll(app, node.scroll, target);
            }
            Operation::Number(value) => {
                let Value::Number { max, .. } = node.value else {
                    return false;
                };
                Self::scroll(app, node.scroll, value.min(max));
            }
            Operation::Focus => {
                if node.edit.is_none() {
                    return false;
                }
            }
            Operation::Cancel => {
                let Some(action) = fresh.dismiss else {
                    return false;
                };
                app.apply_ui_action(action);
            }
        }
        app.schedule_redraw();
        true
    }
    fn scroll(app: &mut App, scroll: Option<Scroll>, value: usize) {
        match scroll {
            Some(Scroll::Settings) => app.core.settings_scroll_offset = value,
            Some(Scroll::ContextMenu) => {
                app.core.context_menu_scroll_offset = value;
            }
            Some(Scroll::Palette) => {
                if let Some(palette) = &mut app.core.command_palette {
                    let position = (value..palette.filtered.len())
                        .chain((0..value).rev())
                        .find(|&p| palette.entries[palette.filtered[p]].kind.is_selectable());
                    if let Some(position) = position {
                        palette.selected_idx = position;
                    }
                }
            }
            None => {}
        }
    }
}

pub(super) fn help_model(sections: Vec<(String, [f32; 4])>) -> Model {
    let mut nodes = Vec::new();
    for (index, (section, rect)) in sections.into_iter().enumerate() {
        let mut node = Node::new(index as u64 + 1, "Key bindings", rect, Role::StaticText);
        node.value = Value::Text(section);
        nodes.push(node);
    }
    Model {
        label: "Keyboard shortcuts".into(),
        identity: digest("help"),
        nodes,
        dismiss: Some(UiAction::CloseHelp),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{PendingPaste, PendingPasteTarget};
    use loom_config::LoomConfig;
    fn app() -> App {
        App::new(LoomConfig::default(), "ax-ui-test")
    }
    fn paste(app: &mut App) {
        let text = "echo example\n".repeat(20);
        app.core.pending_paste = Some(PendingPaste {
            info: crate::app::paste_guard::check_paste_size(&text, 1).unwrap(),
            preview: "echo example...".into(),
            target: PendingPasteTarget::Terminal,
        });
    }
    #[test]
    fn paste_controls_share_pointer_bounds_and_cancel_preserves_underlying_palette() {
        let mut app = app();
        app.core.open_command_palette();
        let query = Model::capture(&app).unwrap();
        paste(&mut app);
        let model = Model::capture(&app).unwrap();
        assert_eq!(model.label, "Confirm paste");
        assert!(!model.nodes.iter().any(|n| n.edit.is_some()));
        assert!(!Model::apply(
            &mut app,
            query.identity,
            &query.nodes[0],
            Operation::Text("wrong".into())
        ));
        let cancel = model
            .nodes
            .iter()
            .find(|n| n.press == Some(UiAction::CancelPaste))
            .unwrap();
        let cx = app.ui_context();
        let dialog = paste_dialog::PasteDialogComponent::capture(&app, &cx).unwrap();
        assert_eq!(
            dialog.click(
                cancel.bounds[0] + cancel.bounds[2] / 2.0,
                cancel.bounds[1] + cancel.bounds[3] / 2.0,
                &cx
            ),
            Some(UiAction::CancelPaste)
        );
        assert!(Model::apply(
            &mut app,
            model.identity,
            cancel,
            Operation::Press
        ));
        assert!(app.core.pending_paste.is_none());
        assert!(app.core.command_palette.is_some());
    }
    #[test]
    fn stale_paste_confirmation_cannot_confirm_a_replacement_payload() {
        let mut app = app();
        paste(&mut app);
        let model = Model::capture(&app).unwrap();
        let paste = model
            .nodes
            .iter()
            .find(|n| n.press == Some(UiAction::ConfirmPaste))
            .unwrap();
        app.core.pending_paste.as_mut().unwrap().info.text = "different\n".into();
        assert!(!Model::apply(
            &mut app,
            model.identity,
            paste,
            Operation::Press
        ));
        assert!(app.core.pending_paste.is_some());
    }
    #[test]
    fn settings_accessibility_matches_visible_fields_and_scrolls_to_last_row() {
        let mut app = app();
        app.core.settings_panel_visible = true;
        app.core.settings_category = loom_app::app::SettingsCategory::Font;
        let model = Model::capture(&app).unwrap();
        let cx = app.ui_context();
        let component = settings_panel::SettingsPanelComponent::capture(&app, &cx).unwrap();
        for node in &model.nodes {
            if let Some(action) = &node.press {
                assert_eq!(
                    component.click(
                        node.bounds[0] + node.bounds[2] / 2.0,
                        node.bounds[1] + node.bounds[3] / 2.0,
                        &cx
                    ),
                    Some(action.clone()),
                    "{}",
                    node.label
                );
            }
        }
        let scroll = model
            .nodes
            .iter()
            .find(|n| n.scroll == Some(Scroll::Settings))
            .unwrap();
        assert!(Model::apply(
            &mut app,
            model.identity,
            scroll,
            Operation::Number(usize::MAX)
        ));
        let after = Model::capture(&app).unwrap();
        assert!(
            after
                .nodes
                .iter()
                .any(|n| n.label.contains("Strikethrough"))
        );
        assert!(
            after
                .nodes
                .iter()
                .all(|n| n.bounds.iter().all(|n| n.is_finite()))
        );
    }
    #[test]
    fn palette_value_edits_filter_without_injecting_control_characters() {
        let mut app = app();
        app.core.open_command_palette();
        let model = Model::capture(&app).unwrap();
        let query = model
            .nodes
            .iter()
            .find(|n| n.edit == Some(Edit::Palette))
            .unwrap();
        assert!(Model::apply(
            &mut app,
            model.identity,
            query,
            Operation::Text("hello\n\x1bworld".into())
        ));
        assert_eq!(
            app.core.command_palette.as_ref().unwrap().query,
            "hello world"
        );
        // An old value must not overwrite text typed after the AX callback.
        assert!(!Model::apply(
            &mut app,
            model.identity,
            query,
            Operation::Text("stale".into())
        ));
    }
    #[test]
    fn context_menu_replacement_invalidates_old_item_even_with_same_label() {
        let mut app = app();
        app.core.context_menu.visible = true;
        app.core.context_menu.items = vec![crate::app::ContextMenuItem {
            label: "Open".into(),
            action: crate::app::ContextMenuAction::OpenLink("https://example.com/one".into()),
            enabled: true,
        }];
        let model = Model::capture(&app).unwrap();
        let item = model.nodes[0].clone();
        app.core.context_menu.items[0].action =
            crate::app::ContextMenuAction::OpenLink("https://example.com/two".into());
        assert!(!Model::apply(
            &mut app,
            model.identity,
            &item,
            Operation::Press
        ));
        assert!(app.core.context_menu.visible);
    }
    fn pane(app: &mut App, pane: u64) {
        app.core
            .workspaces
            .active_mut()
            .add_column_right(pane, loom_layout::column::ColumnWidth::Proportion(1.0));
        let mut grid = crate::grid::ClientPaneGrid::new(8, 2, 0);
        grid.title = format!("Pane {pane}");
        for (cell, ch) in grid.viewport.iter_mut().zip("find me ".chars()) {
            cell.set_ch(ch);
        }
        app.core.pane_grids.insert(pane, grid);
        app.core.connected = true;
    }
    #[test]
    fn search_edits_use_live_matches_and_cache_tracks_query_and_pane() {
        let mut app = app();
        pane(&mut app, 42);
        app.core.search_state = Some(loom_app::app::SearchState {
            pane_id: 42,
            query: String::new(),
            matches: vec![],
            current_match_idx: 0,
            original_scroll_offset: 0,
        });
        let key = Model::cache_key(&app);
        let model = Model::capture(&app).unwrap();
        assert_eq!(model.nodes[0].edit, Some(Edit::Search));
        assert!(Model::apply(
            &mut app,
            model.identity,
            &model.nodes[0],
            Operation::Text("find".into())
        ));
        assert_eq!(app.core.search_state.as_ref().unwrap().matches.len(), 1);
        assert_ne!(key, Model::cache_key(&app));
        let fresh = Model::capture(&app).unwrap();
        assert!(Model::apply(
            &mut app,
            fresh.identity,
            &fresh.nodes[0],
            Operation::Cancel
        ));
        assert!(app.core.search_state.is_none());
    }
    #[test]
    fn chrome_controls_match_mouse_targets_for_all_tab_bar_positions() {
        use loom_config::config::{StatusBarPosition, TabBarPosition};
        let mut app = app();
        pane(&mut app, 42);
        pane(&mut app, 43);
        for position in [
            TabBarPosition::Integrated,
            TabBarPosition::Left,
            TabBarPosition::Right,
        ] {
            app.core.config.tabbar.position = position;
            for status in [StatusBarPosition::Top, StatusBarPosition::Bottom] {
                app.core.config.statusbar.position = status;
                let model = Model::capture(&app).unwrap();
                assert!(model.dismiss.is_none());
                assert!(
                    model
                        .nodes
                        .iter()
                        .any(|n| matches!(n.press, Some(UiAction::FocusPaneTab(_))))
                );
                let cx = app.ui_context();
                let frame = super::super::frame::UiFrame::capture_current(&app, &cx);
                for node in &model.nodes {
                    assert_eq!(
                        frame
                            .click(
                                &app,
                                node.bounds[0] + node.bounds[2] / 2.0,
                                node.bounds[1] + node.bounds[3] / 2.0,
                                &cx
                            )
                            .0,
                        node.press,
                        "{}",
                        node.label
                    );
                }
            }
        }
    }
    #[test]
    fn overview_uses_the_same_zoom_and_content_origin_as_pointer_selection() {
        let mut app = app();
        pane(&mut app, 42);
        app.core.overview.active = true;
        let model = Model::capture(&app).unwrap();
        assert_eq!(model.label, "Terminal overview");
        for node in &model.nodes {
            let target = app.hit_test_overview(
                node.bounds[0] + node.bounds[2] / 2.0,
                node.bounds[1] + node.bounds[3] / 2.0,
            );
            let Some(UiAction::FocusOverviewPane(workspace, pane)) = node.press else {
                panic!("overview action");
            };
            assert_eq!(target, Some((workspace, pane)));
        }
    }
    #[test]
    fn command_identity_and_hidden_paste_payload_invalidate_semantic_cache() {
        let mut app = app();
        app.core.context_menu.visible = true;
        app.core.context_menu.items = vec![crate::app::ContextMenuItem {
            label: "Open".into(),
            action: crate::app::ContextMenuAction::OpenLink("one".into()),
            enabled: true,
        }];
        let key = Model::cache_key(&app);
        app.core.context_menu.items[0].action =
            crate::app::ContextMenuAction::OpenLink("two".into());
        assert_ne!(key, Model::cache_key(&app));
        app.core.context_menu.visible = false;
        paste(&mut app);
        let key = Model::cache_key(&app);
        app.core
            .pending_paste
            .as_mut()
            .unwrap()
            .info
            .text
            .replace_range(..4, "none");
        assert_ne!(key, Model::cache_key(&app));
    }
    #[test]
    fn help_has_separate_section_bounds_and_buttons_do_not_claim_keyboard_focus() {
        let mut app = app();
        app.core.help_visible = true;
        let model = Model::capture(&app).unwrap();
        assert!(model.nodes.len() > 1);
        assert_ne!(model.nodes[0].bounds, model.nodes[1].bounds);
        assert!(Model::apply(
            &mut app,
            model.identity,
            &model.nodes[0],
            Operation::Cancel
        ));
        assert!(!app.core.help_visible);
        let chrome = Model::capture(&app).unwrap();
        assert!(!Model::apply(
            &mut app,
            chrome.identity,
            &chrome.nodes[0],
            Operation::Focus
        ));
    }
    #[test]
    fn disconnected_status_is_readable_without_animating_dots() {
        let mut app = app();
        app.core.server_tx = Some(crossbeam_channel::unbounded().0);
        let model = Model::capture(&app).unwrap();
        let node = model
            .nodes
            .iter()
            .find(|node| node.label == "Connection status")
            .unwrap();
        let Value::Text(value) = &node.value else {
            panic!("status text");
        };
        assert!(value.contains("ax-ui-test"));
        assert!(!value.contains("..."));
        assert!(node.press.is_none());
    }
}
