//! Platform detection and capabilities

#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    MacOS,
    Linux,
    Wsl1,
    Wsl2,
    Windows,
    Unknown,
}

impl Platform {
    pub fn detect() -> Self {
        #[cfg(target_os = "macos")]
        return Platform::MacOS;

        #[cfg(target_os = "linux")]
        return detect_linux_platform();

        #[cfg(target_os = "windows")]
        return Platform::Windows;

        #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
        return Platform::Unknown;
    }

    pub fn name(&self) -> &'static str {
        match self {
            Platform::MacOS => "macos",
            Platform::Linux => "linux",
            Platform::Wsl1 => "wsl1",
            Platform::Wsl2 => "wsl2",
            Platform::Windows => "windows",
            Platform::Unknown => "unknown",
        }
    }

    pub fn supports_unix_sockets(&self) -> bool {
        match self {
            Platform::MacOS | Platform::Linux | Platform::Wsl2 => true,
            Platform::Wsl1 | Platform::Windows | Platform::Unknown => false,
        }
    }
}

#[cfg(target_os = "linux")]
fn detect_linux_platform() -> Platform {
    // Check for WSL
    if let Ok(version) = std::fs::read_to_string("/proc/version") {
        let version_lower = version.to_lowercase();
        if version_lower.contains("microsoft") || version_lower.contains("wsl") {
            // Distinguish WSL1 vs WSL2
            if is_wsl2() {
                return Platform::Wsl2;
            } else {
                return Platform::Wsl1;
            }
        }
    }

    Platform::Linux
}

#[cfg(target_os = "linux")]
fn is_wsl2() -> bool {
    // WSL2 uses a real Linux kernel, check for it
    if let Ok(version) = std::fs::read_to_string("/proc/version") {
        // WSL2 kernel versions typically contain "WSL2"
        if version.contains("WSL2") {
            return true;
        }
    }

    // Another check: WSL2 has /run/WSL
    if std::path::Path::new("/run/WSL").exists() {
        return true;
    }

    // Check for interop, which works differently in WSL1 vs WSL2
    // In WSL2, we can access Windows filesystem more seamlessly
    if let Ok(mounts) = std::fs::read_to_string("/proc/mounts") {
        // WSL2 uses 9p for mounting Windows drives
        if mounts.contains("9p") {
            return true;
        }
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_detection() {
        let platform = Platform::detect();
        // Just verify it returns something valid
        assert!(!platform.name().is_empty());
    }

    #[test]
    fn test_unix_socket_support() {
        let platform = Platform::detect();
        // On macOS and Linux (non-WSL1), we should have socket support
        #[cfg(target_os = "macos")]
        assert!(platform.supports_unix_sockets());
    }
}
