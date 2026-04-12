use serde_json;
use std::fs;
use std::path::PathBuf;

#[cfg(unix)]
const HOOK_BINARY_NAME: &str = "claude-keyboard-hook";
#[cfg(windows)]
const HOOK_BINARY_NAME: &str = "claude-keyboard-hook.exe";

const HOOK_MARKER: &str = "claude-keyboard-hook";

/// Get the path to ~/.claude/
fn claude_dir() -> PathBuf {
    dirs::home_dir()
        .expect("Cannot find home directory")
        .join(".claude")
}

/// Install the hook binary and register it in settings.json
pub fn install_hooks(app_handle: &tauri::AppHandle) {
    let claude = claude_dir();
    let hooks_dir = claude.join("hooks");
    let settings_path = claude.join("settings.json");

    // Create hooks directory
    let _ = fs::create_dir_all(&hooks_dir);

    // Copy the hook binary from bundled resources
    let binary_dest = hooks_dir.join(HOOK_BINARY_NAME);
    if let Ok(resource_path) = app_handle
        .path()
        .resolve(
            &format!("resources/{}", HOOK_BINARY_NAME),
            tauri::path::BaseDirectory::Resource,
        )
    {
        let _ = fs::copy(&resource_path, &binary_dest);
    } else {
        // Fallback for dev mode: look for compiled binary in target directory
        let manifest_dir = env!("CARGO_MANIFEST_DIR");
        let target_debug = PathBuf::from(manifest_dir)
            .join("target")
            .join("debug")
            .join(HOOK_BINARY_NAME);
        let target_release = PathBuf::from(manifest_dir)
            .join("target")
            .join("release")
            .join(HOOK_BINARY_NAME);

        if target_release.exists() {
            let _ = fs::copy(&target_release, &binary_dest);
        } else if target_debug.exists() {
            let _ = fs::copy(&target_debug, &binary_dest);
        } else {
            log::warn!(
                "Hook binary not found in resources or target directory. Looked at: {:?}, {:?}",
                target_debug,
                target_release
            );
        }
    }

    // Make executable (Unix only)
    make_executable(&binary_dest);

    log::info!("Hook binary installed to {:?}", binary_dest);

    // Update settings.json
    update_settings(&settings_path, &hooks_dir);
}

/// Set executable permissions on Unix, no-op on Windows
#[cfg(unix)]
fn make_executable(path: &std::path::Path) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o755));
}

#[cfg(windows)]
fn make_executable(_path: &std::path::Path) {
    // No-op on Windows: executables don't need permission bits
}

/// Check if hooks are installed
pub fn is_installed() -> bool {
    let settings_path = claude_dir().join("settings.json");
    if let Ok(data) = fs::read_to_string(&settings_path) {
        data.contains(HOOK_MARKER)
    } else {
        false
    }
}

/// Uninstall hooks
pub fn uninstall() {
    let claude = claude_dir();
    let binary_path = claude.join("hooks").join(HOOK_BINARY_NAME);
    let settings_path = claude.join("settings.json");

    // Remove binary
    let _ = fs::remove_file(&binary_path);

    // Remove from settings.json
    if let Ok(data) = fs::read_to_string(&settings_path) {
        if let Ok(mut json) = serde_json::from_str::<serde_json::Value>(&data) {
            if let Some(hooks) = json.get_mut("hooks").and_then(|h| h.as_object_mut()) {
                for (_event, entries) in hooks.iter_mut() {
                    if let Some(arr) = entries.as_array_mut() {
                        arr.retain(|entry| {
                            let has_our_hook = entry
                                .get("hooks")
                                .and_then(|h| h.as_array())
                                .map(|hooks| {
                                    hooks.iter().any(|h| {
                                        h.get("command")
                                            .and_then(|c| c.as_str())
                                            .map(|c| c.contains(HOOK_MARKER))
                                            .unwrap_or(false)
                                    })
                                })
                                .unwrap_or(false);
                            !has_our_hook
                        });
                    }
                }
                // Remove empty hook events
                hooks.retain(|_, v| {
                    v.as_array().map(|a| !a.is_empty()).unwrap_or(true)
                });
            }
            if let Ok(output) = serde_json::to_string_pretty(&json) {
                let _ = fs::write(&settings_path, output);
            }
        }
    }

    log::info!("Hooks uninstalled");
}

fn update_settings(settings_path: &PathBuf, hooks_dir: &PathBuf) {
    // Read existing settings
    let mut json: serde_json::Value = if let Ok(data) = fs::read_to_string(settings_path) {
        serde_json::from_str(&data).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    let hook_path = hooks_dir.join(HOOK_BINARY_NAME);
    let command = hook_path.to_string_lossy().to_string();

    let hook_entry = serde_json::json!([{"type": "command", "command": command}]);
    let hook_entry_with_timeout =
        serde_json::json!([{"type": "command", "command": command, "timeout": 86400}]);

    // Define hook events we need
    let hook_events: Vec<(&str, serde_json::Value)> = vec![
        (
            "PreToolUse",
            serde_json::json!([{"matcher": "*", "hooks": hook_entry}]),
        ),
        (
            "PostToolUse",
            serde_json::json!([{"matcher": "*", "hooks": hook_entry}]),
        ),
        (
            "PermissionRequest",
            serde_json::json!([{"matcher": "*", "hooks": hook_entry_with_timeout}]),
        ),
        (
            "Stop",
            serde_json::json!([{"hooks": hook_entry}]),
        ),
        (
            "SessionStart",
            serde_json::json!([{"hooks": hook_entry}]),
        ),
        (
            "SessionEnd",
            serde_json::json!([{"hooks": hook_entry}]),
        ),
    ];

    // Get or create hooks object
    if json.get("hooks").is_none() {
        json["hooks"] = serde_json::json!({});
    }

    let hooks = json.get_mut("hooks").unwrap().as_object_mut().unwrap();

    for (event, config) in hook_events {
        if let Some(existing) = hooks.get(event) {
            // Check if our hook is already there
            let has_ours = existing
                .as_array()
                .map(|entries| {
                    entries.iter().any(|entry| {
                        entry
                            .get("hooks")
                            .and_then(|h| h.as_array())
                            .map(|hooks_arr| {
                                hooks_arr.iter().any(|h| {
                                    h.get("command")
                                        .and_then(|c| c.as_str())
                                        .map(|c| c.contains(HOOK_MARKER))
                                        .unwrap_or(false)
                                })
                            })
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false);

            if !has_ours {
                // Append our config to existing entries
                if let Some(existing_arr) = hooks.get_mut(event).and_then(|v| v.as_array_mut()) {
                    if let Some(config_arr) = config.as_array() {
                        existing_arr.extend(config_arr.iter().cloned());
                    }
                }
            }
        } else {
            hooks.insert(event.to_string(), config);
        }
    }

    // Write back
    if let Ok(output) = serde_json::to_string_pretty(&json) {
        let _ = fs::write(settings_path, output);
        log::info!("Updated settings.json with hooks");
    }
}

use tauri::Manager;
