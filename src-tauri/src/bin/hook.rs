//! Claude Keyboard Hook (Rust)
//!
//! Replaces `resources/claude-keyboard.py`.
//! Self-contained binary — no dependency on the main Tauri crate.
//!
//! Build:  cargo build --bin hook [--release]

use serde_json::Value;
use std::io::{self, Read, Write};
use std::process::Command;
use std::time::Duration;

// ───── IPC ──────────────────────────────────────────────────────────────────

#[cfg(unix)]
const SOCKET_PATH: &str = "/tmp/claude-keyboard.sock";

#[cfg(unix)]
fn ipc_send(payload: &[u8], wait_response: bool) -> Option<Vec<u8>> {
    use std::net::Shutdown;
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(SOCKET_PATH).ok()?;

    if wait_response {
        stream
            .set_read_timeout(Some(Duration::from_secs(300)))
            .ok()?;
    }

    stream.write_all(payload).ok()?;
    stream.shutdown(Shutdown::Write).ok()?;

    if wait_response {
        let mut buf = Vec::with_capacity(4096);
        stream.read_to_end(&mut buf).ok()?;
        if buf.is_empty() {
            None
        } else {
            Some(buf)
        }
    } else {
        None
    }
}

#[cfg(windows)]
fn ipc_send(payload: &[u8], wait_response: bool) -> Option<Vec<u8>> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr;

    const PIPE_NAME: &str = r"\\.\pipe\claude-keyboard";
    const GENERIC_READ: u32 = 0x80000000;
    const GENERIC_WRITE: u32 = 0x40000000;
    const OPEN_EXISTING: u32 = 3;
    const INVALID_HANDLE_VALUE: isize = -1;

    #[link(name = "kernel32")]
    extern "system" {
        fn CreateFileW(
            name: *const u16,
            access: u32,
            share: u32,
            sa: *mut u8,
            disp: u32,
            flags: u32,
            template: isize,
        ) -> isize;
        fn WriteFile(h: isize, buf: *const u8, len: u32, written: *mut u32, ovl: *mut u8) -> i32;
        fn ReadFile(h: isize, buf: *mut u8, len: u32, read: *mut u32, ovl: *mut u8) -> i32;
        fn CloseHandle(h: isize) -> i32;
        fn FlushFileBuffers(h: isize) -> i32;
    }

    let wide: Vec<u16> = OsStr::new(PIPE_NAME)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            0,
            ptr::null_mut(),
            OPEN_EXISTING,
            0,
            0,
        )
    };

    if handle == INVALID_HANDLE_VALUE {
        return None;
    }

    // Write payload
    let mut written: u32 = 0;
    let ok = unsafe {
        WriteFile(
            handle,
            payload.as_ptr(),
            payload.len() as u32,
            &mut written,
            ptr::null_mut(),
        )
    };
    if ok == 0 {
        unsafe { CloseHandle(handle) };
        return None;
    }
    unsafe { FlushFileBuffers(handle) };

    if wait_response {
        let mut buf = [0u8; 4096];
        let mut nread: u32 = 0;
        let ok = unsafe {
            ReadFile(
                handle,
                buf.as_mut_ptr(),
                buf.len() as u32,
                &mut nread,
                ptr::null_mut(),
            )
        };
        unsafe { CloseHandle(handle) };
        if ok != 0 && nread > 0 {
            Some(buf[..nread as usize].to_vec())
        } else {
            None
        }
    } else {
        unsafe { CloseHandle(handle) };
        None
    }
}

// ───── TTY helper ───────────────────────────────────────────────────────────

#[cfg(unix)]
fn get_tty() -> Option<String> {
    let ppid = std::os::unix::process::parent_id();
    let output = Command::new("ps")
        .args(["-p", &ppid.to_string(), "-o", "tty="])
        .output()
        .ok()?;
    let tty = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if tty.is_empty() || tty == "??" || tty == "-" {
        return None;
    }
    if tty.starts_with("/dev/") {
        Some(tty)
    } else {
        Some(format!("/dev/{}", tty))
    }
}

