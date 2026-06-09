//! `loom.*` Lua API bindings.

use mlua::{Lua, LuaSerdeExt, Value};

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

    // loom.json — a sub-table of encode / decode helpers, mirroring
    // Neovim's `vim.json`. Built on serde_json + mlua's `LuaSerdeExt`
    // so the values round-trip through the same data model the rest
    // of the plugin API uses.
    let json = lua.create_table()?;
    json.set(
        "decode",
        lua.create_function(|lua, s: mlua::String| {
            let bytes = s.as_bytes();
            let parsed: serde_json::Value = serde_json::from_slice(&bytes).map_err(|e| {
                mlua::Error::external(format!("loom.json.decode: {e}"))
            })?;
            lua.to_value(&parsed)
        })?,
    )?;
    json.set(
        "encode",
        lua.create_function(|lua, v: Value| {
            let v: serde_json::Value = lua.from_value(v).map_err(|e| {
                mlua::Error::external(format!("loom.json.encode (table → json): {e}"))
            })?;
            serde_json::to_string(&v).map_err(|e| {
                mlua::Error::external(format!("loom.json.encode (serialize): {e}"))
            })
        })?,
    )?;
    loom.set("json", json)?;

    lua.globals().set("loom", loom)?;
    Ok(())
}
