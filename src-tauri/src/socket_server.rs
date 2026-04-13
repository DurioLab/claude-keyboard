use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

use crate::ipc::{cleanup as ipc_cleanup, IpcListener, IpcStream, IPC_PATH};
use crate::permission::PermissionManager;
use crate::reveal_main_window;
use crate::tts::{self, Tts};
use crate::voice::VoiceManager;

/// Event received from Claude Code hook script
#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct HookEvent {
    pub session_id: String,
    pub cwd: String,
    pub event: String,
    pub status: String,
    pub pid: Option<u32>,
    pub tty: Option<String>,
    pub tool: Option<String>,
    pub tool_input: Option<serde_json::Value>,
    pub tool_use_id: Option<String>,
}

/// Response sent back to the hook script
#[derive(Debug, Serialize)]
pub struct HookResponse {
    pub decision: String,
    pub reason: Option<String>,
}

/// Voice command event payload sent to frontend
#[derive(Clone, serde::Serialize)]
struct VoiceCommandPayload {
    decision: String,
}

/// Pending permission request - holds the open IPC connection
struct PendingPermission {
    stream: IpcStream,
    #[allow(dead_code)]
    event: HookEvent,
}

/// Socket server state
pub struct SocketServer {
    pending: Arc<Mutex<Option<PendingPermission>>>,
}

impl SocketServer {
    pub fn new() -> Self {
        Self {
            pending: Arc::new(Mutex::new(None)),
        }
    }

    /// Start the IPC server in a background thread
    pub fn start(
        &self,
        app_handle: AppHandle,
        permission_mgr: Arc<PermissionManager>,
        voice_mgr: Option<Arc<VoiceManager>>,
        notify_mode: Arc<AtomicBool>,
    ) {
        let pending = self.pending.clone();

        std::thread::spawn(move || {
            // Clean up existing socket/pipe
            ipc_cleanup();

            let listener = match IpcListener::bind() {
                Ok(l) => l,
                Err(e) => {
                    log::error!("Failed to bind IPC at {}: {}", IPC_PATH, e);
                    return;
                }
            };

            log::info!("IPC server listening on {}", IPC_PATH);

            loop {
                match listener.accept() {
                    Ok(stream) => {
                        let app = app_handle.clone();
                        let perm = permission_mgr.clone();
                        let pending_clone = pending.clone();
                        let voice = voice_mgr.clone();
                        let mode = notify_mode.clone();
                        std::thread::spawn(move || {
                            handle_client(stream, app, perm, pending_clone, voice, mode);
                        });
                    }
                    Err(e) => {
                        log::error!("IPC accept error: {}", e);
                    }
                }
            }
        });
    }

    /// Send a response to the pending permission request
    pub fn respond(&self, decision: &str, reason: Option<String>) -> Result<(), String> {
        let mut pending_guard = self.pending.lock().unwrap();
        if let Some(mut pending) = pending_guard.take() {
            let response = HookResponse {
                decision: decision.to_string(),
                reason,
            };
            let data = serde_json::to_vec(&response).map_err(|e| e.to_string())?;
            match pending.stream.write_all(&data) {
                Ok(_) => {
                    log::info!(
                        "Sent response: {} for tool {:?}",
                        decision,
                        pending.event.tool
                    );
                    let _ = pending.stream.flush();
                    drop(pending);
                    Ok(())
                }
                Err(e) => {
                    log::error!("Failed to write response: {}", e);
                    Err(format!("Failed to write response: {}", e))
                }
            }
        } else {
            Err("No pending permission request".to_string())
        }
    }

    /// Return the current pending permission event, if any.
    pub fn pending_event(&self) -> Option<HookEvent> {
        let pending_guard = self.pending.lock().unwrap();
        pending_guard.as_ref().map(|pending| pending.event.clone())
    }
}

fn read_event(stream: &mut IpcStream) -> Option<Vec<u8>> {
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));

    let mut all_data = Vec::new();
    let mut buf = vec![0u8; 65536];

    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                all_data.extend_from_slice(&buf[..n]);
                if serde_json::from_slice::<serde_json::Value>(&all_data).is_ok() {
                    break;
                }
            }
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                if !all_data.is_empty() {
                    break;
                }
                continue;
            }
            Err(e) => {
                log::warn!("Read error: {}", e);
                break;
            }
        }
    }

    if all_data.is_empty() {
        None
    } else {
        Some(all_data)
    }
}

