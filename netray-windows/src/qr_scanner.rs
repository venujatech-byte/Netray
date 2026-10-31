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
