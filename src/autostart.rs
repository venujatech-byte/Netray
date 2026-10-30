//! System Startup / Autostart Management
//!
//! Configures Netray to launch on user login via:
//! - Linux: FreeDesktop XDG autostart desktop entry at `~/.config/autostart/netray.desktop`
//! - Windows: Registry entry under HKCU\Software\Microsoft\Windows\CurrentVersion\Run

use std::fs;
use std::path::PathBuf;

pub fn set_run_at_startup(enable: bool) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        set_linux_autostart(enable)
    }
    #[cfg(target_os = "windows")]
    {
        set_windows_autostart(enable)
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        let _ = enable;
        Ok(())
    }
}

pub fn is_run_at_startup_configured() -> bool {
    #[cfg(target_os = "linux")]
    {
        get_linux_autostart_path().map(|p| p.exists()).unwrap_or(false)
    }
    #[cfg(target_os = "windows")]
    {
        is_windows_autostart_configured()
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        false
    }
}

#[cfg(target_os = "linux")]
fn get_linux_autostart_path() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(|h| {
        PathBuf::from(h).join(".config").join("autostart").join("netray.desktop")
    })
}

#[cfg(target_os = "linux")]
fn set_linux_autostart(enable: bool) -> Result<(), String> {
    let desktop_path = get_linux_autostart_path()
        .ok_or_else(|| "Could not determine user HOME directory".to_string())?;

    if enable {
        if let Some(parent) = desktop_path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("Failed to create autostart directory: {e}"))?;
        }
        let exe_path = std::env::current_exe()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "netray".to_string());

        let desktop_entry = format!(
            "[Desktop Entry]\n\
            Type=Application\n\
            Name=Netray\n\
            GenericName=Proxy & VPN Cockpit\n\
            Comment=Next-Gen Multi-Protocol Proxy & Tunnel Client\n\
            Exec=\"{}\"\n\
            Terminal=false\n\
            Categories=Network;Proxy;\n\
            X-GNOME-Autostart-enabled=true\n",
            exe_path
        );

        fs::write(&desktop_path, desktop_entry)
            .map_err(|e| format!("Failed to write autostart desktop file: {e}"))?;
    } else if desktop_path.exists() {
        let _ = fs::remove_file(&desktop_path);
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn set_windows_autostart(enable: bool) -> Result<(), String> {
    let exe_path = std::env::current_exe()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| "netray.exe".to_string());

    if enable {
        let status = std::process::Command::new("reg")
            .args([
                "add",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                "Netray",
                "/t",
                "REG_SZ",
                "/d",
                &format!("\"{}\"", exe_path),
                "/f",
            ])
            .status()
            .map_err(|e| format!("Failed to invoke reg command: {e}"))?;

        if !status.success() {
            return Err("Failed to register Netray in Windows Run registry".to_string());
        }
    } else {
        let _ = std::process::Command::new("reg")
            .args([
                "delete",
                r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
                "/v",
                "Netray",
                "/f",
            ])
            .status();
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn is_windows_autostart_configured() -> bool {
    let output = std::process::Command::new("reg")
        .args([
            "query",
            r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run",
            "/v",
            "Netray",
        ])
        .output();

    matches!(output, Ok(out) if out.status.success())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linux_autostart_path_resolution() {
        #[cfg(target_os = "linux")]
        {
            if std::env::var("HOME").is_ok() {
                let path = get_linux_autostart_path();
                assert!(path.is_some());
                assert!(path.unwrap().ends_with("netray.desktop"));
            }
        }
    }
}
