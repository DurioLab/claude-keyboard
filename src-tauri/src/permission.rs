use std::collections::HashMap;
use std::sync::Mutex;

/// Manages "Allow Always" whitelist for tools
pub struct PermissionManager {
    /// tool_name -> always allowed
    whitelist: Mutex<HashMap<String, bool>>,
}

impl PermissionManager {
    pub fn new() -> Self {
        Self {
            whitelist: Mutex::new(HashMap::new()),
        }
    }

    /// Check if a tool is whitelisted (allow always)
    pub fn is_whitelisted(&self, tool_name: &str) -> bool {
        let wl = self.whitelist.lock().unwrap();
        wl.get(tool_name).copied().unwrap_or(false)
    }

    /// Add a tool to the whitelist
    pub fn add_to_whitelist(&self, tool_name: &str) {
        let mut wl = self.whitelist.lock().unwrap();
        wl.insert(tool_name.to_string(), true);
        log::info!("Added '{}' to allow-always whitelist", tool_name);
    }
}
