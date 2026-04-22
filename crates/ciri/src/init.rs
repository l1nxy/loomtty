use anyhow::Result;
use ciri_config::config::config_path;
use dialoguer::{Input, Select, theme::ColorfulTheme};
use std::process::Command;

pub fn run_init() -> Result<()> {
    let path = config_path();

    if path.exists() {
        let overwrite = Select::with_theme(&ColorfulTheme::default())
            .with_prompt(format!("Config already exists at {}", path.display()))
            .items(&["Overwrite", "Cancel"])
            .default(1)
            .interact()?;
        if overwrite == 1 {
            println!("Cancelled.");
            return Ok(());
        }
    }

    println!("\n  Welcome to ciritty! Let's set up your configuration.\n");

    // 1. Keybinding style
    let mode_idx = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Keybinding style")
        .items(&[
            "Prefix  (tmux-style: leader + key, one action per press)",
            "Sticky  (zellij-style: leader enters mode, chain actions, Esc to exit)",
        ])
        .default(0)
        .interact()?;
    let mode = if mode_idx == 0 { "prefix" } else { "sticky" };

    // 2. Leader key
    let leader_idx = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Leader key")
        .items(&["Ctrl+W", "Ctrl+A", "Alt", "Custom"])
        .default(0)
        .interact()?;
    let leader = match leader_idx {
        0 => "ctrl+w".to_string(),
        1 => "ctrl+a".to_string(),
        2 => "alt".to_string(),
        _ => {
            let custom: String = Input::with_theme(&ColorfulTheme::default())
                .with_prompt("Enter leader key (e.g. ctrl+space, super+a)")
                .validate_with(|input: &String| -> Result<(), &str> {
                    if input.trim().is_empty() {
                        Err("Leader key cannot be empty")
                    } else {
                        Ok(())
                    }
                })
                .interact_text()?;
            custom
        }
    };

    // 3. Theme
    let themes = [
        "one_dark",
        "catppuccin_mocha",
        "tokyo_night",
        "dracula",
        "nord",
        "gruvbox_dark",
    ];
    let theme_idx = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Color theme")
        .items(&themes)
        .default(0)
        .interact()?;
    let theme = themes[theme_idx];

    // 4. Status bar position
    let statusbar_position_idx = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Status bar position")
        .items(&["Top", "Bottom"])
        .default(0)
        .interact()?;
    let statusbar_position = if statusbar_position_idx == 0 {
        "top"
    } else {
        "bottom"
    };

    // 5. Shell and font (auto-detect)
    let default_shell = detect_shell();
    let default_font = detect_font_family();

    // Generate config
    let config = generate_config(
        mode,
        &leader,
        theme,
        statusbar_position,
        &default_shell,
        &default_font,
    );

    // Write
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, &config)?;

    println!("\n  Config written to {}", path.display());
    println!("  Summary:");
    println!("    detected shell: {}", default_shell);
    println!("    detected font: {}", default_font);
    println!("    status bar: {}", statusbar_position);
    println!("  Run `ciritty` to start!\n");

    Ok(())
}

fn detect_shell() -> String {
    // Unix: use $SHELL
    #[cfg(unix)]
    {
        if let Ok(shell) = std::env::var("SHELL")
            && !shell.is_empty()
        {
            return shell;
        }
        "/bin/sh".to_string()
    }

    // Windows: probe PATH for common shells
    #[cfg(windows)]
    {
        for candidate in &["nu.exe", "pwsh.exe", "powershell.exe"] {
            if Command::new("where")
                .arg(candidate)
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false)
            {
                return candidate.to_string();
            }
        }
        "cmd.exe".to_string()
    }

    // Fallback for other platforms
    #[cfg(not(any(unix, windows)))]
    {
        "sh".to_string()
    }
}

fn detect_font_family() -> String {
    #[cfg(unix)]
    {
        if let Ok(output) = Command::new("fc-match")
            .args(["monospace", "--format=%{family[0]}"])
            .output()
            && output.status.success()
        {
            let family = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !family.is_empty() {
                return family;
            }
        }
        "monospace".to_string()
    }

    #[cfg(windows)]
    {
        // Cascadia Mono ships with Windows 10+ and is our preferred default;
        // `Consolas` remains the final fallback for the `#[cfg(not(any(...)))]`
        // arm. The original `for ... { return candidate }` loop was a no-op
        // that always returned the first entry, so collapse it.
        "Cascadia Mono".to_string()
    }

    #[cfg(not(any(unix, windows)))]
    {
        "monospace".to_string()
    }
}

/// Escape a string for safe embedding in a TOML double-quoted value.
fn escape_toml_string(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

fn generate_config(
    mode: &str,
    leader: &str,
    theme: &str,
    statusbar_position: &str,
    shell: &str,
    font: &str,
) -> String {
    let font_family = escape_toml_string(font);
    let shell = escape_toml_string(shell);
    let leader = escape_toml_string(leader);

    format!(
        r#"# Ciritty configuration — generated by `ciritty init`

[font]
family = "{font_family}"
size = 10.0

[appearance]
padding = 4.0
column_gap = 8.0
border_width = 2.0

[terminal]
shell = "{shell}"

[statusbar]
position = "{statusbar_position}"

[input]
mode = "{mode}"

[theme]
preset = "{theme}"

[keys]
leader = "{leader}"

[keys.bindings]
n = "new_column_right"
d = "new_row_below"
x = "close_pane"
h = "focus_left"
l = "focus_right"
k = "focus_up"
j = "focus_down"
"shift+h" = "move_pane_left"
"shift+l" = "move_pane_right"
"[" = "column_width_decrease"
"]" = "column_width_increase"
r = "cycle_preset_width"
"shift+r" = "cycle_preset_width_reverse"
f = "column_width_full"
b = "toggle_broadcast"
c = "consume_into_column"
e = "expel_from_column"
o = "toggle_overview"
tab = "toggle_overview"
q = "detach"
p = "toggle_command_palette"

[keys.overview_bindings]
h = "focus_left"
l = "focus_right"
j = "focus_down"
k = "focus_up"
x = "close_pane"
n = "new_column_right"
escape = "exit_overview"
enter = "exit_overview"
o = "exit_overview"
tab = "exit_overview"
"#
    )
}
