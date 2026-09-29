//! Value snapshots keep Cocoa scripting getters independent of the event loop.
use std::collections::{HashMap, HashSet};
use winit::window::WindowId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Window,
    Tab,
    Terminal,
}
impl Kind {
    pub fn key(self) -> &'static str {
        match self {
            Self::Window => "loomWindows",
            Self::Tab => "loomTabs",
            Self::Terminal => "loomTerminals",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Object {
    pub id: String,
    pub kind: Kind,
    pub name: String,
    pub tabs: Vec<String>,
    pub terminals: Vec<String>,
    pub selected: Option<String>,
    pub focused: Option<String>,
    pub cwd: String,
    pub session: String,
    pub window_id: WindowId,
    pub pane_id: Option<u64>,
    pub ready: bool,
}

#[derive(Clone, Default)]
pub struct Snapshot {
    pub enabled: bool,
    pub windows: Vec<String>,
    pub tabs: Vec<String>,
    pub terminals: Vec<String>,
    pub front: Option<String>,
    pub objects: HashMap<String, Object>,
}

pub struct Ids {
    run: String,
    next: u64,
    pub tabs: HashMap<WindowId, String>,
    terminals: HashMap<(String, u64), String>,
    groups: Vec<(String, Vec<String>)>,
}
impl Default for Ids {
    fn default() -> Self {
        use objc2_foundation::NSUUID;
        Self {
            // Shortcuts can persist entity IDs. Never let an ID saved during
            // an earlier application run resolve to an unrelated terminal.
            run: NSUUID::new().UUIDString().to_string(),
            next: 0,
            tabs: HashMap::new(),
            terminals: HashMap::new(),
            groups: Vec::new(),
        }
    }
}
impl Ids {
    fn next(&mut self, prefix: &str) -> String {
        self.next += 1;
        format!("{prefix}-{}-{}", self.run, self.next)
    }
    pub fn tab(&mut self, key: WindowId) -> String {
        if let Some(id) = self.tabs.get(&key) {
            return id.clone();
        }
        let id = self.next("tab");
        self.tabs.insert(key, id.clone());
        id
    }
    pub fn terminal(&mut self, tab: &str, pane: u64) -> String {
        let key = (tab.to_owned(), pane);
        if let Some(id) = self.terminals.get(&key) {
            return id.clone();
        }
        let id = self.next("terminal");
        self.terminals.insert(key, id.clone());
        id
    }
    pub fn invalidate_terminals(&mut self, tab: &str) {
        self.terminals.retain(|(parent, _), _| parent != tab);
    }
    pub fn prune(&mut self, snapshot: &Snapshot) {
        self.tabs.retain(|_, id| snapshot.objects.contains_key(id));
        self.terminals
            .retain(|_, id| snapshot.objects.contains_key(id));
    }
    // Keep a group's ID when its first tab closes. A merge keeps one original
    // ID; a split creates one new ID. IDs are never recycled in this process.
    pub fn groups(&mut self, memberships: &[Vec<String>]) -> Vec<String> {
        let mut used = HashSet::new();
        let result: Vec<_> = memberships
            .iter()
            .map(|tabs| {
                let old = self
                    .groups
                    .iter()
                    .filter(|(id, _)| !used.contains(id))
                    .map(|(id, old)| (id, tabs.iter().filter(|id| old.contains(id)).count()))
                    .filter(|(_, overlap)| *overlap > 0)
                    .max_by_key(|(_, overlap)| *overlap)
                    .map(|(id, _)| id.clone());
                let id = old.unwrap_or_else(|| self.next("window"));
                used.insert(id.clone());
                (id, tabs.clone())
            })
            .collect();
        let ids = result.iter().map(|(id, _)| id.clone()).collect();
        self.groups = result;
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn tabs(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }
    #[test]
    fn terminal_ids_do_not_resolve_across_bridge_or_application_lifetimes() {
        let old = Ids::default().terminal("tab", 1);
        let new = Ids::default().terminal("tab", 1);
        assert_ne!(old, new);
    }
    #[test]
    fn group_survives_first_tab_closing_and_ids_are_not_recycled() {
        let mut ids = Ids::default();
        let original = ids.groups(&[tabs(&["a", "b"])])[0].clone();
        assert_eq!(ids.groups(&[tabs(&["b"])])[0], original);
        ids.groups(&[]);
        assert_ne!(ids.groups(&[tabs(&["b"])])[0], original);
    }
    #[test]
    fn split_groups_do_not_share_an_id_and_merge_preserves_one() {
        let mut ids = Ids::default();
        let original = ids.groups(&[tabs(&["a", "b"])])[0].clone();
        let split = ids.groups(&[tabs(&["a"]), tabs(&["b"])]);
        assert_eq!(split[0], original);
        assert_ne!(split[0], split[1]);
        assert!(split.contains(&ids.groups(&[tabs(&["a", "b"])])[0]));
    }
    #[test]
    fn reconnect_invalidates_terminal_references_but_not_other_tabs() {
        let mut ids = Ids::default();
        let a = ids.terminal("a", 1);
        let b = ids.terminal("b", 1);
        ids.invalidate_terminals("a");
        assert_ne!(ids.terminal("a", 1), a);
        assert_eq!(ids.terminal("b", 1), b);
    }
}
