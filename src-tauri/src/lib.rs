mod hook_installer;
pub mod ipc;
mod permission;
mod socket_server;
pub mod tts;
pub mod voice;

use permission::PermissionManager;
use socket_server::SocketServer;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::Manager;
use voice::VoiceManager;

const COMPACT_WIDTH: f64 = 220.0;
const COMPACT_HEIGHT: f64 = 52.0;
const EXPANDED_WIDTH: f64 = 520.0;
const EXPANDED_HEIGHT: f64 = 188.0;

fn position_window(window: &tauri::WebviewWindow, width: f64, height: f64) {
    let monitor = window
        .primary_monitor()
        .ok()
        .flatten()
        .or_else(|| window.current_monitor().ok().flatten());
    if let Some(monitor) = monitor {
        let screen_size = monitor.size();
        let scale = monitor.scale_factor();
        let screen_w = screen_size.width as f64 / scale;

        let x = (screen_w - width) / 2.0;
        let y = 0.0;

        log::info!(
            "Screen: {}x{} (scale {}), positioning at ({}, {})",
            screen_size.width,
            screen_size.height,
            scale,
            x,
            y
        );

        let _ = window.set_position(tauri::LogicalPosition::new(x, y));
    }

    let _ = window.set_size(tauri::LogicalSize::new(width, height));
}

pub(crate) fn reveal_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        position_window(&window, EXPANDED_WIDTH, EXPANDED_HEIGHT);

        #[cfg(target_os = "macos")]
        {
            let _ = window.set_visible_on_all_workspaces(true);
        }

        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    } else {
        log::warn!("Main window not found while trying to reveal permission UI");
    }
}

fn build_tray_menu(
    app: &tauri::AppHandle,
    is_voice: bool,
) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
    let sound_label = if !is_voice {
        "✓  音效提醒"
    } else {
        "    音效提醒"
    };
    let voice_label = if is_voice {
        "✓  语音提醒"
    } else {
        "    语音提醒"
    };
    let sound_item = MenuItem::with_id(app, "notify-sound", sound_label, true, None::<&str>)?;
    let voice_item = MenuItem::with_id(app, "notify-voice", voice_label, true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出 Claude Keyboard", true, None::<&str>)?;
    Menu::with_items(app, &[&sound_item, &voice_item, &sep, &quit])
}

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

#[tauri::command]
fn get_pending_permission(state: tauri::State<'_, AppState>) -> Option<socket_server::HookEvent> {
    state.socket_server.pending_event()
}

pub struct AppState {
    pub socket_server: Arc<SocketServer>,
    pub permission_mgr: Arc<PermissionManager>,
    pub voice_mgr: Option<Arc<VoiceManager>>,
    pub notify_mode: Arc<AtomicBool>, // false = sound, true = voice
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let socket_server = Arc::new(SocketServer::new());
    let permission_mgr = Arc::new(PermissionManager::new());
    let notify_mode = Arc::new(AtomicBool::new(false)); // Default: sound mode

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .setup(move |app| {
            let handle = app.handle().clone();

            // Install hooks on startup
            hook_installer::install_hooks(&handle);
            log::info!("Hooks installed: {}", hook_installer::is_installed());

            // Position window at top center of primary screen (window starts hidden)
            if let Some(window) = app.get_webview_window("main") {
                position_window(&window, COMPACT_WIDTH, COMPACT_HEIGHT);
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

            // Create system tray icon with notification mode menu
            let notify_mode_tray = notify_mode.clone();
            let initial_menu = build_tray_menu(&handle, false)?;
            tauri::tray::TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&initial_menu)
                .show_menu_on_left_click(true)
                .on_menu_event(move |app, event| match event.id.as_ref() {
                    "notify-sound" => {
                        notify_mode_tray.store(false, Ordering::Relaxed);
                        if let Ok(menu) = build_tray_menu(app, false) {
                            if let Some(tray) = app.tray_by_id("main") {
                                let _ = tray.set_menu(Some(menu));
                            }
                        }
                    }
                    "notify-voice" => {
                        notify_mode_tray.store(true, Ordering::Relaxed);
                        if let Ok(menu) = build_tray_menu(app, true) {
                            if let Some(tray) = app.tray_by_id("main") {
                                let _ = tray.set_menu(Some(menu));
                            }
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .build(app)?;

            // Start socket server with notification mode
            let reveal_handle = handle.clone();
            socket_server.start(
                reveal_handle.clone(),
                permission_mgr.clone(),
                voice_mgr.clone(),
                notify_mode.clone(),
            );
            log::info!("Socket server started");

            // Store state
            app.manage(AppState {
                socket_server: socket_server.clone(),
                permission_mgr: permission_mgr.clone(),
                voice_mgr,
                notify_mode,
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            respond_permission,
            get_pending_permission
        ])
        .on_window_event(|_window, event| {
            if let tauri::WindowEvent::Destroyed = event {
                socket_server::cleanup();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
