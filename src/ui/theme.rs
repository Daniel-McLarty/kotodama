// SPDX-License-Identifier: MPL-2.0

use iced::overlay::menu;
use iced::theme::Palette;
use iced::widget::{button, container, pick_list, text_input};
use iced::{Border, Color, Shadow, Theme, Vector, color};

use crate::models::{ErrorSeverity, ResolvedTheme};

/// Semantic color tokens shared across Kotodama's Light and Dark appearances.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ThemeTokens {
    /// Main scrollable canvas background color.
    pub canvas_bg: Color,
    /// Left navigation sidebar background color.
    pub sidebar_bg: Color,
    /// Top navigation bar background color.
    pub top_bar_bg: Color,
    /// Primary elevated card surface color.
    pub card_surface: Color,
    /// Muted nested card surface color.
    pub card_muted: Color,
    /// Border stroke color for cards, navigation dividers, and pills.
    pub border: Color,
    /// High-contrast primary body and heading text color.
    pub text_primary: Color,
    /// Secondary muted text color for captions and helper copy.
    pub text_muted: Color,
    /// Primary coral brand accent color.
    pub coral: Color,
    /// Coral accent color used for button hover/pressed states.
    pub coral_hover: Color,
    /// Soft tinted coral fill for selected navigation and pill controls.
    pub coral_soft: Color,
    /// High-contrast coral text color on soft or surface backgrounds.
    pub coral_text: Color,
    /// Warm amber accent used for cautions and GIGO warnings.
    pub amber: Color,
    /// Soft amber background tint for warning banners and caution toasts.
    pub amber_soft: Color,
    /// Encouraging sage green accent for streaks, goals, and clear scores.
    pub sage: Color,
    /// Soft sage background tint for success badges.
    pub sage_soft: Color,
    /// Warm crimson accent for recoverable errors.
    pub danger: Color,
    /// Soft crimson background tint for error toast cards.
    pub danger_soft: Color,
    /// Text color rendered inside solid coral primary buttons.
    pub primary_button_text: Color,
    /// Drop shadow color applied to elevated cards and toasts.
    pub shadow_color: Color,
}

/// Warm cream and soft slate color tokens for Light mode.
pub const LIGHT_TOKENS: ThemeTokens = ThemeTokens {
    canvas_bg: color!(0xFB, 0xF7, 0xF0),
    sidebar_bg: color!(0xF4, 0xED, 0xE2),
    top_bar_bg: color!(0xF8, 0xF2, 0xE8),
    card_surface: color!(0xFF, 0xFD, 0xF9),
    card_muted: color!(0xF7, 0xF1, 0xE7),
    border: color!(0xE6, 0xDA, 0xC8),
    text_primary: color!(0x2E, 0x29, 0x25),
    text_muted: color!(0x6E, 0x65, 0x5E),
    coral: color!(0xE8, 0x6A, 0x58),
    coral_hover: color!(0xD6, 0x58, 0x46),
    coral_soft: color!(0xFC, 0xEB, 0xE8),
    coral_text: color!(0xD6, 0x58, 0x46),
    amber: color!(0xD9, 0x8F, 0x2B),
    amber_soft: color!(0xFE, 0xF4, 0xDF),
    sage: color!(0x52, 0x99, 0x6E),
    sage_soft: color!(0xEA, 0xF5, 0xEE),
    danger: color!(0xC9, 0x4A, 0x4A),
    danger_soft: color!(0xFD, 0xEC, 0xEC),
    primary_button_text: color!(0xFF, 0xFD, 0xF9),
    shadow_color: Color::from_rgba(46.0 / 255.0, 41.0 / 255.0, 37.0 / 255.0, 0.06),
};

/// Warm charcoal and espresso color tokens for Dark mode.
pub const DARK_TOKENS: ThemeTokens = ThemeTokens {
    canvas_bg: color!(0x1D, 0x19, 0x17),
    sidebar_bg: color!(0x24, 0x1F, 0x1C),
    top_bar_bg: color!(0x26, 0x21, 0x1E),
    card_surface: color!(0x2C, 0x26, 0x22),
    card_muted: color!(0x35, 0x2E, 0x29),
    border: color!(0x46, 0x3D, 0x36),
    text_primary: color!(0xF3, 0xEC, 0xE3),
    text_muted: color!(0xB5, 0xA9, 0x9C),
    coral: color!(0xE6, 0x6E, 0x5C),
    coral_hover: color!(0xF0, 0x82, 0x71),
    coral_soft: color!(0x3F, 0x29, 0x25),
    coral_text: color!(0xF7, 0xA1, 0x93),
    amber: color!(0xED, 0xAD, 0x4F),
    amber_soft: color!(0x3A, 0x2F, 0x1F),
    sage: color!(0x6F, 0xC1, 0x8D),
    sage_soft: color!(0x21, 0x34, 0x28),
    danger: color!(0xE3, 0x6B, 0x6B),
    danger_soft: color!(0x3C, 0x23, 0x23),
    primary_button_text: color!(0x1D, 0x19, 0x17),
    shadow_color: Color::from_rgba(12.0 / 255.0, 10.0 / 255.0, 9.0 / 255.0, 0.34),
};

