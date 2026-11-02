use std::sync::LazyLock;
use tray_icon::Icon;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Disconnected,
    Connected,
    Error,
}

impl Status {
    pub fn for_connection(running: bool, failed: bool, stopped: bool) -> Self {
        if stopped {
            Self::Disconnected
        } else if failed {
            Self::Error
        } else if running {
            Self::Connected
        } else {
            Self::Disconnected
        }
    }

    pub fn icon(self) -> Result<Icon, String> {
        // Keep only tray-sized pixels; decode the supplied artwork once per state.
        static IDLE: LazyLock<Result<image::RgbaImage, String>> = LazyLock::new(|| decode(include_bytes!("../packaging/netray-tray.png")));
        static BLUE: LazyLock<Result<image::RgbaImage, String>> = LazyLock::new(|| decode(include_bytes!("../packaging/netray-tray-connected.png")));
        static RED: LazyLock<Result<image::RgbaImage, String>> = LazyLock::new(|| decode(include_bytes!("../packaging/netray-tray-error.png")));
        let pixels = match self {
            Self::Disconnected => &*IDLE,
            Self::Connected => &*BLUE,
            Self::Error => &*RED,
        }.as_ref().map_err(Clone::clone)?;
        Icon::from_rgba(pixels.as_raw().clone(), pixels.width(), pixels.height())
            .map_err(|e| e.to_string())
    }
}

fn decode(bytes: &[u8]) -> Result<image::RgbaImage, String> {
    Ok(image::load_from_memory(bytes).map_err(|e| e.to_string())?
        .thumbnail(64, 64).into_rgba8())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_priority_and_assets() {
        assert_eq!(Status::for_connection(true, false, false), Status::Connected);
        assert_eq!(Status::for_connection(true, true, false), Status::Error);
        assert_eq!(Status::for_connection(false, true, false), Status::Error);
        assert_eq!(Status::for_connection(true, true, true), Status::Disconnected);
        assert_eq!(Status::for_connection(false, false, false), Status::Disconnected);
        for status in [Status::Disconnected, Status::Connected, Status::Error] {
            assert!(status.icon().is_ok());
        }
    }
}
