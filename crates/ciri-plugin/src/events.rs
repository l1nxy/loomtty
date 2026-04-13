//! Event registry and dispatch logic.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use mlua::{FromLuaMulti, IntoLuaMulti, Lua, RegistryKey, Value};

/// Maximum consecutive failures before a handler is disabled.
const MAX_FAILURES: u32 = 3;

/// A registered event handler with failure tracking.
struct Handler {
    key: RegistryKey,
    failures: u32,
    disabled: bool,
}

/// Thread-local event handler registry.
///
/// Uses `Rc<RefCell<>>` because the Lua VM is single-threaded.
#[derive(Clone)]
pub(crate) struct EventRegistry {
    inner: Rc<RefCell<HashMap<String, Vec<Handler>>>>,
}

impl EventRegistry {
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(HashMap::new())),
        }
    }

    /// Register a handler for an event. Returns the registration index.
    pub fn register(&self, event: String, key: RegistryKey) {
        self.inner
            .borrow_mut()
            .entry(event)
            .or_default()
            .push(Handler {
                key,
                failures: 0,
                disabled: false,
            });
    }

    /// Clear all handlers (used during reload).
    pub fn clear(&self) {
        self.inner.borrow_mut().clear();
    }

    /// Dispatch a void event (handlers return nothing meaningful).
    pub fn dispatch_void<A: IntoLuaMulti + Clone>(&self, lua: &Lua, event: &str, args: A) {
        let handlers_exist = {
            let map = self.inner.borrow();
            map.contains_key(event)
        };
        if !handlers_exist {
            return;
        }

        // Collect keys to call — we must not hold the borrow during Lua calls.
        let handler_count = {
            let map = self.inner.borrow();
            map.get(event).map(|h| h.len()).unwrap_or(0)
        };

        // Iterate in reverse (last-registered-first) for override semantics.
        for idx in (0..handler_count).rev() {
            let (disabled, key_ref) = {
                let map = self.inner.borrow();
                let handlers = match map.get(event) {
                    Some(h) => h,
                    None => return,
                };
                let h = match handlers.get(idx) {
                    Some(h) => h,
                    None => continue,
                };
                if h.disabled {
                    continue;
                }
                // We need the registry key to call the function.
                // RegistryKey is not Clone, so we get a reference via the Lua VM.
                (h.disabled, idx)
            };
            if disabled {
                continue;
            }

            let result = {
                let map = self.inner.borrow();
                let handlers = map.get(event).unwrap();
                let func: mlua::Function = match lua.registry_value(&handlers[key_ref].key) {
                    Ok(f) => f,
                    Err(e) => {
                        log::warn!("[plugin] failed to retrieve handler for '{event}': {e}");
                        continue;
                    }
                };
                func.call::<()>(args.clone())
            };

            if let Err(e) = result {
                log::warn!("[plugin] error in '{event}' handler: {e}");
                let mut map = self.inner.borrow_mut();
                if let Some(handlers) = map.get_mut(event) {
                    if let Some(h) = handlers.get_mut(key_ref) {
                        h.failures += 1;
                        if h.failures >= MAX_FAILURES {
                            h.disabled = true;
                            log::warn!(
                                "[plugin] disabled '{event}' handler after {MAX_FAILURES} consecutive failures"
                            );
                        }
                    }
                }
            }
        }
    }

    /// Dispatch an event expecting a return value. Last-registered handler that
    /// returns a non-nil value wins. Returns `None` if all handlers return nil.
    pub fn dispatch_first<A, R>(&self, lua: &Lua, event: &str, args: A) -> Option<R>
    where
        A: IntoLuaMulti + Clone,
        R: FromLuaMulti,
    {
        let handlers_exist = {
            let map = self.inner.borrow();
            map.contains_key(event)
        };
        if !handlers_exist {
            return None;
        }

        let handler_count = {
            let map = self.inner.borrow();
            map.get(event).map(|h| h.len()).unwrap_or(0)
        };

        // Last-registered-first for override semantics.
        for idx in (0..handler_count).rev() {
            let disabled = {
                let map = self.inner.borrow();
                let handlers = match map.get(event) {
                    Some(h) => h,
                    None => return None,
                };
                match handlers.get(idx) {
                    Some(h) => h.disabled,
                    None => continue,
                }
            };
            if disabled {
                continue;
            }

            let result = {
                let map = self.inner.borrow();
                let handlers = map.get(event).unwrap();
                let func: mlua::Function = match lua.registry_value(&handlers[idx].key) {
                    Ok(f) => f,
                    Err(e) => {
                        log::warn!("[plugin] failed to retrieve handler for '{event}': {e}");
                        continue;
                    }
                };
                func.call::<Value>(args.clone())
            };

            match result {
                Ok(Value::Nil) => continue,
                Ok(val) => {
                    // Reset failure count on success.
                    {
                        let mut map = self.inner.borrow_mut();
                        if let Some(handlers) = map.get_mut(event) {
                            if let Some(h) = handlers.get_mut(idx) {
                                h.failures = 0;
                            }
                        }
                    }
                    match R::from_lua_multi(val.into_lua_multi(lua).ok()?, lua) {
                        Ok(r) => return Some(r),
                        Err(e) => {
                            log::warn!(
                                "[plugin] failed to convert return value for '{event}': {e}"
                            );
                            continue;
                        }
                    }
                }
                Err(e) => {
                    log::warn!("[plugin] error in '{event}' handler: {e}");
                    let mut map = self.inner.borrow_mut();
                    if let Some(handlers) = map.get_mut(event) {
                        if let Some(h) = handlers.get_mut(idx) {
                            h.failures += 1;
                            if h.failures >= MAX_FAILURES {
                                h.disabled = true;
                                log::warn!(
                                    "[plugin] disabled '{event}' handler after {MAX_FAILURES} consecutive failures"
                                );
                            }
                        }
                    }
                }
            }
        }

        None
    }
}
