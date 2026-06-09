//! Lua VM lifecycle: initialization, plugin loading, and reload.

use std::path::{Path, PathBuf};

use mlua::Lua;

use crate::api;
use crate::builtin;
use crate::events::EventRegistry;
use crate::sandbox;

/// Load all plugins into the Lua VM in the correct order:
/// 1. Built-in plugins (embedded)
/// 2. User plugins from ~/.config/loom/plugins/<name>/init.lua
/// 3. User init.lua from ~/.config/loom/init.lua
pub(crate) fn init_vm(registry: &EventRegistry) -> anyhow::Result<Lua> {
    let lua = sandbox::create_sandboxed_lua().map_err(lua_err)?;
    api::register_api(&lua, registry).map_err(lua_err)?;

    // 1. Built-in plugins
    load_chunk(&lua, builtin::SESSION_RESTORE, "builtin:session_restore")?;
    load_chunk(&lua, builtin::USAGE, "builtin:usage")?;

    // 2. User plugins directory
    let plugins_dir = user_plugins_dir();
    if plugins_dir.is_dir() {
        load_plugins_from_dir(&lua, &plugins_dir)?;
    }

    // 3. User init.lua
    let init_path = user_init_path();
    if init_path.is_file() {
        load_file(&lua, &init_path)?;
    }

    Ok(lua)
}

/// Load a Lua source string with a descriptive chunk name.
fn load_chunk(lua: &Lua, source: &str, name: &str) -> anyhow::Result<()> {
    lua.load(source).set_name(name).exec().map_err(|e| {
        log::error!("[plugin] failed to load '{name}': {e}");
        anyhow::anyhow!("failed to load plugin '{name}': {e}")
    })
}

/// Load a Lua file.
fn load_file(lua: &Lua, path: &Path) -> anyhow::Result<()> {
    let source = std::fs::read_to_string(path)?;
    let name = path.display().to_string();
    load_chunk(lua, &source, &name)
}

/// Discover and load plugins from a directory.
/// Each subdirectory with an `init.lua` is treated as a plugin.
fn load_plugins_from_dir(lua: &Lua, dir: &Path) -> anyhow::Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir() && p.join("init.lua").is_file())
        .collect();
    entries.sort();

    for plugin_dir in entries {
        let init_lua = plugin_dir.join("init.lua");
        let plugin_name = plugin_dir.file_name().unwrap_or_default().to_string_lossy();
        log::info!("[plugin] loading plugin: {plugin_name}");
        if let Err(e) = load_file(lua, &init_lua) {
            log::error!("[plugin] failed to load plugin '{plugin_name}': {e}");
            // Continue loading other plugins.
        }
    }

    Ok(())
}

fn user_plugins_dir() -> PathBuf {
    dirs::config_dir()
        .map(|d| d.join("loom").join("plugins"))
        .unwrap_or_else(|| PathBuf::from("/tmp/.config/loom/plugins"))
}

fn user_init_path() -> PathBuf {
    dirs::config_dir()
        .map(|d| d.join("loom").join("init.lua"))
        .unwrap_or_else(|| PathBuf::from("/tmp/.config/loom/init.lua"))
}

/// Convert mlua::Error (which is !Send) to anyhow::Error via Display.
fn lua_err(e: mlua::Error) -> anyhow::Error {
    anyhow::anyhow!("{e}")
}
