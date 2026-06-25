//! Event registry and dispatch logic.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use mlua::{FromLuaMulti, Function, IntoLuaMulti, Lua, RegistryKey, Value};

/// Maximum consecutive failures before a handler is disabled.
const MAX_FAILURES: u32 = 3;

/// A registered event handler with consecutive-failure tracking.
struct Handler {
    key: RegistryKey,
    failures: u32,
}

impl Handler {
    /// A handler is disabled once it has failed `MAX_FAILURES` times in a row.
    fn is_disabled(&self) -> bool {
        self.failures >= MAX_FAILURES
    }
}

/// Thread-local event handler registry.
///
/// Uses `Rc<RefCell<>>` because the Lua VM is single-threaded.
#[derive(Clone, Default)]
pub(crate) struct EventRegistry {
    inner: Rc<RefCell<HashMap<String, Vec<Handler>>>>,
}

impl EventRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a handler for an event.
    pub fn register(&self, event: String, key: RegistryKey) {
        self.inner
            .borrow_mut()
            .entry(event)
            .or_default()
            .push(Handler { key, failures: 0 });
    }

    /// Clear all handlers (used during reload).
    pub fn clear(&self) {
        self.inner.borrow_mut().clear();
    }

    /// Number of handlers currently registered for `event`.
    fn handler_count(&self, event: &str) -> usize {
        self.inner.borrow().get(event).map_or(0, Vec::len)
    }

    /// Fetch the `idx`th handler's Lua function, or `None` if it is missing,
    /// disabled, or its registry value cannot be retrieved.
    ///
    /// The registry borrow is released before the returned function is called,
    /// so a handler that re-enters the registry (e.g. by calling `loom.on`)
    /// cannot trigger a `RefCell` double borrow.
    fn handler_fn(&self, lua: &Lua, event: &str, idx: usize) -> Option<Function> {
        let map = self.inner.borrow();
        let handler = map.get(event)?.get(idx)?;
        if handler.is_disabled() {
            return None;
        }
        match lua.registry_value(&handler.key) {
            Ok(func) => Some(func),
            Err(e) => {
                log::warn!("[plugin] failed to retrieve handler for '{event}': {e}");
                None
            }
        }
    }

    /// Reset a handler's consecutive-failure count after a successful call.
    fn reset_failures(&self, event: &str, idx: usize) {
        if let Some(h) = self
            .inner
            .borrow_mut()
            .get_mut(event)
            .and_then(|hs| hs.get_mut(idx))
        {
            h.failures = 0;
        }
    }

    /// Record a handler failure, disabling it once it reaches `MAX_FAILURES`
    /// consecutive failures (logging the transition exactly once).
    fn record_failure(&self, event: &str, idx: usize) {
        let mut map = self.inner.borrow_mut();
        let Some(h) = map.get_mut(event).and_then(|hs| hs.get_mut(idx)) else {
            return;
        };
        h.failures += 1;
        if h.failures == MAX_FAILURES {
            log::warn!(
                "[plugin] disabled '{event}' handler after {MAX_FAILURES} consecutive failures"
            );
        }
    }

    /// Dispatch a void event: every enabled handler runs in last-registered-first
    /// order and its return value is ignored.
    pub fn dispatch_void<A: IntoLuaMulti + Clone>(&self, lua: &Lua, event: &str, args: A) {
        // Iterate over a snapshot of the handler count; a handler that registers
        // another mid-dispatch only appends and is not invoked in this pass.
        for idx in (0..self.handler_count(event)).rev() {
            let Some(func) = self.handler_fn(lua, event, idx) else {
                continue;
            };
            match func.call::<()>(args.clone()) {
                // A successful call clears the handler's failure streak so that
                // only *consecutive* failures count toward disabling it.
                Ok(()) => self.reset_failures(event, idx),
                Err(e) => {
                    log::warn!("[plugin] error in '{event}' handler: {e}");
                    self.record_failure(event, idx);
                }
            }
        }
    }

    /// Dispatch an event expecting a return value. The last-registered handler
    /// that returns a non-nil value wins; returns `None` if all return nil.
    pub fn dispatch_first<A, R>(&self, lua: &Lua, event: &str, args: A) -> Option<R>
    where
        A: IntoLuaMulti + Clone,
        R: FromLuaMulti,
    {
        for idx in (0..self.handler_count(event)).rev() {
            let Some(func) = self.handler_fn(lua, event, idx) else {
                continue;
            };
            match func.call::<Value>(args.clone()) {
                Ok(Value::Nil) => continue,
                Ok(val) => {
                    // A non-nil return clears the handler's failure streak.
                    self.reset_failures(event, idx);
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
                    self.record_failure(event, idx);
                }
            }
        }
        None
    }
}
