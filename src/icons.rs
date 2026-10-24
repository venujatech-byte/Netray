use iced::widget::svg::{self, Handle, Svg};
use iced::{Color, Theme};

macro_rules! lucide_svg {
    ($body:expr) => {
        concat!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"#,
            $body,
            r#"</svg>"#
        ).as_bytes()
    };
}

pub const MENU: &[u8] = lucide_svg!(r#"<line x1="4" x2="20" y1="12" y2="12"/><line x1="4" x2="20" y1="6" y2="6"/><line x1="4" x2="20" y1="18" y2="18"/>"#);
pub const FILE_PLUS: &[u8] = lucide_svg!(r#"<path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/><path d="M9 15h6"/><path d="M12 12v6"/>"#);
pub const QR_CODE: &[u8] = lucide_svg!(r#"<rect width="5" height="5" x="3" y="3" rx="1"/><rect width="5" height="5" x="16" y="3" rx="1"/><rect width="5" height="5" x="3" y="16" rx="1"/><path d="M21 16h-3a2 2 0 0 0-2 2v3"/><path d="M21 21v.01"/><path d="M12 7v3a2 2 0 0 1-2 2H7"/><path d="M3 12h.01"/><path d="M12 3h.01"/><path d="M12 16v.01"/><path d="M16 12h1"/><path d="M21 12v.01"/><path d="M12 21v-1"/>"#);
pub const LINK: &[u8] = lucide_svg!(r#"<path d="M9 17H7A5 5 0 0 1 7 7h2"/><path d="M15 7h2a5 5 0 1 1 0 10h-2"/><line x1="8" x2="16" y1="12" y2="12"/>"#);
pub const MORE_VERTICAL: &[u8] = lucide_svg!(r#"<circle cx="12" cy="12" r="1"/><circle cx="12" cy="5" r="1"/><circle cx="12" cy="19" r="1"/>"#);
pub const PLUS: &[u8] = lucide_svg!(r#"<path d="M5 12h14"/><path d="M12 5v14"/>"#);
pub const SHARE: &[u8] = lucide_svg!(r#"<circle cx="18" cy="5" r="3"/><circle cx="6" cy="12" r="3"/><circle cx="18" cy="19" r="3"/><line x1="8.59" x2="15.42" y1="13.51" y2="17.49"/><line x1="15.41" x2="8.59" y1="6.51" y2="10.49"/>"#);
pub const PENCIL: &[u8] = lucide_svg!(r#"<path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z"/>"#);
pub const TRASH: &[u8] = lucide_svg!(r#"<path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/><line x1="10" x2="10" y1="11" y2="17"/><line x1="14" x2="14" y1="11" y2="17"/>"#);
pub const ACTIVITY: &[u8] = lucide_svg!(r#"<path d="M22 12h-4l-3 9L9 3l-3 9H2"/>"#);
pub const PLAY: &[u8] = lucide_svg!(r#"<polygon points="6 3 20 12 6 21 6 3" fill="currentColor"/>"#);
pub const SQUARE: &[u8] = lucide_svg!(r#"<rect width="16" height="16" x="4" y="4" rx="2" fill="currentColor"/>"#);
pub const CHECK: &[u8] = lucide_svg!(r#"<path d="M20 6 9 17l-5-5"/>"#);
pub const MINIMIZE: &[u8] = lucide_svg!(r#"<path d="M5 12h14"/>"#);
pub const MAXIMIZE: &[u8] = lucide_svg!(r#"<rect x="5" y="5" width="14" height="14" rx="1"/>"#);
pub const CLOSE: &[u8] = lucide_svg!(r#"<path d="M18 6 6 18"/><path d="m6 6 12 12"/>"#);
pub const BACK: &[u8] = lucide_svg!(r#"<path d="m12 19-7-7 7-7"/><path d="M19 12H5"/>"#);
pub const COPY: &[u8] = lucide_svg!(r#"<rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/>"#);
pub const INFO: &[u8] = lucide_svg!(r#"<circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/>"#);
pub const LOCK: &[u8] = lucide_svg!(r#"<rect width="18" height="11" x="3" y="11" rx="2" ry="2"/><path d="M7 11V7a5 5 0 0 1 10 0v4"/>"#);
pub const SUN: &[u8] = lucide_svg!(r#"<circle cx="12" cy="12" r="4"/><path d="M12 2v2"/><path d="M12 20v2"/><path d="m4.93 4.93 1.41 1.41"/><path d="m17.66 17.66 1.41 1.41"/><path d="M2 12h2"/><path d="M20 12h2"/><path d="m6.34 17.66-1.41 1.41"/><path d="m19.07 4.93-1.41 1.41"/>"#);
pub const MOON: &[u8] = lucide_svg!(r#"<path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z"/>"#);
pub const MONITOR: &[u8] = lucide_svg!(r#"<rect width="20" height="14" x="2" y="3" rx="2"/><line x1="8" x2="16" y1="21" y2="21"/><line x1="12" x2="12" y1="17" y2="21"/>"#);

use std::sync::LazyLock;

static HANDLE_MENU: LazyLock<Handle> = LazyLock::new(|| Handle::from_memory(MENU));
static HANDLE_FILE_PLUS: LazyLock<Handle> = LazyLock::new(|| Handle::from_memory(FILE_PLUS));
static HANDLE_QR_CODE: LazyLock<Handle> = LazyLock::new(|| Handle::from_memory(QR_CODE));
static HANDLE_LINK: LazyLock<Handle> = LazyLock::new(|| Handle::from_memory(LINK));
static HANDLE_MORE_VERTICAL: LazyLock<Handle> = LazyLock::new(|| Handle::from_memory(MORE_VERTICAL));
static HANDLE_PLUS: LazyLock<Handle> = LazyLock::new(|| Handle::from_memory(PLUS));
static HANDLE_SHARE: LazyLock<Handle> = LazyLock::new(|| Handle::from_memory(SHARE));
static HANDLE_PENCIL: LazyLock<Handle> = LazyLock::new(|| Handle::from_memory(PENCIL));
static HANDLE_TRASH: LazyLock<Handle> = LazyLock::new(|| Handle::from_memory(TRASH));
static HANDLE_ACTIVITY: LazyLock<Handle> = LazyLock::new(|| Handle::from_memory(ACTIVITY));
static HANDLE_PLAY: LazyLock<Handle> = LazyLock::new(|| Handle::from_memory(PLAY));
static HANDLE_SQUARE: LazyLock<Handle> = LazyLock::new(|| Handle::from_memory(SQUARE));
static HANDLE_CHECK: LazyLock<Handle> = LazyLock::new(|| Handle::from_memory(CHECK));
static HANDLE_MINIMIZE: LazyLock<Handle> = LazyLock::new(|| Handle::from_memory(MINIMIZE));
static HANDLE_MAXIMIZE: LazyLock<Handle> = LazyLock::new(|| Handle::from_memory(MAXIMIZE));
static HANDLE_CLOSE: LazyLock<Handle> = LazyLock::new(|| Handle::from_memory(CLOSE));
static HANDLE_BACK: LazyLock<Handle> = LazyLock::new(|| Handle::from_memory(BACK));
static HANDLE_COPY: LazyLock<Handle> = LazyLock::new(|| Handle::from_memory(COPY));
static HANDLE_INFO: LazyLock<Handle> = LazyLock::new(|| Handle::from_memory(INFO));
static HANDLE_LOCK: LazyLock<Handle> = LazyLock::new(|| Handle::from_memory(LOCK));
static HANDLE_SUN: LazyLock<Handle> = LazyLock::new(|| Handle::from_memory(SUN));
static HANDLE_MOON: LazyLock<Handle> = LazyLock::new(|| Handle::from_memory(MOON));
static HANDLE_MONITOR: LazyLock<Handle> = LazyLock::new(|| Handle::from_memory(MONITOR));

/// Returns a pre-cached `Handle` for known vector icons with zero heap allocation or byte hashing
pub fn handle_for(svg_bytes: &'static [u8]) -> Handle {
    if std::ptr::eq(svg_bytes, MENU) {
        HANDLE_MENU.clone()
    } else if std::ptr::eq(svg_bytes, FILE_PLUS) {
        HANDLE_FILE_PLUS.clone()
    } else if std::ptr::eq(svg_bytes, QR_CODE) {
        HANDLE_QR_CODE.clone()
    } else if std::ptr::eq(svg_bytes, LINK) {
        HANDLE_LINK.clone()
    } else if std::ptr::eq(svg_bytes, MORE_VERTICAL) {
        HANDLE_MORE_VERTICAL.clone()
    } else if std::ptr::eq(svg_bytes, PLUS) {
        HANDLE_PLUS.clone()
    } else if std::ptr::eq(svg_bytes, SHARE) {
        HANDLE_SHARE.clone()
    } else if std::ptr::eq(svg_bytes, PENCIL) {
        HANDLE_PENCIL.clone()
    } else if std::ptr::eq(svg_bytes, TRASH) {
        HANDLE_TRASH.clone()
    } else if std::ptr::eq(svg_bytes, ACTIVITY) {
        HANDLE_ACTIVITY.clone()
    } else if std::ptr::eq(svg_bytes, PLAY) {
        HANDLE_PLAY.clone()
    } else if std::ptr::eq(svg_bytes, SQUARE) {
        HANDLE_SQUARE.clone()
    } else if std::ptr::eq(svg_bytes, CHECK) {
        HANDLE_CHECK.clone()
    } else if std::ptr::eq(svg_bytes, MINIMIZE) {
        HANDLE_MINIMIZE.clone()
    } else if std::ptr::eq(svg_bytes, MAXIMIZE) {
        HANDLE_MAXIMIZE.clone()
    } else if std::ptr::eq(svg_bytes, CLOSE) {
        HANDLE_CLOSE.clone()
    } else if std::ptr::eq(svg_bytes, BACK) {
        HANDLE_BACK.clone()
    } else if std::ptr::eq(svg_bytes, COPY) {
        HANDLE_COPY.clone()
    } else if std::ptr::eq(svg_bytes, INFO) {
        HANDLE_INFO.clone()
    } else if std::ptr::eq(svg_bytes, LOCK) {
        HANDLE_LOCK.clone()
    } else if std::ptr::eq(svg_bytes, SUN) {
        HANDLE_SUN.clone()
    } else if std::ptr::eq(svg_bytes, MOON) {
        HANDLE_MOON.clone()
    } else if std::ptr::eq(svg_bytes, MONITOR) {
        HANDLE_MONITOR.clone()
    } else {
        Handle::from_memory(svg_bytes)
    }
}

/// Renders a Lucide vector icon with zero runtime string/memory allocations
pub fn lucide_icon(svg_bytes: &'static [u8], size: u16, color: Color) -> Svg<'static, Theme> {
    Svg::new(handle_for(svg_bytes))
        .width(size)
        .height(size)
        .style(move |_, _| svg::Style { color: Some(color) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_handle_id_behavior() {
        let h1 = handle_for(MENU);
        let h2 = handle_for(MENU);
        assert_eq!(h1.id(), h2.id());
    }
}


