use iced::widget::{column, container, pick_list, row, text, text_input, toggler};
use iced::{Alignment, Border, Color, Element, Length, Theme};

use crate::Message;

pub fn secondary(is_dark: bool) -> Color {
    if is_dark {
        Color::from_rgb8(184, 193, 207)
    } else {
        Color::from_rgb8(77, 87, 104)
    }
}

pub fn accent(is_dark: bool) -> Color {
    if is_dark {
        Color::from_rgb8(169, 200, 255)
    } else {
        Color::from_rgb8(29, 82, 162)
    }
}

pub fn surface(theme: &Theme) -> container::Style {
    container::Style {
        background: Some(
            if matches!(theme, Theme::Light) {
                Color::from_rgb8(233, 238, 246)
            } else {
                Color::from_rgb8(35, 42, 54)
            }
            .into(),
        ),
        border: Border {
            radius: 20.0.into(),
            ..Default::default()
        },
        ..Default::default()
    }
}

pub fn input(is_dark: bool, status: text_input::Status) -> text_input::Style {
    let mut style = crate::theme::text_input_style(is_dark, status);
    style.background = if is_dark {
        Color::from_rgb8(25, 31, 41)
    } else {
        Color::from_rgb8(248, 250, 254)
    }
    .into();
    style.border.radius = 12.0.into();
    style.border.width = if matches!(status, text_input::Status::Focused) {
        2.0
    } else {
        0.0
    };
    style.placeholder = secondary(is_dark);
    if matches!(status, text_input::Status::Disabled) {
        style.value = secondary(is_dark);
    }
    style
}

pub fn menu_surface(theme: &Theme) -> container::Style {
    let light = matches!(theme, Theme::Light);
    let mut style = surface(theme);
    style.background = Some(
        if light {
            Color::WHITE
        } else {
            Color::from_rgb8(55, 63, 77)
        }
        .into(),
    );
    style.shadow = iced::Shadow {
        color: Color::from_rgba(0.0, 0.0, 0.0, if light { 0.18 } else { 0.4 }),
        offset: iced::Vector::new(0.0, 6.0),
        blur_radius: 20.0,
    };
    style
}

pub fn select(is_dark: bool, status: pick_list::Status) -> pick_list::Style {
    let mut style = crate::theme::pick_list_style(is_dark, status);
    style.border.width = 0.0;
    style.border.radius = 12.0.into();
    style.placeholder_color = secondary(is_dark);
    style
}

pub fn switch_row<'a>(
    title: &'a str,
    description: &'a str,
    checked: bool,
    enabled: bool,
    on_toggle: fn(bool) -> Message,
    is_dark: bool,
) -> Element<'a, Message> {
    container(
        row![
            column![
                text(title).size(14),
                text(description).size(12).color(secondary(is_dark)),
            ]
            .spacing(4)
            .width(Length::Fill),
            toggler(checked)
                .size(22)
                .on_toggle_maybe(enabled.then_some(on_toggle))
                .style(move |theme, status| {
                    let mut style = toggler::default(theme, status);
                    if matches!(status, toggler::Status::Disabled) {
                        style.background = if checked {
                            if is_dark {
                                Color::from_rgb8(100, 126, 161)
                            } else {
                                Color::from_rgb8(94, 119, 153)
                            }
                        } else {
                            if is_dark {
                                Color::from_rgb8(82, 90, 102)
                            } else {
                                Color::from_rgb8(126, 135, 148)
                            }
                        };
                        style.foreground = Color::WHITE;
                    } else {
                        style.background = if checked {
                            accent(is_dark)
                        } else {
                            secondary(is_dark)
                        };
                        style.foreground = if checked && is_dark {
                            Color::from_rgb8(20, 46, 83)
                        } else {
                            Color::WHITE
                        };
                    }
                    style
                }),
        ]
        .spacing(16)
        .align_y(Alignment::Center),
    )
    .padding([8, 0])
    .width(Length::Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_text_has_readable_contrast_in_both_themes() {
        fn luminance(color: Color) -> f32 {
            let [r, g, b, _] = color.into_linear();
            0.2126 * r + 0.7152 * g + 0.0722 * b
        }
        for (dark, theme) in [(false, Theme::Light), (true, Theme::Dark)] {
            for style in [surface(&theme), menu_surface(&theme)] {
                let iced::Background::Color(background) = style.background.unwrap() else {
                    panic!("Settings surfaces must use a solid color");
                };
                for foreground in [secondary(dark), accent(dark)] {
                    let a = luminance(foreground);
                    let b = luminance(background);
                    assert!((a.max(b) + 0.05) / (a.min(b) + 0.05) >= 4.5);
                }
            }
        }
    }
}
