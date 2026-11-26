// SPDX-License-Identifier: MPL-2.0

use iced::theme::Palette;
use iced::widget::{button, container};
use iced::{Border, Color, Shadow, Theme, Vector, color};

use crate::models::ErrorSeverity;

/// Soft cream background used across the main application canvas.
pub const CREAM_BG: Color = color!(0xFB, 0xF7, 0xF0);

/// Warm parchment tone for the left navigation sidebar.
pub const SIDEBAR_BG: Color = color!(0xF4, 0xED, 0xE2);

/// Clean white surface for primary cards and interactive panels.
pub const CARD_SURFACE: Color = color!(0xFF, 0xFF, 0xFF);

/// Gentle tinted surface for secondary or nested callout cards.
pub const CARD_MUTED: Color = color!(0xF7, 0xF1, 0xE7);

/// Soft warm border stroke separating cards and navigation elements.
pub const BORDER_WARM: Color = color!(0xE6, 0xDA, 0xC8);

/// Primary warm-slate text color for high contrast without harshness.
pub const WARM_SLATE: Color = color!(0x2E, 0x29, 0x25);

/// Secondary muted-slate text color for subtitles and captions.
pub const MUTED_SLATE: Color = color!(0x6E, 0x65, 0x5E);

/// Playful coral accent used for primary actions and active highlights.
pub const CORAL: Color = color!(0xE8, 0x6A, 0x58);

/// Slightly deeper coral for hovered primary buttons.
pub const CORAL_HOVER: Color = color!(0xD6, 0x58, 0x46);

/// Soft blush tint for active badges and selected pill backgrounds.
pub const CORAL_SOFT: Color = color!(0xFC, 0xEB, 0xE8);

/// Warm amber accent used for GIGO audio cautions and warning banners.
pub const AMBER: Color = color!(0xD9, 0x8F, 0x2B);

/// Soft amber background fill for warning banners and caution toasts.
pub const AMBER_SOFT: Color = color!(0xFE, 0xF4, 0xDF);

/// Encouraging sage green used for streaks, goals, and clear scores.
pub const SAGE: Color = color!(0x52, 0x99, 0x6E);

/// Soft sage background tint for success badges and progress pills.
pub const SAGE_SOFT: Color = color!(0xEA, 0xF5, 0xEE);

/// Warm crimson used for recoverable error states and alerts.
pub const DANGER: Color = color!(0xC9, 0x4A, 0x4A);

/// Soft rose background fill for dismissible error toast cards.
pub const DANGER_SOFT: Color = color!(0xFD, 0xEC, 0xEC);

/// Builds Kotodama's custom warm and playful Iced [`Theme`].
pub fn kotodama_theme() -> Theme {
    Theme::custom(
        "Kotodama Warm".to_string(),
        Palette {
            background: CREAM_BG,
            text: WARM_SLATE,
            primary: CORAL,
            success: SAGE,
            warning: AMBER,
            danger: DANGER,
        },
    )
}

/// Container style for the root application background.
pub fn app_shell_style(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(WARM_SLATE),
        background: Some(CREAM_BG.into()),
        ..container::Style::default()
    }
}

/// Container style for the left navigation sidebar.
pub fn sidebar_style(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(WARM_SLATE),
        background: Some(SIDEBAR_BG.into()),
        border: Border {
            color: BORDER_WARM,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

/// Rounded surface style for primary content cards.
pub fn card_style(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(WARM_SLATE),
        background: Some(CARD_SURFACE.into()),
        border: Border {
            color: BORDER_WARM,
            width: 1.0,
            radius: 16.0.into(),
        },
        shadow: Shadow {
            color: Color::from_rgba8(46, 41, 37, 0.06),
            offset: Vector::new(0.0, 4.0),
            blur_radius: 14.0,
        },
        ..container::Style::default()
    }
}

/// Rounded muted surface style for nested helper cards and shortcut blocks.
pub fn muted_card_style(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(WARM_SLATE),
        background: Some(CARD_MUTED.into()),
        border: Border {
            color: BORDER_WARM,
            width: 1.0,
            radius: 12.0.into(),
        },
        ..container::Style::default()
    }
}

/// Highlighted amber callout style for the Garbage-In, Garbage-Out deck warning.
pub fn gigo_banner_style(_theme: &Theme) -> container::Style {
    container::Style {
        text_color: Some(WARM_SLATE),
        background: Some(AMBER_SOFT.into()),
        border: Border {
            color: AMBER,
            width: 1.5,
            radius: 14.0.into(),
        },
        ..container::Style::default()
    }
}

/// Returns a container style closure for dismissible error and warning toasts.
pub fn toast_card_style(severity: ErrorSeverity) -> impl Fn(&Theme) -> container::Style {
    move |_theme| {
        let (bg, border) = match severity {
            ErrorSeverity::Warning => (AMBER_SOFT, AMBER),
            ErrorSeverity::Error => (DANGER_SOFT, DANGER),
        };
        container::Style {
            text_color: Some(WARM_SLATE),
            background: Some(bg.into()),
            border: Border {
                color: border,
                width: 1.5,
                radius: 14.0.into(),
            },
            shadow: Shadow {
                color: Color::from_rgba8(46, 41, 37, 0.14),
                offset: Vector::new(0.0, 6.0),
                blur_radius: 18.0,
            },
            ..container::Style::default()
        }
    }
}

