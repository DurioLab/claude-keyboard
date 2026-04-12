use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

use crate::permission::PermissionManager;

const SOCKET_PATH: &str = "/tmp/claude-keyboard.sock";

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

/// Pending permission request - holds the open socket connection
struct PendingPermission {
    stream: UnixStream,
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

    /// Start the Unix socket server in a background thread
    pub fn start(&self, app_handle: AppHandle, permission_mgr: Arc<PermissionManager>) {
        let pending = self.pending.clone();

        std::thread::spawn(move || {
            // Clean up existing socket
            let _ = std::fs::remove_file(SOCKET_PATH);

            let listener = match UnixListener::bind(SOCKET_PATH) {
                Ok(l) => l,
                Err(e) => {
                    log::error!("Failed to bind socket at {}: {}", SOCKET_PATH, e);
                    return;
                }
            };

            // Make socket world-writable so the hook script can connect
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(
                    SOCKET_PATH,
                    std::fs::Permissions::from_mode(0o777),
                );
            }

            log::info!("Socket server listening on {}", SOCKET_PATH);

            for stream in listener.incoming() {
                match stream {
                    Ok(stream) => {
                        let app = app_handle.clone();
                        let perm = permission_mgr.clone();
                        let pending_clone = pending.clone();
                        // Handle each client in its own thread to avoid blocking
                        std::thread::spawn(move || {
                            handle_client(stream, app, perm, pending_clone);
                        });
                    }
                    Err(e) => {
                        log::error!("Socket accept error: {}", e);
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
                    // Explicitly flush and shutdown
                    let _ = pending.stream.flush();
                    let _ = pending
                        .stream
                        .shutdown(std::net::Shutdown::Both);
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
}

fn read_event(stream: &mut UnixStream) -> Option<Vec<u8>> {
    // Set a read timeout for initial data
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_secs(5)));

    let mut all_data = Vec::new();
    let mut buf = vec![0u8; 65536];

    loop {
        match stream.read(&mut buf) {
            Ok(0) => break, // EOF - client closed write side
            Ok(n) => {
                all_data.extend_from_slice(&buf[..n]);
                // If we can parse as valid JSON, we have the full message
                if serde_json::from_slice::<serde_json::Value>(&all_data).is_ok() {
                    break;
                }
            }
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                // Timeout - if we have data, try to use it
                if !all_data.is_empty() {
                    break;
                }
                // Otherwise keep waiting a bit
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

fn handle_client(
    mut stream: UnixStream,
    app: AppHandle,
    permission_mgr: Arc<PermissionManager>,
    pending: Arc<Mutex<Option<PendingPermission>>>,
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
            // Notify frontend
            let _ = app.emit("permission-auto-approved", &event);
            return;
        }

        // Clear the read timeout - this stream needs to stay open until user decides
        let _ = stream.set_read_timeout(None);

        // Store the pending request (keep socket open for response)
        log::info!(
            "Storing pending permission for tool: {:?}, keeping socket alive",
            event.tool
        );
        {
            let mut pending_guard = pending.lock().unwrap();
            // Drop any previous pending (closes old socket)
            if pending_guard.is_some() {
                log::warn!("Replacing existing pending permission request");
            }
            *pending_guard = Some(PendingPermission {
                stream,
                event: event.clone(),
            });
        }

        // Emit event to frontend to show the keyboard UI
        let _ = app.emit("permission-request", &event);
        log::info!("Permission request emitted to UI for tool: {:?}", event.tool);

        // NOTE: We intentionally do NOT return or drop the stream here.
        // The stream lives inside PendingPermission until respond() is called.
        return;
    }

    // For non-permission events, connection is closed when stream is dropped
    log::info!("Non-permission event processed: {}", event.event);
}

/// Cleanup socket on exit
pub fn cleanup() {
    let path = Path::new(SOCKET_PATH);
    if path.exists() {
        let _ = std::fs::remove_file(path);
        log::info!("Cleaned up socket file");
    }
}
