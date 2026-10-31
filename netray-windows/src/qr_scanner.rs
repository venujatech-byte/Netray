//! Pure Rust QR code scanning and decoding engine for Netray Windows.
//!
//! Provides image parsing, grayscale transformation, grid detection,
//! and payload decoding from byte buffers, image files, and raw buffers.

use rqrr::PreparedImage;
use std::path::Path;

/// Decodes QR code content from raw image file bytes (PNG, JPEG, etc.).
pub fn decode_qr_from_image_bytes(bytes: &[u8]) -> Result<String, String> {
    if bytes.is_empty() {
        return Err("Cannot decode QR code from empty image payload".to_string());
    }

    let img = image::load_from_memory(bytes)
        .map_err(|e| format!("Failed to decode image payload: {e}"))?;
    let luma = img.to_luma8();
    let mut prepared = PreparedImage::prepare(luma);
    let grids = prepared.detect_grids();

    if grids.is_empty() {
        return Err("No QR code detected in the captured image".to_string());
    }

    for grid in grids {
        let mut content_bytes = Vec::new();
        if grid.decode_to(&mut content_bytes).is_ok() {
            if let Ok(text) = String::from_utf8(content_bytes) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    return Ok(trimmed.to_string());
                }
            }
        }
    }

    Err("QR code pattern detected but payload could not be decoded".to_string())
}

/// Decodes QR code content directly from an image file on disk.
pub fn decode_qr_from_file<P: AsRef<Path>>(path: P) -> Result<String, String> {
    let path_ref = path.as_ref();
    let bytes = std::fs::read(path_ref)
        .map_err(|e| format!("Failed to read image file '{}': {e}", path_ref.display()))?;
    decode_qr_from_image_bytes(&bytes)
}

/// Decodes QR code content from raw grayscale (luma8) pixel buffer.
pub fn decode_qr_from_luma(width: usize, height: usize, luma_data: Vec<u8>) -> Result<String, String> {
    let img = image::GrayImage::from_raw(width as u32, height as u32, luma_data)
        .ok_or_else(|| "Invalid raw grayscale image buffer dimensions".to_string())?;
    let mut prepared = PreparedImage::prepare(img);
    let grids = prepared.detect_grids();

    if grids.is_empty() {
        return Err("No QR code detected in grayscale buffer".to_string());
    }

    for grid in grids {
        let mut content_bytes = Vec::new();
        if grid.decode_to(&mut content_bytes).is_ok() {
            if let Ok(text) = String::from_utf8(content_bytes) {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    return Ok(trimmed.to_string());
                }
            }
        }
    }

    Err("QR code pattern detected but payload could not be decoded".to_string())
}

/// Captures a full desktop screenshot and returns the raw PNG/JPEG image bytes.
pub fn capture_desktop_screenshot() -> Result<Vec<u8>, String> {
    let temp_dir = std::env::temp_dir();
    let file_name = format!(
        "netray_qr_screen_{}_{}.png",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    );
    let temp_path = temp_dir.join(file_name);
    let path_str = temp_path.to_string_lossy().to_string();

    #[cfg(target_os = "linux")]
    {
        // Try known Linux screenshot capture utilities in priority order
        let commands = [
            ("import", vec!["-window", "root", path_str.as_str()]),
            ("grim", vec![path_str.as_str()]),
            ("gnome-screenshot", vec!["-f", path_str.as_str()]),
            ("spectacle", vec!["-b", "-n", "-o", path_str.as_str()]),
            ("maim", vec![path_str.as_str()]),
            ("scrot", vec![path_str.as_str()]),
        ];

        let mut success = false;
        let mut last_error = String::new();

        for (cmd, args) in &commands {
            match std::process::Command::new(cmd).args(args).output() {
                Ok(output) if output.status.success() && temp_path.exists() => {
                    success = true;
                    break;
                }
                Ok(output) => {
                    last_error = format!("{} exited with {}: {}", cmd, output.status, String::from_utf8_lossy(&output.stderr));
                }
                Err(e) => {
                    last_error = format!("Failed to spawn {}: {}", cmd, e);
                }
            }
        }

        // If CLI tools failed, try D-Bus GNOME screenshot portal if available
        if !success {
            let dbus_args = vec![
                "call", "--session",
                "--dest", "org.gnome.Shell.Screenshot",
                "--object-path", "/org/gnome/Shell/Screenshot",
                "--method", "org.gnome.Shell.Screenshot.Screenshot",
                "true", "false", path_str.as_str(),
            ];
            if let Ok(output) = std::process::Command::new("gdbus").args(&dbus_args).output() {
                if output.status.success() && temp_path.exists() {
                    success = true;
                }
            }
        }

        if !success || !temp_path.exists() {
            let _ = std::fs::remove_file(&temp_path);
            return Err(format!("Unable to capture desktop screenshot. Please ensure 'import' (ImageMagick), 'grim', or 'gnome-screenshot' is installed. ({last_error})"));
        }
    }

    #[cfg(target_os = "windows")]
    {
        let ps_script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; Add-Type -AssemblyName System.Drawing; \
             $bounds = [System.Windows.Forms.Screen]::PrimaryScreen.Bounds; \
             $bitmap = New-Object System.Drawing.Bitmap $bounds.Width, $bounds.Height; \
             $graphics = [System.Drawing.Graphics]::FromImage($bitmap); \
             $graphics.CopyFromScreen($bounds.Location, [System.Drawing.Point]::Empty, $bounds.Size); \
             $bitmap.Save('{}', [System.Drawing.Imaging.ImageFormat]::Png); \
             $graphics.Dispose(); \
             $bitmap.Dispose();",
            path_str.replace('\\', "\\\\")
        );

        let output = std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &ps_script])
            .output()
            .map_err(|e| format!("Failed to execute PowerShell screenshot command: {e}"))?;

        if !output.status.success() || !temp_path.exists() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let _ = std::fs::remove_file(&temp_path);
            return Err(format!("Windows screen capture failed: {stderr}"));
        }
    }

    let bytes = std::fs::read(&temp_path)
        .map_err(|e| format!("Failed to read captured screenshot: {e}"))?;
    let _ = std::fs::remove_file(&temp_path);

    Ok(bytes)
}

