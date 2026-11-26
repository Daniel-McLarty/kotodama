// SPDX-License-Identifier: MPL-2.0

use iced::widget::{button, column, container, row, text};
use iced::{Alignment, Element, Length};

use crate::models::{DeckConfig, DeckSource, FieldMapping, ResolvedTheme};
use crate::ui::theme::{self, ThemeTokens};
use crate::ui::Message;

/// Renders the Anki deck setup, field mapping overview, and GIGO warning view.
pub fn view<'a>(
    deck_config: Option<&'a DeckConfig>,
    resolved_theme: ResolvedTheme,
) -> Element<'a, Message> {
    let colors = theme::tokens(resolved_theme);

    let gigo_banner = container(
        column![
            text("Audio Quality Notice (Garbage-In, Garbage-Out)")
                .size(17)
                .color(colors.amber),
            text("Kotodama grades your pronunciation and pitch contour directly against your Anki deck's audio. Missing clips, files shorter than 200ms, or clipped recordings will be flagged automatically before practice.")
                .size(14)
                .color(colors.text_primary),
        ]
        .spacing(6),
    )
    .width(Length::Fill)
    .padding(16)
    .style(theme::gigo_banner_style);

    let connection_card = match deck_config {
        Some(config) => render_connected_deck(config, colors),
        None => render_empty_deck_state(colors),
    };

    let mapping_card = render_field_mapping_card(deck_config.map(|c| &c.field_mapping), colors);

    column![gigo_banner, connection_card, mapping_card]
        .spacing(18)
        .width(Length::Fill)
        .into()
}

fn render_empty_deck_state<'a>(colors: &'static ThemeTokens) -> Element<'a, Message> {
    container(
        column![
            text("Connect Your Anki Deck").size(19).color(colors.text_primary),
            text("Import learned cards (queue = 2) from a safe temporary copy of collection.anki2 or AnkiConnect so your reading intervals stay untouched.")
                .size(14)
                .color(colors.text_muted),
            row![
                button(text("Select collection.anki2").size(14))
                    .padding([10, 16])
                    .style(theme::primary_button_style)
                    .on_press(Message::TriggerRecoverableAction),
                button(text("Check AnkiConnect (localhost:8765)").size(14))
                    .padding([10, 16])
                    .style(theme::secondary_button_style)
                    .on_press(Message::TriggerRecoverableAction),
            ]
            .spacing(12)
            .align_y(Alignment::Center),
        ]
        .spacing(12),
    )
    .width(Length::Fill)
    .padding(20)
    .style(theme::card_style)
    .into()
}

fn render_connected_deck<'a>(
    config: &'a DeckConfig,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    let source_summary = match &config.source {
        DeckSource::LocalCollection { collection_path, .. } => {
            format!("Local collection: {}", collection_path.display())
        }
        DeckSource::AnkiConnect { endpoint_url } => {
            format!("AnkiConnect: {endpoint_url}")
        }
    };

    let audio_badge = if config.flagged_audio_count == 0 {
        container(text("Audio checks passed").size(12).color(colors.sage))
            .padding([4, 10])
            .style(theme::badge_style(colors.sage_soft, colors.sage))
    } else {
        container(
            text(format!("{} flagged clips", config.flagged_audio_count))
                .size(12)
                .color(colors.amber),
        )
        .padding([4, 10])
        .style(theme::badge_style(colors.amber_soft, colors.amber))
    };

    container(
        column![
            row![
                text(format!("Connected Deck: {}", config.deck_name))
                    .size(19)
                    .color(colors.text_primary),
                audio_badge,
            ]
            .spacing(12)
            .align_y(Alignment::Center),
            text(source_summary).size(14).color(colors.text_muted),
        ]
        .spacing(10),
    )
    .width(Length::Fill)
    .padding(20)
    .style(theme::card_style)
    .into()
}

fn render_field_mapping_card<'a>(
    mapping: Option<&'a FieldMapping>,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    let word = mapping.map_or("Expression (required)", |m| m.word_field.as_str());
    let kana = mapping.map_or("Reading (required)", |m| m.kana_field.as_str());
    let sentence = mapping
        .and_then(|m| m.sentence_field.as_deref())
        .unwrap_or("Sentence (optional)");
    let word_audio = mapping.map_or("WordAudio [sound:...] (required)", |m| {
        m.word_audio_field.as_str()
    });
    let sentence_audio = mapping
        .and_then(|m| m.sentence_audio_field.as_deref())
        .unwrap_or("SentenceAudio [sound:...] (optional)");

    container(
        column![
            text("Note Field Mapping").size(18).color(colors.text_primary),
            text("Map your Anki note fields once per deck. Kotodama persists mappings locally.")
                .size(14)
                .color(colors.text_muted),
            mapping_row("Target Word / Expression", word, colors),
            mapping_row("Kana Reading", kana, colors),
            mapping_row("Example Sentence", sentence, colors),
            mapping_row("Word Audio", word_audio, colors),
            mapping_row("Sentence Audio", sentence_audio, colors),
        ]
        .spacing(10),
    )
    .width(Length::Fill)
    .padding(20)
    .style(theme::card_style)
    .into()
}

fn mapping_row<'a>(
    label: &'a str,
    value: &'a str,
    colors: &'static ThemeTokens,
) -> Element<'a, Message> {
    container(
        row![
            text(label)
                .size(14)
                .color(colors.text_primary)
                .width(Length::FillPortion(2)),
            text(value)
                .size(14)
                .color(colors.text_muted)
                .width(Length::FillPortion(3)),
        ]
        .align_y(Alignment::Center),
    )
    .width(Length::Fill)
    .padding([10, 14])
    .style(theme::muted_card_style)
    .into()
}
