//! `loom.*` Lua API bindings.

use mlua::Lua;

use crate::events::EventRegistry;

/// Register the `loom` global table with core API functions.
pub(crate) fn register_api(lua: &Lua, registry: &EventRegistry) -> mlua::Result<()> {
    let loom = lua.create_table()?;

    // loom.on(event_name, callback)
    let reg = registry.clone();
    loom.set(
        "on",
        lua.create_function(move |lua, (event, func): (String, mlua::Function)| {
            let key = lua.create_registry_value(func)?;
            reg.register(event, key);
            Ok(())
        })?,
    )?;

    // loom.log(msg)
    loom.set(
        "log",
        lua.create_function(|_, msg: String| {
            log::info!("[plugin] {msg}");
            Ok(())
        })?,
    )?;

    // loom.warn(msg)
    loom.set(
        "warn",
        lua.create_function(|_, msg: String| {
            log::warn!("[plugin] {msg}");
            Ok(())
        })?,
    )?;

    lua.globals().set("loom", loom)?;
    Ok(())
}
