//! Register replacements before releasing the working shortcut. A typo or an
//! OS-level conflict during hot reload must not strand a hidden terminal.
use global_hotkey::{GlobalHotKeyEvent, GlobalHotKeyManager, HotKeyState, hotkey::HotKey};

pub trait Registrar {
    fn register(&self, hotkey: HotKey) -> Result<(), String>;
    fn unregister(&self, hotkey: HotKey) -> Result<(), String>;
}

impl Registrar for GlobalHotKeyManager {
    fn register(&self, hotkey: HotKey) -> Result<(), String> {
        self.register(hotkey).map_err(|error| error.to_string())
    }
    fn unregister(&self, hotkey: HotKey) -> Result<(), String> {
        self.unregister(hotkey).map_err(|error| error.to_string())
    }
}

#[derive(Default)]
pub struct Registration {
    attempted: Option<String>,
    registered: Option<HotKey>,
    down: bool,
    pub error: Option<String>,
}

impl Registration {
    pub fn update(&mut self, manager: &impl Registrar, shortcut: &str) {
        if self.attempted.as_deref() == Some(shortcut) {
            return;
        }
        self.attempted = Some(shortcut.to_owned());
        self.error = self.replace(manager, shortcut).err();
        if let Some(error) = &self.error {
            log::warn!("Quick Terminal shortcut {shortcut:?}: {error}");
        }
    }

    fn replace(&mut self, manager: &impl Registrar, shortcut: &str) -> Result<(), String> {
        let next = if shortcut.trim().is_empty() {
            None
        } else {
            Some(
                shortcut
                    .parse::<HotKey>()
                    .map_err(|error| error.to_string())?,
            )
        };
        if next == self.registered {
            return Ok(());
        }
        if let Some(next) = next {
            manager.register(next)?;
        }
        if let Some(old) = self.registered
            && let Err(error) = manager.unregister(old)
        {
            if let Some(next) = next {
                let _ = manager.unregister(next);
            }
            return Err(error);
        }
        self.registered = next;
        self.down = false;
        Ok(())
    }

    /// Toggle once per press, never on key repeat or a stale registration.
    pub fn pressed(&mut self, event: GlobalHotKeyEvent) -> bool {
        if self.registered.is_none_or(|hotkey| hotkey.id() != event.id) {
            return false;
        }
        match event.state {
            HotKeyState::Pressed => {
                let first = !self.down;
                self.down = true;
                first
            }
            HotKeyState::Released => {
                self.down = false;
                false
            }
        }
    }

    pub fn label(&self) -> Option<String> {
        self.registered.map(HotKey::into_string)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    struct FakeRegistrar {
        registered: RefCell<Vec<HotKey>>,
        conflict: Cell<Option<u32>>,
    }
    impl Registrar for FakeRegistrar {
        fn register(&self, key: HotKey) -> Result<(), String> {
            if self.conflict.get() == Some(key.id()) {
                return Err("already registered by another app".into());
            }
            self.registered.borrow_mut().push(key);
            Ok(())
        }
        fn unregister(&self, key: HotKey) -> Result<(), String> {
            self.registered
                .borrow_mut()
                .retain(|registered| *registered != key);
            Ok(())
        }
    }

    #[test]
    fn conflicting_or_invalid_reload_preserves_the_live_shortcut() {
        let manager = FakeRegistrar::default();
        let mut registration = Registration::default();
        registration.update(&manager, "Super+Backquote");
        let original = registration.registered.unwrap();
        let replacement: HotKey = "Super+Space".parse().unwrap();
        manager.conflict.set(Some(replacement.id()));
        registration.update(&manager, "Super+Space");
        assert!(registration.error.is_some());
        assert_eq!(*manager.registered.borrow(), [original]);
        registration.update(&manager, "not a real key");
        assert!(registration.error.is_some());
        assert_eq!(registration.registered, Some(original));
        registration.update(&manager, "");
        assert!(manager.registered.borrow().is_empty());
        assert!(registration.error.is_none());
    }

    #[test]
    fn old_hotkey_events_and_repeats_do_not_toggle_a_terminal() {
        let manager = FakeRegistrar::default();
        let mut registration = Registration::default();
        registration.update(&manager, "Super+Backquote");
        let old = registration.registered.unwrap().id();
        registration.update(&manager, "Control+Super+Space");
        let new = registration.registered.unwrap().id();
        let event = |id, state| GlobalHotKeyEvent { id, state };
        assert!(!registration.pressed(event(old, HotKeyState::Pressed)));
        assert!(registration.pressed(event(new, HotKeyState::Pressed)));
        assert!(!registration.pressed(event(new, HotKeyState::Pressed)));
        assert!(!registration.pressed(event(new, HotKeyState::Released)));
        assert!(registration.pressed(event(new, HotKeyState::Pressed)));
    }
}
