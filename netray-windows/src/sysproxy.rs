use tokio::process::Command;

/// Enable Windows system-wide proxy routing via Registry (reg.exe)
pub async fn enable_system_proxy(socks_port: u16, http_port: u16) -> Result<(), String> {
    let reg_path = r#"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings"#;
    let proxy_server = format!("http=127.0.0.1:{};https=127.0.0.1:{};socks=127.0.0.1:{}", http_port, http_port, socks_port);

    // 1. Enable proxy
    let _ = Command::new("reg")
        .args(["add", reg_path, "/v", "ProxyEnable", "/t", "REG_DWORD", "/d", "1", "/f"])
        .output()
        .await;

    // 2. Set server string
    let _ = Command::new("reg")
        .args(["add", reg_path, "/v", "ProxyServer", "/t", "REG_SZ", "/d", &proxy_server, "/f"])
        .output()
        .await;

    // 3. Bypass local intranet
    let _ = Command::new("reg")
        .args(["add", reg_path, "/v", "ProxyOverride", "/t", "REG_SZ", "/d", "<local>;localhost;127.0.0.1", "/f"])
        .output()
        .await;

    // 4. Force WinINet / Internet Settings refresh via PowerShell or rundll32
    let _ = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", "
            $sig = @'
            [DllImport(\"wininet.dll\", SetLastError = true, CharSet = CharSet.Auto)]
            public static extern bool InternetSetOption(IntPtr hInternet, int dwOption, IntPtr lpBuffer, int dwBufferLength);
'@
            try {
                $type = Add-Type -MemberDefinition $sig -Name WinINet -Namespace Netray -PassThru
                $type::InternetSetOption([IntPtr]::Zero, 39, [IntPtr]::Zero, 0) | Out-Null
                $type::InternetSetOption([IntPtr]::Zero, 37, [IntPtr]::Zero, 0) | Out-Null
            } catch {}
        "])
        .output()
        .await;

    Ok(())
}

/// Disable Windows system-wide proxy and restore direct internet
pub async fn disable_system_proxy() {
    let reg_path = r#"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings"#;

    let _ = Command::new("reg")
        .args(["add", reg_path, "/v", "ProxyEnable", "/t", "REG_DWORD", "/d", "0", "/f"])
        .output()
        .await;

    let _ = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", "
            $sig = @'
            [DllImport(\"wininet.dll\", SetLastError = true, CharSet = CharSet.Auto)]
            public static extern bool InternetSetOption(IntPtr hInternet, int dwOption, IntPtr lpBuffer, int dwBufferLength);
'@
            try {
                $type = Add-Type -MemberDefinition $sig -Name WinINet -Namespace Netray -PassThru
                $type::InternetSetOption([IntPtr]::Zero, 39, [IntPtr]::Zero, 0) | Out-Null
                $type::InternetSetOption([IntPtr]::Zero, 37, [IntPtr]::Zero, 0) | Out-Null
            } catch {}
        "])
        .output()
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_proxy_format() {
        let socks_port = 10808;
        let http_port = 10809;
        let proxy_server = format!("http=127.0.0.1:{};https=127.0.0.1:{};socks=127.0.0.1:{}", http_port, http_port, socks_port);
        assert_eq!(proxy_server, "http=127.0.0.1:10809;https=127.0.0.1:10809;socks=127.0.0.1:10808");
    }
}