/// Returns the semantic [`ThemeTokens`] for a [`ResolvedTheme`].
pub const fn tokens(mode: ResolvedTheme) -> &'static ThemeTokens {
    match mode {
        ResolvedTheme::Light => &LIGHT_TOKENS,
        ResolvedTheme::Dark => &DARK_TOKENS,
    }
}

/// Resolves the matching [`ThemeTokens`] from an active Iced [`Theme`].
pub fn tokens_for_theme(theme: &Theme) -> &'static ThemeTokens {
    if theme.palette().background == DARK_TOKENS.canvas_bg {
        &DARK_TOKENS
    } else {
        &LIGHT_TOKENS
    }
}

/// Builds Kotodama's custom warm Iced [`Theme`] for the given [`ResolvedTheme`].
pub fn kotodama_theme(mode: ResolvedTheme) -> Theme {
    let colors = tokens(mode);
    let name = match mode {
        ResolvedTheme::Light => "Kotodama Warm Light",
        ResolvedTheme::Dark => "Kotodama Warm Dark",
    };

    Theme::custom(
        name.to_string(),
        Palette {
            background: colors.canvas_bg,
            text: colors.text_primary,
            primary: colors.coral,
            success: colors.sage,
            warning: colors.amber,
            danger: colors.danger,
        },
    )
}

/// Container style for the root application background.
pub fn app_shell_style(theme: &Theme) -> container::Style {
    let colors = tokens_for_theme(theme);
    container::Style {
        text_color: Some(colors.text_primary),
        background: Some(colors.canvas_bg.into()),
        ..container::Style::default()
    }
}