/// Play a brief system notification sound (sound mode only, macOS)
fn play_notification_sound() {
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("afplay")
            .arg("/System/Library/Sounds/Ping.aiff")
            .spawn();
    }
}

fn handle_client(
    mut stream: IpcStream,
    app: AppHandle,
    permission_mgr: Arc<PermissionManager>,
    pending: Arc<Mutex<Option<PendingPermission>>>,
    voice_mgr: Option<Arc<VoiceManager>>,
    notify_mode: Arc<AtomicBool>,
) {
    let data = match read_event(&mut stream) {
        Some(d) => d,
        None => return,
    };

    let event: HookEvent = match serde_json::from_slice(&data) {
        Ok(e) => e,
        Err(e) => {
            log::warn!("Failed to parse hook event: {}", e);
            return;
        }
    };

    log::info!(
        "Received event: {} status={} tool={:?}",
        event.event,
        event.status,
        event.tool
    );

    // Only handle permission requests specially
    if event.event == "PermissionRequest" && event.status == "waiting_for_approval" {
        let tool_name = event.tool.clone().unwrap_or_default();

        // Check allow-always whitelist
        if permission_mgr.is_whitelisted(&tool_name) {
            log::info!("Auto-allowing '{}' (whitelisted)", tool_name);
            let response = HookResponse {
                decision: "allow".to_string(),
                reason: None,
            };
            if let Ok(data) = serde_json::to_vec(&response) {
                let _ = stream.write_all(&data);
                let _ = stream.flush();
            }
            let _ = app.emit("permission-auto-approved", &event);
            return;
        }

        // Clear the read timeout
        let _ = stream.set_read_timeout(None);

        // Store the pending request
        log::info!(
            "Storing pending permission for tool: {:?}, keeping connection alive",
            event.tool
        );
        {
            let mut pending_guard = pending.lock().unwrap();
            if pending_guard.is_some() {
                log::warn!("Replacing existing pending permission request");
            }
            *pending_guard = Some(PendingPermission {
                stream,
                event: event.clone(),
            });
        }

        // Reveal the window from Rust as well so hidden-agent-window activation
        // does not depend on frontend listener timing.
        reveal_main_window(&app);

        let _ = app.emit("permission-request", &event);
        log::info!(
            "Permission request emitted to UI for tool: {:?}",
            event.tool
        );

        // Notification based on current mode
        let is_voice = notify_mode.load(Ordering::Relaxed);
        if is_voice {
            // Voice mode: TTS announces tool name, then listens for voice command
            if let Some(ref vm) = voice_mgr {
                let prompt = tts::format_permission_prompt(&tool_name);
                let vm_clone = vm.clone();
                let app_clone = app.clone();

                // Pause mic during TTS to avoid self-hearing
                vm.pause();

                std::thread::spawn(move || {
                    // Speak synchronously (blocks until done)
                    Tts::speak_sync(&prompt);

                    // Resume mic and start listening
                    vm_clone.resume();

                    if let Err(e) =
                        vm_clone.start_listening(app_clone.clone(), move |decision, text| {
                            log::info!(
                                "Voice command received: decision={}, text=\"{}\"",
                                decision,
                                text
                            );
                            let _ = app_clone.emit(
                                "voice-command",
                                VoiceCommandPayload {
                                    decision: decision.to_string(),
                                },
                            );
                        })
                    {
                        log::warn!("Failed to start voice listening: {}", e);
                    }
                });
            }
        } else {
            // Sound mode: play system notification chime, user responds via keyboard/mouse
            play_notification_sound();
        }

        return;
    }

    // For non-permission events, connection is closed when stream is dropped
    log::info!("Non-permission event processed: {}", event.event);
}

/// Cleanup IPC on exit
pub fn cleanup() {
    ipc_cleanup();
    log::info!("Cleaned up IPC");
}