/// Convenience function to capture desktop screenshot and immediately decode any QR code.
pub fn scan_qr_from_desktop() -> Result<String, String> {
    let bytes = capture_desktop_screenshot()?;
    decode_qr_from_image_bytes(&bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use qrcode::QrCode;

    fn create_test_qr_luma(content: &str, scale: usize) -> (usize, usize, Vec<u8>) {
        let code = QrCode::new(content.as_bytes()).expect("Valid QR");
        let width = code.width();
        let border = 4;
        let total_modules = width + border * 2;
        let img_dim = total_modules * scale;
        let mut pixels = vec![255u8; img_dim * img_dim];

        for y in 0..width {
            for x in 0..width {
                if code[(x, y)] == qrcode::Color::Dark {
                    let start_x = (x + border) * scale;
                    let start_y = (y + border) * scale;
                    for dy in 0..scale {
                        for dx in 0..scale {
                            let px = start_x + dx;
                            let py = start_y + dy;
                            pixels[py * img_dim + px] = 0u8;
                        }
                    }
                }
            }
        }
        (img_dim, img_dim, pixels)
    }

    #[test]
    fn test_decode_qr_empty_buffer_error() {
        let res = decode_qr_from_image_bytes(&[]);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("empty image payload"));
    }

    #[test]
    fn test_decode_qr_invalid_image_error() {
        let res = decode_qr_from_image_bytes(&[1, 2, 3, 4, 5]);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("Failed to decode image payload"));
    }

    #[test]
    fn test_decode_qr_from_luma_roundtrip() {
        let expected = "vless://user-uuid@example.com:443?security=tls#Production-Node";
        let (w, h, luma) = create_test_qr_luma(expected, 6);
        let decoded = decode_qr_from_luma(w, h, luma);
        assert!(decoded.is_ok(), "Decode error: {:?}", decoded);
        assert_eq!(decoded.unwrap(), expected);
    }

    #[test]
    fn test_decode_qr_from_image_bytes_png() {
        let expected = "vmess://eyJhZGQiOiIxMjcuMC4wLjEiLCJwb3J0Ijo0NDN9";
        let (w, h, luma) = create_test_qr_luma(expected, 8);
        let img = image::GrayImage::from_raw(w as u32, h as u32, luma).unwrap();
        
        let mut png_bytes = Vec::new();
        let dyn_img = image::DynamicImage::ImageLuma8(img);
        dyn_img
            .write_to(&mut std::io::Cursor::new(&mut png_bytes), image::ImageFormat::Png)
            .expect("Encode PNG");

        let decoded = decode_qr_from_image_bytes(&png_bytes);
        assert!(decoded.is_ok(), "Decode PNG error: {:?}", decoded);
        assert_eq!(decoded.unwrap(), expected);
    }
}