/// Container style for the left navigation sidebar.
pub fn sidebar_style(theme: &Theme) -> container::Style {
    let colors = tokens_for_theme(theme);
    container::Style {
        text_color: Some(colors.text_primary),
        background: Some(colors.sidebar_bg.into()),
        border: Border {
            color: colors.border,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

/// Container style for the top navigation header bar.
pub fn top_bar_style(theme: &Theme) -> container::Style {
    let colors = tokens_for_theme(theme);
    container::Style {
        text_color: Some(colors.text_primary),
        background: Some(colors.top_bar_bg.into()),
        border: Border {
            color: colors.border,
            width: 1.0,
            radius: 0.0.into(),
        },
        ..container::Style::default()
    }
}

/// Rounded surface style for primary content cards.
pub fn card_style(theme: &Theme) -> container::Style {
    let colors = tokens_for_theme(theme);
    container::Style {
        text_color: Some(colors.text_primary),
        background: Some(colors.card_surface.into()),
        border: Border {
            color: colors.border,
            width: 1.0,
            radius: 16.0.into(),
        },
        shadow: Shadow {
            color: colors.shadow_color,
            offset: Vector::new(0.0, 4.0),
            blur_radius: 14.0,
        },
        ..container::Style::default()
    }
}

/// Rounded muted surface style for nested helper cards and shortcut blocks.
pub fn muted_card_style(theme: &Theme) -> container::Style {
    let colors = tokens_for_theme(theme);
    container::Style {
        text_color: Some(colors.text_primary),
        background: Some(colors.card_muted.into()),
        border: Border {
            color: colors.border,
            width: 1.0,
            radius: 12.0.into(),
        },
        ..container::Style::default()
    }
}

/// Highlighted amber callout style for the Garbage-In, Garbage-Out deck warning.
pub fn gigo_banner_style(theme: &Theme) -> container::Style {
    let colors = tokens_for_theme(theme);
    container::Style {
        text_color: Some(colors.text_primary),
        background: Some(colors.amber_soft.into()),
        border: Border {
            color: colors.amber,
            width: 1.5,
            radius: 14.0.into(),
        },
        ..container::Style::default()
    }
}

/// Returns a container style closure for dismissible error and warning toasts.
pub fn toast_card_style(severity: ErrorSeverity) -> impl Fn(&Theme) -> container::Style {
    move |theme| {
        let colors = tokens_for_theme(theme);
        let (bg, border) = match severity {
            ErrorSeverity::Warning => (colors.amber_soft, colors.amber),
            ErrorSeverity::Error => (colors.danger_soft, colors.danger),
        };
        container::Style {
            text_color: Some(colors.text_primary),
            background: Some(bg.into()),
            border: Border {
                color: border,
                width: 1.5,
                radius: 14.0.into(),
            },
            shadow: Shadow {
                color: colors.shadow_color,
                offset: Vector::new(0.0, 6.0),
                blur_radius: 18.0,
            },
            ..container::Style::default()
        }
    }
}

/// Returns a button style closure for sidebar navigation entries.
pub fn nav_button_style(is_active: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let colors = tokens_for_theme(theme);
        let background = match (is_active, status) {
            (true, _) => Some(colors.coral_soft.into()),
            (false, button::Status::Hovered) => Some(colors.card_surface.into()),
            (false, button::Status::Pressed) => Some(colors.card_muted.into()),
            _ => None,
        };
        let border_color = if is_active {
            colors.coral
        } else if matches!(status, button::Status::Hovered) {
            colors.border
        } else {
            Color::TRANSPARENT
        };
        button::Style {
            background,
            text_color: if is_active {
                colors.coral_text
            } else {
                colors.text_primary
            },
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
pub fn primary_button_style(theme: &Theme, status: button::Status) -> button::Style {
    let colors = tokens_for_theme(theme);
    let background = match status {
        button::Status::Active => colors.coral,
        button::Status::Hovered | button::Status::Pressed => colors.coral_hover,
        button::Status::Disabled => colors.border,
    };
    let text_color = match status {
        button::Status::Disabled => colors.text_muted,
        _ => colors.primary_button_text,
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

/// Secondary warm-bordered button style for dismiss, theme toggle, and auxiliary actions.
pub fn secondary_button_style(theme: &Theme, status: button::Status) -> button::Style {
    let colors = tokens_for_theme(theme);
    let background = match status {
        button::Status::Hovered => colors.card_muted,
        button::Status::Pressed => colors.sidebar_bg,
        _ => colors.card_surface,
    };
    button::Style {
        background: Some(background.into()),
        text_color: colors.text_primary,
        border: Border {
            color: colors.border,
            width: 1.0,
            radius: 10.0.into(),
        },
        ..button::Style::default()
    }
}

/// Returns a rounded pill button style closure for mode and option selectors.
pub fn pill_button_style(is_selected: bool) -> impl Fn(&Theme, button::Status) -> button::Style {
    move |theme, status| {
        let colors = tokens_for_theme(theme);
        let background = match (is_selected, status) {
            (true, _) => colors.coral_soft,
            (false, button::Status::Hovered) => colors.card_muted,
            _ => colors.card_surface,
        };
        let border_color = if is_selected {
            colors.coral
        } else {
            colors.border
        };
        button::Style {
            background: Some(background.into()),
            text_color: if is_selected {
                colors.coral_text
            } else {
                colors.text_primary
            },
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
    move |theme| {
        let colors = tokens_for_theme(theme);
        container::Style {
            text_color: Some(colors.text_primary),
            background: Some(bg.into()),
            border: Border {
                color: border,
                width: 1.0,
                radius: 999.0.into(),
            },
            ..container::Style::default()
        }
    }
}

/// Adaptive warm Light/Dark style for text inputs in setup and settings forms.
pub fn text_input_style(theme: &Theme, status: text_input::Status) -> text_input::Style {
    let colors = tokens_for_theme(theme);
    let (background, border_color) = match status {
        text_input::Status::Active => (colors.card_surface, colors.border),
        text_input::Status::Hovered => (colors.card_surface, colors.text_muted),
        text_input::Status::Focused { .. } => (colors.card_surface, colors.coral),
        text_input::Status::Disabled => (colors.card_muted, colors.border),
    };

    text_input::Style {
        background: background.into(),
        border: Border {
            color: border_color,
            width: if matches!(status, text_input::Status::Focused { .. }) {
                1.5
            } else {
                1.0
            },
            radius: 10.0.into(),
        },
        icon: colors.text_muted,
        placeholder: colors.text_muted,
        value: colors.text_primary,
        selection: colors.coral_soft,
    }
}

/// Adaptive warm Light/Dark style for dropdown pick-list controls.
pub fn pick_list_style(theme: &Theme, status: pick_list::Status) -> pick_list::Style {
    let colors = tokens_for_theme(theme);
    let (background, border_color, handle_color) = match status {
        pick_list::Status::Active => (colors.card_surface, colors.border, colors.text_muted),
        pick_list::Status::Hovered => (colors.card_muted, colors.text_muted, colors.text_primary),
        pick_list::Status::Opened { .. } => (colors.card_surface, colors.coral, colors.coral_text),
    };

    pick_list::Style {
        text_color: colors.text_primary,
        placeholder_color: colors.text_muted,
        handle_color,
        background: background.into(),
        border: Border {
            color: border_color,
            width: if matches!(status, pick_list::Status::Opened { .. }) {
                1.5
            } else {
                1.0
            },
            radius: 10.0.into(),
        },
    }
}

/// Adaptive warm Light/Dark style for dropdown menu overlays.
pub fn pick_list_menu_style(theme: &Theme) -> menu::Style {
    let colors = tokens_for_theme(theme);
    menu::Style {
        background: colors.card_surface.into(),
        border: Border {
            color: colors.border,
            width: 1.0,
            radius: 10.0.into(),
        },
        text_color: colors.text_primary,
        selected_text_color: colors.coral_text,
        selected_background: colors.coral_soft.into(),
        shadow: Shadow {
            color: colors.shadow_color,
            offset: Vector::new(0.0, 4.0),
            blur_radius: 12.0,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kotodama_theme_builds_distinct_warm_light_and_dark_palettes() {
        let light = kotodama_theme(ResolvedTheme::Light);
        let dark = kotodama_theme(ResolvedTheme::Dark);

        assert_eq!(light.palette().background, LIGHT_TOKENS.canvas_bg);
        assert_eq!(light.palette().text, LIGHT_TOKENS.text_primary);
        assert_eq!(dark.palette().background, DARK_TOKENS.canvas_bg);
        assert_eq!(dark.palette().text, DARK_TOKENS.text_primary);
        assert_eq!(tokens_for_theme(&light), &LIGHT_TOKENS);
        assert_eq!(tokens_for_theme(&dark), &DARK_TOKENS);
    }

    #[test]
    fn dark_tokens_avoid_harsh_pure_black_and_pure_white() {
        assert_ne!(DARK_TOKENS.canvas_bg, Color::BLACK);
        assert_ne!(DARK_TOKENS.sidebar_bg, Color::BLACK);
        assert_ne!(DARK_TOKENS.card_surface, Color::BLACK);
        assert_ne!(DARK_TOKENS.text_primary, Color::WHITE);
        assert_ne!(DARK_TOKENS.primary_button_text, Color::WHITE);
    }

    #[test]
    fn container_and_button_styles_adapt_to_light_and_dark_themes() {
        for mode in [ResolvedTheme::Light, ResolvedTheme::Dark] {
            let theme = kotodama_theme(mode);
            let colors = tokens(mode);

            assert_eq!(app_shell_style(&theme).background, Some(colors.canvas_bg.into()));
            assert_eq!(top_bar_style(&theme).background, Some(colors.top_bar_bg.into()));
            assert_eq!(gigo_banner_style(&theme).border.color, colors.amber);
            assert_eq!(
                toast_card_style(ErrorSeverity::Error)(&theme).border.color,
                colors.danger
            );
            assert_eq!(
                toast_card_style(ErrorSeverity::Warning)(&theme).border.color,
                colors.amber
            );

            let active_nav = nav_button_style(true)(&theme, button::Status::Active);
            assert_eq!(active_nav.border.color, colors.coral);
            assert_eq!(active_nav.text_color, colors.coral_text);

            let disabled_primary = primary_button_style(&theme, button::Status::Disabled);
            assert_eq!(disabled_primary.text_color, colors.text_muted);

            let selected_pill = pill_button_style(true)(&theme, button::Status::Active);
            assert_eq!(selected_pill.border.color, colors.coral);

            let focused_input = text_input_style(&theme, text_input::Status::Focused { is_hovered: false });
            assert_eq!(focused_input.border.color, colors.coral);
            assert_eq!(focused_input.value, colors.text_primary);

            let opened_pick = pick_list_style(&theme, pick_list::Status::Opened { is_hovered: false });
            assert_eq!(opened_pick.border.color, colors.coral);
            assert_eq!(opened_pick.handle_color, colors.coral_text);

            let menu_appearance = pick_list_menu_style(&theme);
            assert_eq!(menu_appearance.selected_text_color, colors.coral_text);
            assert_eq!(menu_appearance.background, colors.card_surface.into());
        }
    }
}
