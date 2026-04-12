pub mod ipc;
pub mod tts;
pub mod voice;
mod hook_installer;
mod permission;
mod socket_server;

use permission::PermissionManager;
use socket_server::SocketServer;
use std::sync::Arc;
use tauri::Manager;
use voice::VoiceManager;

const WINDOW_WIDTH: f64 = 600.0;
const WINDOW_HEIGHT: f64 = 150.0;

#[tauri::command]
fn respond_permission(
    decision: String,
    tool_name: String,
    state: tauri::State<'_, AppState>,
) -> Result<(), String> {
    // Stop voice listening when user confirms via keyboard
    if let Some(vm) = &state.voice_mgr {
        vm.stop_listening();
    }
    // Stop any ongoing TTS
    tts::Tts::stop();

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

pub struct AppState {
    pub socket_server: Arc<SocketServer>,
    pub permission_mgr: Arc<PermissionManager>,
    pub voice_mgr: Option<Arc<VoiceManager>>,
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

            // Initialize VoiceManager (whisper model from resources)
            let voice_mgr = match app.path().resource_dir() {
                Ok(res_dir) => {
                    let model_path = res_dir.join("resources").join("ggml-tiny.bin");
                    let model_str = model_path.to_string_lossy().to_string();
                    match VoiceManager::new(&model_str) {
                        Ok(vm) => {
                            log::info!("VoiceManager initialized with model: {}", model_str);
                            Some(Arc::new(vm))
                        }
                        Err(e) => {
                            log::warn!("VoiceManager init failed (voice disabled): {}", e);
                            None
                        }
                    }
                }
                Err(e) => {
                    log::warn!("Could not resolve resource dir (voice disabled): {}", e);
                    None
                }
            };

            // Start socket server with voice manager
            socket_server.start(handle.clone(), permission_mgr.clone(), voice_mgr.clone());
            log::info!("Socket server started");

            // Store state
            app.manage(AppState {
                socket_server: socket_server.clone(),
                permission_mgr: permission_mgr.clone(),
                voice_mgr,
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