#[cfg(windows)]
fn get_tty() -> Option<String> {
    None
}

// ───── Main ─────────────────────────────────────────────────────────────────

fn main() {
    // 1. Read JSON from stdin
    let mut input = String::new();
    if io::stdin().read_to_string(&mut input).is_err() {
        std::process::exit(1);
    }
    let data: Value = match serde_json::from_str(&input) {
        Ok(v) => v,
        Err(_) => std::process::exit(1),
    };

    let session_id = data["session_id"].as_str().unwrap_or("unknown");
    let event = data["hook_event_name"].as_str().unwrap_or("");
    let cwd = data["cwd"].as_str().unwrap_or("");
    let tool_input = &data["tool_input"];
    let tool_name = data["tool_name"].as_str();
    let tool_use_id = data["tool_use_id"].as_str();

    #[cfg(unix)]
    let pid = std::os::unix::process::parent_id();
    #[cfg(windows)]
    let pid = std::process::id(); // fallback

    let tty = get_tty();

    // 2. Build state object
    let mut state = serde_json::json!({
        "session_id": session_id,
        "cwd": cwd,
        "event": event,
        "pid": pid,
        "tty": tty,
    });

    let is_permission_request;

    match event {
        "PreToolUse" => {
            state["status"] = "running_tool".into();
            if let Some(t) = tool_name {
                state["tool"] = t.into();
            }
            if !tool_input.is_null() {
                state["tool_input"] = tool_input.clone();
            }
            if let Some(id) = tool_use_id {
                state["tool_use_id"] = id.into();
            }
            is_permission_request = false;
        }
        "PostToolUse" => {
            state["status"] = "processing".into();
            if let Some(t) = tool_name {
                state["tool"] = t.into();
            }
            if !tool_input.is_null() {
                state["tool_input"] = tool_input.clone();
            }
            if let Some(id) = tool_use_id {
                state["tool_use_id"] = id.into();
            }
            is_permission_request = false;
        }
        "PermissionRequest" => {
            state["status"] = "waiting_for_approval".into();
            if let Some(t) = tool_name {
                state["tool"] = t.into();
            }
            if !tool_input.is_null() {
                state["tool_input"] = tool_input.clone();
            }
            is_permission_request = true;
        }
        "Stop" => {
            state["status"] = "waiting_for_input".into();
            is_permission_request = false;
        }
        "SessionStart" => {
            state["status"] = "waiting_for_input".into();
            is_permission_request = false;
        }
        "SessionEnd" => {
            state["status"] = "ended".into();
            is_permission_request = false;
        }
        _ => {
            state["status"] = "unknown".into();
            is_permission_request = false;
        }
    }

    // 3. Send via IPC
    let payload = serde_json::to_vec(&state).unwrap_or_default();

    if is_permission_request {
        // Send and wait for response
        if let Some(resp_bytes) = ipc_send(&payload, true) {
            if let Ok(resp) = serde_json::from_slice::<Value>(&resp_bytes) {
                let decision = resp["decision"].as_str().unwrap_or("ask");

                match decision {
                    "allow" => {
                        let output = serde_json::json!({
                            "hookSpecificOutput": {
                                "hookEventName": "PermissionRequest",
                                "decision": { "behavior": "allow" }
                            }
                        });
                        println!("{}", output);
                    }
                    "deny" => {
                        let reason = resp["reason"]
                            .as_str()
                            .unwrap_or("Denied by user via Claude Keyboard");
                        let output = serde_json::json!({
                            "hookSpecificOutput": {
                                "hookEventName": "PermissionRequest",
                                "decision": {
                                    "behavior": "deny",
                                    "message": reason
                                }
                            }
                        });
                        println!("{}", output);
                    }
                    _ => {
                        // "ask" or unknown — fall through to Claude Code's normal UI
                    }
                }
            }
        }
        // No response or "ask" — exit silently, Claude Code shows its own UI
    } else {
        // Fire and forget
        let _ = ipc_send(&payload, false);
    }
}
