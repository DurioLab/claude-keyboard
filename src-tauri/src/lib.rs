pub mod ipc;
mod hook_installer;
mod permission;
mod socket_server;

use permission::PermissionManager;
use socket_server::SocketServer;
use std::sync::Arc;
use tauri::Manager;

const WINDOW_WIDTH: f64 = 600.0;
const WINDOW_HEIGHT: f64 = 150.0;

#[tauri::command]
fn respond_permission(
    decision: String,
    tool_name: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    let actual_decision = if decision == "allow-always" {
        // Add to whitelist and respond with allow
        state.permission_mgr.add_to_whitelist(&tool_name);
        "allow"
    } else {
        &decision
    };

    let reason = if actual_decision == "deny" {
        Some("Denied by user via Claude Keyboard".to_string())
    } else {
        None
    };

    state.socket_server.respond(actual_decision, reason)
}

struct AppState {
    socket_server: Arc<SocketServer>,
    permission_mgr: Arc<PermissionManager>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let socket_server = Arc::new(SocketServer::new());
    let permission_mgr = Arc::new(PermissionManager::new());

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(move |app| {
            let handle = app.handle().clone();

            // Install hooks on startup
            hook_installer::install_hooks(&handle);
            log::info!("Hooks installed: {}", hook_installer::is_installed());

            // Position window at top center of screen
            if let Some(window) = app.get_webview_window("main") {
                if let Some(monitor) = window.current_monitor().ok().flatten() {
                    let screen_size = monitor.size();
                    let scale = monitor.scale_factor();
                    let screen_w = screen_size.width as f64 / scale;
                    let _screen_h = screen_size.height as f64 / scale;

                    let x = (screen_w - WINDOW_WIDTH) / 2.0;
                    let y = if cfg!(target_os = "macos") { 38.0 } else { 8.0 };

                    log::info!(
                        "Screen: {}x{} (scale {}), positioning at ({}, {})",
                        screen_size.width, screen_size.height, scale, x, y
                    );

                    let _ = window.set_position(tauri::LogicalPosition::new(x, y));
                }
            }

            // Windows vibrancy
            #[cfg(target_os = "windows")]
            {
                use window_vibrancy::apply_mica;
                if let Some(window) = app.get_webview_window("main") {
                    let _ = apply_mica(&window, None);
                }
            }

            // Start socket server
            socket_server.start(handle.clone(), permission_mgr.clone());
            log::info!("Socket server started");

            // Store state
            app.manage(AppState {
                socket_server: socket_server.clone(),
                permission_mgr: permission_mgr.clone(),
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![respond_permission])
        .on_window_event(|_window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                socket_server::cleanup();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
