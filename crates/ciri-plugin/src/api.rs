//! `ciri.*` Lua API bindings.

use mlua::Lua;

use crate::events::EventRegistry;

/// Register the `ciri` global table with core API functions.
pub(crate) fn register_api(lua: &Lua, registry: &EventRegistry) -> mlua::Result<()> {
    let ciri = lua.create_table()?;

    // ciri.on(event_name, callback)
    let reg = registry.clone();
    ciri.set(
        "on",
        lua.create_function(move |lua, (event, func): (String, mlua::Function)| {
            let key = lua.create_registry_value(func)?;
            reg.register(event, key);
            Ok(())
        })?,
    )?;

    // ciri.log(msg)
    ciri.set(
        "log",
        lua.create_function(|_, msg: String| {
            log::info!("[plugin] {msg}");
            Ok(())
        })?,
    )?;

    // ciri.warn(msg)
    ciri.set(
        "warn",
        lua.create_function(|_, msg: String| {
            log::warn!("[plugin] {msg}");
            Ok(())
        })?,
    )?;

    lua.globals().set("ciri", ciri)?;
    Ok(())
}
