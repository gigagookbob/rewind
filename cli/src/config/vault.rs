//! Obsidian vault discovery and platform-specific path handling.

use std::path::PathBuf;

/// Reads vault path from Obsidian's app config (obsidian.json).
/// macOS:   ~/Library/Application Support/obsidian/obsidian.json
/// Linux:   ~/.config/obsidian/obsidian.json
fn detect_vault_from_obsidian_json() -> Option<PathBuf> {
    let mut json_paths: Vec<PathBuf> = Vec::new();
    let mut push_candidate = |path: PathBuf| {
        if !json_paths.iter().any(|p| p == &path) {
            json_paths.push(path);
        }
    };

    if cfg!(target_os = "macos") {
        push_candidate(
            dirs::home_dir()?
                .join("Library")
                .join("Application Support")
                .join("obsidian")
                .join("obsidian.json"),
        );
    } else {
        push_candidate(
            dirs::home_dir()?
                .join(".config")
                .join("obsidian")
                .join("obsidian.json"),
        );
    }

    // WSL fallback: Obsidian for Windows stores config under %APPDATA%.
    if cfg!(target_os = "linux") && is_wsl_environment() {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            push_candidate(
                PathBuf::from(appdata)
                    .join("obsidian")
                    .join("obsidian.json"),
            );
        }
        if let Some(userprofile) = std::env::var_os("USERPROFILE") {
            push_candidate(
                PathBuf::from(userprofile)
                    .join("AppData")
                    .join("Roaming")
                    .join("obsidian")
                    .join("obsidian.json"),
            );
        }
        if let Ok(entries) = std::fs::read_dir("/mnt/c/Users") {
            for entry in entries.flatten() {
                push_candidate(
                    entry
                        .path()
                        .join("AppData")
                        .join("Roaming")
                        .join("obsidian")
                        .join("obsidian.json"),
                );
            }
        }
    }

    for json_path in json_paths {
        let content = match std::fs::read_to_string(&json_path) {
            Ok(content) => content,
            Err(_) => continue,
        };
        let json: serde_json::Value = match serde_json::from_str(&content) {
            Ok(json) => json,
            Err(_) => continue,
        };

        let Some(vaults) = json.get("vaults").and_then(|v| v.as_object()) else {
            continue;
        };

        for (_id, vault_info) in vaults {
            if let Some(path_str) = vault_info.get("path").and_then(|v| v.as_str()) {
                let path = normalize_obsidian_vault_path(path_str);
                if path.exists() {
                    return Some(path);
                }
            }
        }
    }
    None
}

fn normalize_obsidian_vault_path(path_str: &str) -> PathBuf {
    let path = PathBuf::from(path_str);
    if path.exists() {
        return path;
    }

    if cfg!(target_os = "linux")
        && is_wsl_environment()
        && let Some(wsl_path) = windows_path_to_wsl(path_str)
    {
        return wsl_path;
    }

    path
}

fn windows_path_to_wsl(path: &str) -> Option<PathBuf> {
    let mut chars = path.chars();
    let drive = chars.next()?;
    if !drive.is_ascii_alphabetic() || chars.next()? != ':' {
        return None;
    }
    let sep = chars.next()?;
    if sep != '\\' && sep != '/' {
        return None;
    }

    let rest = chars.as_str().replace('\\', "/");
    Some(PathBuf::from(format!(
        "/mnt/{}/{}",
        drive.to_ascii_lowercase(),
        rest
    )))
}

fn is_wsl_environment() -> bool {
    if std::env::var_os("WSL_DISTRO_NAME").is_some() {
        return true;
    }

    std::fs::read_to_string("/proc/version")
        .map(|s| s.to_ascii_lowercase().contains("microsoft"))
        .unwrap_or(false)
}

/// Finds a directory containing `.obsidian` (Obsidian vault marker) under the given path.
pub fn detect_vault_in_dir(search_dir: &std::path::Path) -> Option<PathBuf> {
    let entries = std::fs::read_dir(search_dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && path.join(".obsidian").is_dir() {
            return Some(path);
        }
    }
    None
}

/// Auto-detects the Obsidian vault path.
/// Priority: 1) obsidian.json, 2) .obsidian marker under ~/Documents/Obsidian/.
pub(super) fn detect_obsidian_vault() -> Option<PathBuf> {
    if let Some(vault) = detect_vault_from_obsidian_json() {
        return Some(vault);
    }

    let home = dirs::home_dir()?;
    let obsidian_dir = home.join("Documents").join("Obsidian");
    detect_vault_in_dir(&obsidian_dir)
}

/// Default output path when no Obsidian vault is detected: ~/.rwd/output
pub(super) fn default_output_path() -> PathBuf {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".rwd").join("output")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_vault_returns_path_when_obsidian_dir_exists() {
        let temp_dir = std::env::temp_dir().join("rwd_test_vault_detect");
        let _ = std::fs::remove_dir_all(&temp_dir);
        let vault_dir = temp_dir.join("TestVault");
        let obsidian_marker = vault_dir.join(".obsidian");
        std::fs::create_dir_all(&obsidian_marker).expect("create dir");

        let result = detect_vault_in_dir(&temp_dir);
        assert!(result.is_some());
        assert_eq!(result.unwrap(), vault_dir);

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_detect_vault_returns_none_when_missing() {
        let temp_dir = std::env::temp_dir().join("rwd_test_no_vault");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(&temp_dir).expect("create dir");

        let result = detect_vault_in_dir(&temp_dir);
        assert!(result.is_none());

        std::fs::remove_dir_all(&temp_dir).ok();
    }

    #[test]
    fn test_windows_path_to_wsl_converts_drive_path() {
        let converted = windows_path_to_wsl(r"C:\Users\alice\Documents\Vault")
            .expect("should convert Windows path");
        assert_eq!(
            converted,
            PathBuf::from("/mnt/c/Users/alice/Documents/Vault")
        );
    }

    #[test]
    fn test_windows_path_to_wsl_rejects_non_windows_path() {
        assert!(windows_path_to_wsl("/home/alice/vault").is_none());
    }
}