/// Returns a button style closure for sidebar navigation entries.
pub fn nav_button_style(is_active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let background = match (is_active, status) {
            (true, _) => Some(CORAL_SOFT.into()),
            (false, button::Status::Hovered) => Some(CARD_SURFACE.into()),
            (false, button::Status::Pressed) => Some(CARD_MUTED.into()),
            _ => None,
        };
        let border_color = if is_active {
            CORAL
        } else if matches!(status, button::Status::Hovered) {
            BORDER_WARM
        } else {
            Color::TRANSPARENT
        };
        button::Style {
            background,
            text_color: if is_active { CORAL_HOVER } else { WARM_SLATE },
            border: Border {
                color: border_color,
                width: if is_active { 1.5 } else { 1.0 },
                radius: 12.0.into(),
            },
            ..button::Style::default()
        }
    }
}

/// Primary coral button style for main call-to-action controls.
pub fn primary_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Active => CORAL,
        button::Status::Hovered => CORAL_HOVER,
        button::Status::Pressed => CORAL_HOVER,
        button::Status::Disabled => BORDER_WARM,
    };
    let text_color = match status {
        button::Status::Disabled => MUTED_SLATE,
        _ => Color::WHITE,
    };
    button::Style {
        background: Some(background.into()),
        text_color,
        border: Border {
            color: Color::TRANSPARENT,
            width: 0.0,
            radius: 12.0.into(),
        },
        ..button::Style::default()
    }
}

/// Secondary warm-bordered button style for dismiss and auxiliary actions.
pub fn secondary_button_style(_theme: &Theme, status: button::Status) -> button::Style {
    let background = match status {
        button::Status::Hovered => CARD_MUTED,
        button::Status::Pressed => SIDEBAR_BG,
        _ => CARD_SURFACE,
    };
    button::Style {
        background: Some(background.into()),
        text_color: WARM_SLATE,
        border: Border {
            color: BORDER_WARM,
            width: 1.0,
            radius: 10.0.into(),
        },
        ..button::Style::default()
    }
}

/// Returns a rounded pill button style closure for mode and option selectors.
pub fn pill_button_style(is_selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |_theme, status| {
        let background = match (is_selected, status) {
            (true, _) => CORAL_SOFT,
            (false, button::Status::Hovered) => CARD_MUTED,
            _ => CARD_SURFACE,
        };
        let border_color = if is_selected { CORAL } else { BORDER_WARM };
        button::Style {
            background: Some(background.into()),
            text_color: if is_selected { CORAL_HOVER } else { WARM_SLATE },
            border: Border {
                color: border_color,
                width: if is_selected { 1.5 } else { 1.0 },
                radius: 999.0.into(),
            },
            ..button::Style::default()
        }
    }
}

/// Returns a rounded pill badge container style closure using `bg` and `border`.
pub fn badge_style(bg: Color, border: Color) -> impl Fn(&Theme) -> container::Style {
    move |_theme| container::Style {
        text_color: Some(WARM_SLATE),
        background: Some(bg.into()),
        border: Border {
            color: border,
            width: 1.0,
            radius: 999.0.into(),
        },
        ..container::Style::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kotodama_theme_uses_warm_palette_colors() {
        let theme = kotodama_theme();
        let palette = theme.palette();
        assert_eq!(palette.background, CREAM_BG);
        assert_eq!(palette.text, WARM_SLATE);
        assert_eq!(palette.primary, CORAL);
        assert_eq!(palette.success, SAGE);
        assert_eq!(palette.warning, AMBER);
        assert_eq!(palette.danger, DANGER);
    }

    #[test]
    fn container_and_button_styles_apply_expected_accents() {
        let theme = kotodama_theme();
        let gigo = gigo_banner_style(&theme);
        assert_eq!(gigo.border.color, AMBER);

        let err_toast = toast_card_style(ErrorSeverity::Error)(&theme);
        assert_eq!(err_toast.border.color, DANGER);

        let warn_toast = toast_card_style(ErrorSeverity::Warning)(&theme);
        assert_eq!(warn_toast.border.color, AMBER);

        let active_nav = nav_button_style(true)(&theme, button::Status::Active);
        assert_eq!(active_nav.border.color, CORAL);
        assert_eq!(active_nav.text_color, CORAL_HOVER);

        let disabled_primary = primary_button_style(&theme, button::Status::Disabled);
        assert_eq!(disabled_primary.text_color, MUTED_SLATE);

        let selected_pill = pill_button_style(true)(&theme, button::Status::Active);
        assert_eq!(selected_pill.border.color, CORAL);
    }
}
