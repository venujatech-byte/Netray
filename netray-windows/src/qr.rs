use qrcode::render::svg;
use qrcode::QrCode;

/// Generates a scalable vector SVG QR code from any string content
pub fn generate_qr_svg(content: &str) -> Result<String, String> {
    let code = QrCode::new(content.as_bytes())
        .map_err(|e| format!("QR Code generation error: {e}"))?;

    let image = code
        .render::<svg::Color>()
        .min_dimensions(240, 240)
        .dark_color(svg::Color("#0f1117"))
        .light_color(svg::Color("#ffffff"))
        .build();

    Ok(image)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_windows_generate_qr_svg_produces_valid_svg() {
        let res = generate_qr_svg("vless://windows-test@127.0.0.1:443#WinSample");
        assert!(res.is_ok());
        let svg = res.unwrap();
        assert!(svg.contains("<svg"));
        assert!(svg.contains("</svg>"));
        assert!(svg.contains("#ffffff"));
    }
}
