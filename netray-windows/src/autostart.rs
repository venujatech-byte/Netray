//! System Startup / Autostart Management for Netray Windows

use std::path::PathBuf;

pub fn set_run_at_startup(enable: bool) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        set_windows_autostart(enable)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = enable;
        Ok(())
    }
}

pub fn is_run_at_startup_configured() -> bool {
    #[cfg(target_os = "windows")]
    {
        is_windows_autostart_configured()
    }
    #[cfg(not(target_os = "windows"))]
    {
        false
    }
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
    fn test_autostart_functions_callable() {
        assert!(set_run_at_startup(false).is_ok());
    }
}
