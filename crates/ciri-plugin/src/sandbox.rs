//! Lua VM sandboxing: stdlib whitelist and instruction limits.

use mlua::{Lua, LuaOptions, StdLib};

/// Maximum instructions per callback invocation before aborting.
const INSTRUCTION_LIMIT: u32 = 100_000;

/// Create a sandboxed Lua VM with only safe standard libraries.
pub(crate) fn create_sandboxed_lua() -> mlua::Result<Lua> {
    let libs = StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8;
    let lua = Lua::new_with(libs, LuaOptions::default())?;

    // Kill runaway scripts after INSTRUCTION_LIMIT VM instructions.
    let _ = lua.set_hook(
        mlua::HookTriggers::new().every_nth_instruction(INSTRUCTION_LIMIT),
        |_lua, _debug| {
            Err(mlua::Error::runtime(
                "plugin exceeded instruction limit (possible infinite loop)",
            ))
        },
    );

    Ok(lua)
}
