use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::thread;

/// Global handle to the current TTS subprocess, used by stop() to kill it.
static CURRENT_PROCESS: std::sync::LazyLock<Arc<Mutex<Option<Child>>>> =
    std::sync::LazyLock::new(|| Arc::new(Mutex::new(None)));

/// Cross-platform text-to-speech via system commands.
pub struct Tts;

impl Tts {
    /// Speak text asynchronously in a background thread.
    /// Returns a JoinHandle so the caller can optionally wait for completion.
    pub fn speak(text: &str) -> thread::JoinHandle<()> {
        let text = text.to_string();
        thread::spawn(move || {
            Self::speak_inner(&text);
        })
    }

    /// Speak text synchronously, blocking until playback finishes.
    pub fn speak_sync(text: &str) {
        Self::speak_inner(text);
    }

    /// Stop the currently playing TTS by killing the subprocess.
    pub fn stop() {
        let mut guard = CURRENT_PROCESS.lock().unwrap();
        if let Some(ref mut child) = *guard {
            if let Err(e) = child.kill() {
                log::warn!("Failed to kill TTS process: {}", e);
            } else {
                log::info!("TTS playback stopped");
                // Reap the zombie
                let _ = child.wait();
            }
        }
        *guard = None;
    }

    /// Internal: spawn the platform TTS command and wait for it.
    fn speak_inner(text: &str) {
        if text.is_empty() {
            return;
        }

        let child = Self::spawn_platform_tts(text);
        match child {
            Ok(child) => {
                // Store the child so stop() can kill it, then release the lock
                // before blocking on wait() to avoid deadlock with stop().
                {
                    let mut guard = CURRENT_PROCESS.lock().unwrap();
                    *guard = Some(child);
                }
                // Poll in a loop so we don't hold the lock while waiting
                loop {
                    let status = {
                        let mut guard = CURRENT_PROCESS.lock().unwrap();
                        if let Some(ref mut c) = *guard {
                            c.try_wait()
                        } else {
                            // Process was killed by stop()
                            break;
                        }
                    };
                    match status {
                        Ok(Some(exit_status)) => {
                            if !exit_status.success() {
                                log::warn!("TTS process exited with status: {}", exit_status);
                            }
                            let mut guard = CURRENT_PROCESS.lock().unwrap();
                            *guard = None;
                            break;
                        }
                        Ok(None) => {
                            // Still running, sleep briefly
                            std::thread::sleep(std::time::Duration::from_millis(50));
                        }
                        Err(e) => {
                            log::warn!("Failed to check TTS process status: {}", e);
                            let mut guard = CURRENT_PROCESS.lock().unwrap();
                            *guard = None;
                            break;
                        }
                    }
                }
            }
            Err(e) => {
                log::warn!("Failed to spawn TTS process: {}", e);
            }
        }
    }

    /// Spawn the platform-specific TTS command.
    #[cfg(target_os = "macos")]
    fn spawn_platform_tts(text: &str) -> std::io::Result<Child> {
        let sanitized = sanitize_text(text);
        let mut cmd = Command::new("say");
        if contains_chinese(&sanitized) {
            cmd.arg("-v").arg("Tingting");
        }
        cmd.arg(&sanitized);
        cmd.spawn()
    }

    #[cfg(target_os = "windows")]
    fn spawn_platform_tts(text: &str) -> std::io::Result<Child> {
        let sanitized = sanitize_for_powershell(text);
        let script = format!(
            "Add-Type -AssemblyName System.Speech; \
             (New-Object System.Speech.Synthesis.SpeechSynthesizer).Speak('{}')",
            sanitized
        );
        Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .spawn()
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    fn spawn_platform_tts(text: &str) -> std::io::Result<Child> {
        // Linux fallback: try espeak
        let sanitized = sanitize_text(text);
        Command::new("espeak").arg(&sanitized).spawn()
    }
}

/// Format a tool name into a spoken permission prompt.
///
/// Examples:
/// - "Bash" → "Bash 请求权限"
/// - "Read" → "Read 请求权限"
pub fn format_permission_prompt(tool_name: &str) -> String {
    format!("{} 请求权限", tool_name)
}

/// Check if text contains any CJK Unified Ideograph (Chinese characters).
fn contains_chinese(text: &str) -> bool {
    text.chars().any(|c| {
        matches!(c,
            '\u{4E00}'..='\u{9FFF}'   // CJK Unified Ideographs
            | '\u{3400}'..='\u{4DBF}' // CJK Extension A
            | '\u{F900}'..='\u{FAFF}' // CJK Compatibility Ideographs
        )
    })
}

/// Sanitize text for safe use as a shell argument on macOS/Linux.
/// Removes characters that could cause shell injection.
fn sanitize_text(text: &str) -> String {
    text.chars()
        .filter(|c| {
            c.is_alphanumeric()
                || c.is_whitespace()
                || matches!(c, '.' | ',' | '!' | '?' | '-' | '_' | ':' | '(' | ')')
                // Keep CJK characters
                || ('\u{4E00}'..='\u{9FFF}').contains(c)
                || ('\u{3400}'..='\u{4DBF}').contains(c)
                || ('\u{F900}'..='\u{FAFF}').contains(c)
        })
        .collect()
}

/// Sanitize text for embedding in a PowerShell single-quoted string.
/// In PowerShell single-quoted strings, the only escape is '' for a literal '.
#[cfg(target_os = "windows")]
fn sanitize_for_powershell(text: &str) -> String {
    text.replace('\'', "''")
        .chars()
        .filter(|c| {
            c.is_alphanumeric()
                || c.is_whitespace()
                || matches!(
                    c,
                    '.' | ',' | '!' | '?' | '-' | '_' | ':' | '(' | ')' | '\''
                )
                || ('\u{4E00}'..='\u{9FFF}').contains(c)
                || ('\u{3400}'..='\u{4DBF}').contains(c)
                || ('\u{F900}'..='\u{FAFF}').contains(c)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_contains_chinese() {
        assert!(contains_chinese("你好"));
        assert!(contains_chinese("Hello 世界"));
        assert!(!contains_chinese("Hello World"));
        assert!(!contains_chinese("Bash"));
    }

    #[test]
    fn test_sanitize_text() {
        assert_eq!(sanitize_text("hello world"), "hello world");
        assert_eq!(sanitize_text("Bash 请求权限"), "Bash 请求权限");
        // Dangerous characters stripped
        assert_eq!(sanitize_text("test; rm -rf /"), "test rm -rf ");
        assert_eq!(sanitize_text("$(evil)"), "(evil)");
        assert_eq!(sanitize_text("hello`whoami`"), "hellowhoami");
    }

    #[test]
    fn test_format_permission_prompt() {
        assert_eq!(format_permission_prompt("Bash"), "Bash 请求权限");
        assert_eq!(format_permission_prompt("Read"), "Read 请求权限");
    }
}
