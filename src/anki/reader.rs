// SPDX-License-Identifier: MPL-2.0

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OpenFlags};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tempfile::TempDir;

use crate::models::{
    AnkiCard, AnkiCollectionSummary, AnkiDeckSummary, AnkiNoteType, AnkiRawNote, DeckSource,
    FieldMapping,
};

const ANKI_FIELD_SEPARATOR: char = '\x1f';
const ANKICONNECT_API_VERSION: u8 = 6;
const ANKICONNECT_BATCH_SIZE: usize = 250;

#[derive(Serialize)]
struct AnkiConnectRequest<'a, P> {
    action: &'a str,
    version: u8,
    params: P,
}

#[derive(Deserialize)]
struct AnkiConnectResponse<T> {
    result: Option<T>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct AnkiConnectFieldValue {
    value: String,
    order: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AnkiConnectCardInfo {
    card_id: i64,
    note: i64,
    deck_name: String,
    #[serde(default)]
    model_name: String,
    queue: i32,
    fields: BTreeMap<String, AnkiConnectFieldValue>,
}

/// Infers the sibling `collection.media` directory path for a `collection.anki2` file.
pub fn infer_media_dir(collection_path: &Path) -> PathBuf {
    match collection_path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent.join("collection.media"),
        _ => PathBuf::from("collection.media"),
    }
}

/// Extracts the first non-empty media filename from an Anki `[sound:...]` tag.
pub fn extract_sound_filename(raw_field: &str) -> Option<String> {
    let lower = raw_field.to_ascii_lowercase();
    let mut search_start = 0;

    while let Some(rel_idx) = lower[search_start..].find("[sound:") {
        let tag_start = search_start + rel_idx + "[sound:".len();
        let Some(rel_end) = raw_field[tag_start..].find(']') else {
            break;
        };
        let candidate = raw_field[tag_start..tag_start + rel_end].trim();
        if !candidate.is_empty() {
            return Some(candidate.to_string());
        }
        search_start = tag_start + rel_end + 1;
    }

    None
}

/// Strips `[sound:...]` tags and basic HTML markup from an Anki note field.
pub fn clean_anki_field_text(raw_field: &str) -> String {
    let without_sound = strip_sound_tags(raw_field);
    let with_breaks = replace_html_line_breaks(&without_sound);
    let without_tags = strip_html_tags(&with_breaks);
    let decoded = decode_common_html_entities(&without_tags);
    decoded.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Suggests default field mappings from note field names and sample note contents.
pub fn suggest_field_mapping(field_names: &[String], sample_notes: &[AnkiRawNote]) -> FieldMapping {
    let sound_fields = detect_sound_fields(field_names, sample_notes);

    let word_audio_field = find_matching_field(
        field_names,
        &[
            "wordaudio",
            "word_audio",
            "word audio",
            "vocab_audio",
            "vocabaudio",
            "audio",
            "sound",
            "pronunciation",
        ],
        None,
    )
    .or_else(|| sound_fields.first().cloned())
    .or_else(|| field_names.last().cloned())
    .unwrap_or_default();

    let sentence_audio_field = find_matching_field(
        field_names,
        &[
            "sentenceaudio",
            "sentence_audio",
            "sentence audio",
            "example_audio",
            "exampleaudio",
            "context_audio",
        ],
        Some(word_audio_field.as_str()),
    )
    .or_else(|| {
        sound_fields
            .iter()
            .find(|name| *name != &word_audio_field)
            .cloned()
    });

    let non_sound_fields: Vec<String> = field_names
        .iter()
        .filter(|name| !sound_fields.contains(name))
        .cloned()
        .collect();
    let text_candidates = if non_sound_fields.is_empty() {
        field_names
    } else {
        &non_sound_fields
    };

    let word_field = find_matching_field(
        text_candidates,
        &[
            "expression",
            "word",
            "vocabulary",
            "vocab",
            "kanji",
            "term",
            "front",
        ],
        None,
    )
    .or_else(|| text_candidates.first().cloned())
    .unwrap_or_default();

    let kana_field = find_matching_field(
        text_candidates,
        &[
            "reading",
            "kana",
            "furigana",
            "hiragana",
            "katakana",
            "yomi",
        ],
        None,
    )
    .or_else(|| {
        text_candidates
            .iter()
            .find(|name| *name != &word_field)
            .cloned()
    })
    .unwrap_or_else(|| word_field.clone());

    let sentence_field = find_matching_field(
        text_candidates,
        &["sentence", "example", "context", "phrase"],
        Some(word_field.as_str()),
    );

    FieldMapping {
        word_field,
        kana_field,
        sentence_field,
        word_audio_field,
        sentence_audio_field,
    }
}

/// Converts an [`AnkiRawNote`] into an [`AnkiCard`] using the active [`FieldMapping`].
pub fn build_anki_card(note: &AnkiRawNote, mapping: &FieldMapping) -> AnkiCard {
    let raw_word = note
        .fields
        .get(&mapping.word_field)
        .map(String::as_str)
        .unwrap_or_default();
    let raw_kana = note
        .fields
        .get(&mapping.kana_field)
        .map(String::as_str)
        .unwrap_or_default();
    let raw_sentence = mapping
        .sentence_field
        .as_ref()
        .and_then(|field| note.fields.get(field))
        .map(String::as_str)
        .unwrap_or_default();
    let raw_word_audio = note
        .fields
        .get(&mapping.word_audio_field)
        .map(String::as_str)
        .unwrap_or_default();
    let raw_sentence_audio = mapping
        .sentence_audio_field
        .as_ref()
        .and_then(|field| note.fields.get(field))
        .map(String::as_str)
        .unwrap_or_default();

    let cleaned_sentence = clean_anki_field_text(raw_sentence);
    let sentence = if cleaned_sentence.is_empty() {
        None
    } else {
        Some(cleaned_sentence)
    };

    AnkiCard {
        card_id: note.card_id,
        note_id: note.note_id,
        deck_name: note.deck_name.clone(),
        word: clean_anki_field_text(raw_word),
        kana: clean_anki_field_text(raw_kana),
        sentence,
        word_audio: extract_sound_filename(raw_word_audio).unwrap_or_default(),
        sentence_audio: extract_sound_filename(raw_sentence_audio),
        anki_queue: note.anki_queue,
        audio_issues: Vec::new(),
        last_pitch_error_percent: None,
    }
}

/// Inspects a local `collection.anki2` database via a safe temporary copy.
pub fn inspect_local_collection(
    collection_path: &Path,
    media_dir: &Path,
) -> Result<AnkiCollectionSummary> {
    let (_temp_dir, conn) = open_temp_collection(collection_path)?;
    let raw_decks = load_sqlite_decks(&conn)?;
    let note_types = load_sqlite_note_types(&conn)?;

    let deck_names_by_id: HashMap<i64, String> = raw_decks.iter().cloned().collect();
    let mut learned_notes_by_deck: HashMap<String, HashSet<i64>> = HashMap::new();
    let mut total_cards_by_deck: HashMap<String, usize> = HashMap::new();

    let mut stmt = conn
        .prepare("SELECT did, nid, queue FROM cards")
        .context("Failed to query Anki cards table; ensure the selected file is a valid collection.anki2")?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i32>(2)?,
            ))
        })
        .context("Failed to iterate rows from Anki cards table")?;

    for row in rows {
        let (did, nid, queue) = row.context("Failed to read card row from Anki database")?;
        if let Some(deck_name) = deck_names_by_id.get(&did) {
            let total = total_cards_by_deck.entry(deck_name.clone()).or_insert(0);
            *total = total.saturating_add(1);
            if queue == AnkiCard::LEARNED_QUEUE {
                learned_notes_by_deck
                    .entry(deck_name.clone())
                    .or_default()
                    .insert(nid);
            }
        }
    }

    let mut decks: Vec<AnkiDeckSummary> = raw_decks
        .into_iter()
        .map(|(deck_id, name)| {
            let mut learned_nids = HashSet::new();
            let mut total_card_count = 0_usize;

            for (candidate_name, count) in &total_cards_by_deck {
                if deck_matches_or_is_subdeck(candidate_name, &name) {
                    total_card_count = total_card_count.saturating_add(*count);
                }
            }
            for (candidate_name, nids) in &learned_notes_by_deck {
                if deck_matches_or_is_subdeck(candidate_name, &name) {
                    learned_nids.extend(nids.iter().copied());
                }
            }

            AnkiDeckSummary {
                deck_id,
                name,
                learned_card_count: learned_nids.len(),
                total_card_count,
            }
        })
        .collect();

    decks.sort_by(|left, right| {
        (left.learned_card_count == 0)
            .cmp(&(right.learned_card_count == 0))
            .then_with(|| left.name.cmp(&right.name))
    });

    Ok(AnkiCollectionSummary {
        source: DeckSource::LocalCollection {
            collection_path: collection_path.to_path_buf(),
            media_dir: media_dir.to_path_buf(),
        },
        decks,
        note_types,
    })
}

/// Loads graduated (`queue = 2`) raw notes for `deck_name` from a local `collection.anki2` copy.
pub fn load_local_learned_notes(
    collection_path: &Path,
    deck_name: &str,
    limit: Option<usize>,
) -> Result<Vec<AnkiRawNote>> {
    let (_temp_dir, conn) = open_temp_collection(collection_path)?;
    let raw_decks = load_sqlite_decks(&conn)?;
    let note_types = load_sqlite_note_types(&conn)?;

    let matching_decks: HashMap<i64, String> = raw_decks
        .into_iter()
        .filter(|(_, name)| deck_matches_or_is_subdeck(name, deck_name))
        .collect();
    if matching_decks.is_empty() {
        bail!("Deck '{deck_name}' was not found in the selected Anki collection");
    }

    let fields_by_model: HashMap<i64, Vec<String>> = note_types
        .into_iter()
        .map(|nt| (nt.id, nt.field_names))
        .collect();

    let mut stmt = conn
        .prepare(
            "SELECT c.id, c.nid, c.did, n.mid, n.flds, c.queue
             FROM cards c
             JOIN notes n ON n.id = c.nid
             WHERE c.queue = ?1
             ORDER BY c.id ASC",
        )
        .context("Failed to prepare learned-cards query on Anki database")?;

    let rows = stmt
        .query_map([AnkiCard::LEARNED_QUEUE], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i32>(5)?,
            ))
        })
        .context("Failed to query learned cards from Anki database")?;

    let mut seen_notes = HashSet::new();
    let mut notes = Vec::new();

    for row in rows {
        let (card_id, note_id, did, note_type_id, flds, anki_queue) =
            row.context("Failed to read learned note row from Anki database")?;
        let Some(card_deck_name) = matching_decks.get(&did) else {
            continue;
        };
        if !seen_notes.insert(note_id) {
            continue;
        }

        let field_names = fields_by_model.get(&note_type_id);
        let fields = map_sqlite_fields(&flds, field_names);

        notes.push(AnkiRawNote {
            card_id,
            note_id,
            deck_name: card_deck_name.clone(),
            note_type_id,
            anki_queue,
            fields,
        });

        if limit.is_some_and(|max_notes| notes.len() >= max_notes) {
            break;
        }
    }

    Ok(notes)
}

/// Imports graduated (`queue = 2`) [`AnkiCard`] items from a local `collection.anki2` file.
#[allow(dead_code)]
pub fn import_local_learned_cards(
    collection_path: &Path,
    deck_name: &str,
    mapping: &FieldMapping,
) -> Result<Vec<AnkiCard>> {
    mapping.validate()?;
    let notes = load_local_learned_notes(collection_path, deck_name, None)?;
    Ok(notes
        .iter()
        .map(|note| build_anki_card(note, mapping))
        .collect())
}

/// Inspects a running AnkiConnect HTTP endpoint and discovers decks and media folder location.
#[allow(dead_code)]
pub fn inspect_ankiconnect(endpoint_url: &str) -> Result<(AnkiCollectionSummary, Option<PathBuf>)> {
    let normalized_url = normalize_endpoint_url(endpoint_url)?;
    let agent = build_ankiconnect_agent();

    let version: u8 = post_ankiconnect(&agent, &normalized_url, "version", Value::Null)
        .context("Could not reach AnkiConnect; verify Anki is open with AnkiConnect installed")?;
    if version < ANKICONNECT_API_VERSION {
        bail!("AnkiConnect API version {version} is too old (version 6 or newer is required)");
    }

    let deck_map: BTreeMap<String, i64> =
        post_ankiconnect(&agent, &normalized_url, "deckNamesAndIds", Value::Null)
            .context("Failed to fetch deck list from AnkiConnect")?;

    let media_dir = post_ankiconnect::<Value, String>(
        &agent,
        &normalized_url,
        "getMediaDirPath",
        Value::Null,
    )
    .ok()
    .map(|raw| raw.trim().to_string())
    .filter(|raw| !raw.is_empty())
    .map(PathBuf::from);

    let note_types = fetch_ankiconnect_note_types(&agent, &normalized_url).unwrap_or_default();

    let mut decks: Vec<AnkiDeckSummary> = deck_map
        .into_iter()
        .map(|(name, deck_id)| AnkiDeckSummary {
            deck_id,
            name,
            learned_card_count: 0,
            total_card_count: 0,
        })
        .collect();
    decks.sort_by(|left, right| left.name.cmp(&right.name));

    Ok((
        AnkiCollectionSummary {
            source: DeckSource::AnkiConnect {
                endpoint_url: normalized_url,
            },
            decks,
            note_types,
        },
        media_dir,
    ))
}

/// Loads graduated (`queue = 2`) raw notes for `deck_name` from AnkiConnect.
pub fn load_ankiconnect_learned_notes(
    endpoint_url: &str,
    deck_name: &str,
    limit: Option<usize>,
) -> Result<Vec<AnkiRawNote>> {
    let normalized_url = normalize_endpoint_url(endpoint_url)?;
    let agent = build_ankiconnect_agent();
    let escaped_deck = deck_name.replace('"', "\\\"");
    let query = format!("deck:\"{escaped_deck}\" is:review");

    let card_ids: Vec<i64> = post_ankiconnect(
        &agent,
        &normalized_url,
        "findCards",
        serde_json::json!({ "query": query }),
    )
    .with_context(|| format!("Failed to search learned cards in deck '{deck_name}' via AnkiConnect"))?;

    let mut notes = Vec::new();
    let mut seen_notes = HashSet::new();

    for chunk in card_ids.chunks(ANKICONNECT_BATCH_SIZE) {
        let info_values: Vec<Value> = post_ankiconnect(
            &agent,
            &normalized_url,
            "cardsInfo",
            serde_json::json!({ "cards": chunk }),
        )
        .context("Failed to fetch card details from AnkiConnect")?;

        let parsed = parse_ankiconnect_cards_info(&info_values)?;
        for note in parsed {
            if note.anki_queue != AnkiCard::LEARNED_QUEUE || !seen_notes.insert(note.note_id) {
                continue;
            }
            notes.push(note);
            if limit.is_some_and(|max_notes| notes.len() >= max_notes) {
                return Ok(notes);
            }
        }
    }

    Ok(notes)
}

/// Imports graduated (`queue = 2`) [`AnkiCard`] items from AnkiConnect.
#[allow(dead_code)]
pub fn import_ankiconnect_learned_cards(
    endpoint_url: &str,
    deck_name: &str,
    mapping: &FieldMapping,
) -> Result<Vec<AnkiCard>> {
    mapping.validate()?;
    let notes = load_ankiconnect_learned_notes(endpoint_url, deck_name, None)?;
    Ok(notes
        .iter()
        .map(|note| build_anki_card(note, mapping))
        .collect())
}

/// Inspects either a local SQLite collection or an AnkiConnect server.
#[allow(dead_code)]
pub fn inspect_collection(source: &DeckSource) -> Result<(AnkiCollectionSummary, Option<PathBuf>)> {
    match source {
        DeckSource::LocalCollection {
            collection_path,
            media_dir,
        } => {
            let summary = inspect_local_collection(collection_path, media_dir)?;
            Ok((summary, Some(media_dir.clone())))
        }
        DeckSource::AnkiConnect { endpoint_url } => inspect_ankiconnect(endpoint_url),
    }
}

/// Loads graduated (`queue = 2`) raw notes for `deck_name` from the configured [`DeckSource`].
#[allow(dead_code)]
pub fn load_learned_notes(
    source: &DeckSource,
    deck_name: &str,
    limit: Option<usize>,
) -> Result<Vec<AnkiRawNote>> {
    match source {
        DeckSource::LocalCollection {
            collection_path, ..
        } => load_local_learned_notes(collection_path, deck_name, limit),
        DeckSource::AnkiConnect { endpoint_url } => {
            load_ankiconnect_learned_notes(endpoint_url, deck_name, limit)
        }
    }
}

/// Imports all graduated (`queue = 2`) [`AnkiCard`] items from the configured [`DeckSource`].
#[allow(dead_code)]
pub fn import_learned_cards(
    source: &DeckSource,
    deck_name: &str,
    mapping: &FieldMapping,
) -> Result<Vec<AnkiCard>> {
    mapping.validate()?;
    let notes = load_learned_notes(source, deck_name, None)?;
    Ok(notes
        .iter()
        .map(|note| build_anki_card(note, mapping))
        .collect())
}

fn open_temp_collection(collection_path: &Path) -> Result<(TempDir, Connection)> {
    if !collection_path.exists() {
        bail!(
            "Anki collection file does not exist at {}",
            collection_path.display()
        );
    }
    if !collection_path.is_file() {
        bail!(
            "Selected collection path is not a file: {}",
            collection_path.display()
        );
    }

    let temp_dir = TempDir::new().context("Failed to create temporary directory for Anki copy")?;
    let temp_db_path = temp_dir.path().join("collection.anki2");

    fs::copy(collection_path, &temp_db_path).with_context(|| {
        format!(
            "Failed to copy Anki collection from {} to temporary storage",
            collection_path.display()
        )
    })?;

    copy_sqlite_sidecar(collection_path, &temp_db_path, "-wal")?;
    copy_sqlite_sidecar(collection_path, &temp_db_path, "-shm")?;

    let conn = Connection::open_with_flags(
        &temp_db_path,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .with_context(|| {
        format!(
            "Failed to open SQLite copy of {}",
            collection_path.display()
        )
    })?;

    conn.create_collation("unicase", |left, right| {
        left.to_lowercase().cmp(&right.to_lowercase())
    })
    .context("Failed to register Anki unicase SQLite collation")?;

    Ok((temp_dir, conn))
}

fn copy_sqlite_sidecar(source_db: &Path, temp_db: &Path, suffix: &str) -> Result<()> {
    let mut source_os = source_db.as_os_str().to_os_string();
    source_os.push(suffix);
    let source_sidecar = PathBuf::from(source_os);

    if !source_sidecar.is_file() {
        return Ok(());
    }

    let mut temp_os = temp_db.as_os_str().to_os_string();
    temp_os.push(suffix);
    let temp_sidecar = PathBuf::from(temp_os);

    fs::copy(&source_sidecar, &temp_sidecar).with_context(|| {
        format!(
            "Failed to copy SQLite sidecar {}",
            source_sidecar.display()
        )
    })?;
    Ok(())
}

fn table_has_rows(conn: &Connection, table_name: &str) -> Result<bool> {
    let exists: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [table_name],
            |row| row.get(0),
        )
        .with_context(|| format!("Failed to inspect SQLite schema for table '{table_name}'"))?;

    if !exists {
        return Ok(false);
    }

    let sql = format!("SELECT EXISTS(SELECT 1 FROM \"{table_name}\" LIMIT 1)");
    conn.query_row(&sql, [], |row| row.get(0))
        .with_context(|| format!("Failed to check rows in table '{table_name}'"))
}

fn load_sqlite_decks(conn: &Connection) -> Result<Vec<(i64, String)>> {
    if table_has_rows(conn, "decks")? {
        let mut stmt = conn
            .prepare("SELECT id, name FROM decks")
            .context("Failed to read modern Anki decks table")?;
        let rows = stmt
            .query_map([], |row| {
                let id: i64 = row.get(0)?;
                let raw_name: String = row.get(1)?;
                Ok((id, normalize_deck_name(&raw_name)))
            })
            .context("Failed to iterate modern Anki decks table")?;

        let mut decks = Vec::new();
        for row in rows {
            decks.push(row.context("Failed to parse Anki deck row")?);
        }
        return Ok(decks);
    }

    let decks_json: String = conn
        .query_row("SELECT decks FROM col LIMIT 1", [], |row| row.get(0))
        .context("Failed to read decks metadata from Anki col table")?;
    let parsed: Value =
        serde_json::from_str(&decks_json).context("Failed to parse Anki col.decks JSON")?;
    let obj = parsed
        .as_object()
        .context("Expected Anki col.decks to be a JSON object")?;

    let mut decks = Vec::with_capacity(obj.len());
    for (key, value) in obj {
        let deck_id = value
            .get("id")
            .and_then(Value::as_i64)
            .or_else(|| key.parse::<i64>().ok())
            .unwrap_or(0);
        let raw_name = value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Default");
        decks.push((deck_id, normalize_deck_name(raw_name)));
    }
    Ok(decks)
}

fn load_sqlite_note_types(conn: &Connection) -> Result<Vec<AnkiNoteType>> {
    if table_has_rows(conn, "notetypes")? && table_has_rows(conn, "fields")? {
        let mut nt_stmt = conn
            .prepare("SELECT id, name FROM notetypes ORDER BY id ASC")
            .context("Failed to read modern Anki notetypes table")?;
        let nt_rows = nt_stmt
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))
            .context("Failed to iterate modern Anki notetypes table")?;

        let mut note_types_map: BTreeMap<i64, AnkiNoteType> = BTreeMap::new();
        for row in nt_rows {
            let (id, name) = row.context("Failed to parse Anki notetype row")?;
            note_types_map.insert(
                id,
                AnkiNoteType {
                    id,
                    name,
                    field_names: Vec::new(),
                },
            );
        }

        let mut field_stmt = conn
            .prepare("SELECT ntid, name FROM fields ORDER BY ntid ASC, ord ASC")
            .context("Failed to read modern Anki fields table")?;
        let field_rows = field_stmt
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))
            .context("Failed to iterate modern Anki fields table")?;

        for row in field_rows {
            let (ntid, field_name) = row.context("Failed to parse Anki field row")?;
            if let Some(nt) = note_types_map.get_mut(&ntid) {
                nt.field_names.push(field_name);
            }
        }

        return Ok(note_types_map.into_values().collect());
    }

    let models_json: String = conn
        .query_row("SELECT models FROM col LIMIT 1", [], |row| row.get(0))
        .context("Failed to read note types from Anki col.models")?;
    let parsed: Value =
        serde_json::from_str(&models_json).context("Failed to parse Anki col.models JSON")?;
    let obj = parsed
        .as_object()
        .context("Expected Anki col.models to be a JSON object")?;

    let mut note_types = Vec::with_capacity(obj.len());
    for (key, value) in obj {
        let id = value
            .get("id")
            .and_then(Value::as_i64)
            .or_else(|| key.parse::<i64>().ok())
            .unwrap_or(0);
        let name = value
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Note Type")
            .to_string();

        let mut flds: Vec<(usize, String)> = value
            .get("flds")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, item)| {
                        let field_name = item.get("name").and_then(Value::as_str)?;
                        let ord = item
                            .get("ord")
                            .and_then(Value::as_u64)
                            .map(|v| v as usize)
                            .unwrap_or(idx);
                        Some((ord, field_name.to_string()))
                    })
                    .collect()
            })
            .unwrap_or_default();
        flds.sort_by_key(|(ord, _)| *ord);

        note_types.push(AnkiNoteType {
            id,
            name,
            field_names: flds.into_iter().map(|(_, name)| name).collect(),
        });
    }

    note_types.sort_by_key(|nt| nt.id);
    Ok(note_types)
}

fn normalize_deck_name(raw_name: &str) -> String {
    raw_name.replace(ANKI_FIELD_SEPARATOR, "::")
}

fn deck_matches_or_is_subdeck(candidate: &str, selected: &str) -> bool {
    if candidate == selected {
        return true;
    }
    candidate
        .strip_prefix(selected)
        .is_some_and(|rest| rest.starts_with("::"))
}

fn map_sqlite_fields(flds: &str, field_names: Option<&Vec<String>>) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    for (idx, value) in flds.split(ANKI_FIELD_SEPARATOR).enumerate() {
        let key = field_names
            .and_then(|names| names.get(idx))
            .cloned()
            .unwrap_or_else(|| format!("Field {}", idx + 1));
        map.insert(key, value.to_string());
    }
    map
}

fn normalize_endpoint_url(endpoint_url: &str) -> Result<String> {
    let trimmed = endpoint_url.trim().trim_end_matches('/');
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        bail!("AnkiConnect endpoint URL must start with http:// or https://");
    }
    Ok(trimmed.to_string())
}

fn build_ankiconnect_agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_connect(Some(Duration::from_secs(5)))
        .timeout_global(Some(Duration::from_secs(15)))
        .build();
    config.into()
}

fn post_ankiconnect<P: Serialize, T: DeserializeOwned>(
    agent: &ureq::Agent,
    endpoint_url: &str,
    action: &str,
    params: P,
) -> Result<T> {
    let payload = AnkiConnectRequest {
        action,
        version: ANKICONNECT_API_VERSION,
        params,
    };

    let mut response = agent
        .post(endpoint_url)
        .send_json(&payload)
        .with_context(|| format!("AnkiConnect request '{action}' to {endpoint_url} failed"))?;

    let decoded: AnkiConnectResponse<T> = response
        .body_mut()
        .read_json()
        .with_context(|| format!("Failed to decode AnkiConnect '{action}' JSON response"))?;

    if let Some(err_msg) = decoded.error {
        bail!("AnkiConnect '{action}' returned an error: {err_msg}");
    }

    decoded
        .result
        .with_context(|| format!("AnkiConnect '{action}' response did not include a result"))
}

#[allow(dead_code)]
fn fetch_ankiconnect_note_types(
    agent: &ureq::Agent,
    endpoint_url: &str,
) -> Result<Vec<AnkiNoteType>> {
    let model_map: BTreeMap<String, i64> =
        post_ankiconnect(agent, endpoint_url, "modelNamesAndIds", Value::Null)?;
    let mut note_types = Vec::with_capacity(model_map.len());
    for (name, id) in model_map {
        let field_names: Vec<String> = post_ankiconnect(
            agent,
            endpoint_url,
            "modelFieldNames",
            serde_json::json!({ "modelName": name }),
        )
        .unwrap_or_default();
        note_types.push(AnkiNoteType {
            id,
            name,
            field_names,
        });
    }
    note_types.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(note_types)
}

fn parse_ankiconnect_cards_info(values: &[Value]) -> Result<Vec<AnkiRawNote>> {
    let mut notes = Vec::with_capacity(values.len());
    for value in values {
        let info: AnkiConnectCardInfo = serde_json::from_value(value.clone())
            .context("Failed to parse card item from AnkiConnect cardsInfo response")?;

        let mut ordered_fields: Vec<(usize, String, String)> = info
            .fields
            .into_iter()
            .map(|(name, field)| (field.order, name, field.value))
            .collect();
        ordered_fields.sort_by_key(|(order, _, _)| *order);

        let mut fields = BTreeMap::new();
        for (_, name, val) in ordered_fields {
            fields.insert(name, val);
        }

        let note_type_id = info
            .model_name
            .bytes()
            .fold(0_i64, |acc, byte| acc.wrapping_mul(31).wrapping_add(i64::from(byte)));

        notes.push(AnkiRawNote {
            card_id: info.card_id,
            note_id: info.note,
            deck_name: info.deck_name,
            note_type_id,
            anki_queue: info.queue,
            fields,
        });
    }
    Ok(notes)
}

fn strip_sound_tags(input: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let mut out = String::with_capacity(input.len());
    let mut cursor = 0;

    while let Some(rel_idx) = lower[cursor..].find("[sound:") {
        let tag_start = cursor + rel_idx;
        out.push_str(&input[cursor..tag_start]);
        if let Some(rel_end) = input[tag_start..].find(']') {
            cursor = tag_start + rel_end + 1;
        } else {
            cursor = tag_start + "[sound:".len();
        }
    }

    out.push_str(&input[cursor..]);
    out
}

fn replace_html_line_breaks(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut cursor = 0;

    while let Some(rel_lt) = input[cursor..].find('<') {
        let lt_idx = cursor + rel_lt;
        result.push_str(&input[cursor..lt_idx]);
        if let Some(rel_gt) = input[lt_idx..].find('>') {
            let tag = &input[lt_idx + 1..lt_idx + rel_gt];
            let trimmed = tag.trim().trim_end_matches('/').trim();
            if trimmed.eq_ignore_ascii_case("br") {
                result.push(' ');
                cursor = lt_idx + rel_gt + 1;
            } else {
                result.push_str(&input[lt_idx..=lt_idx + rel_gt]);
                cursor = lt_idx + rel_gt + 1;
            }
        } else {
            result.push_str(&input[lt_idx..]);
            return result;
        }
    }

    result.push_str(&input[cursor..]);
    result
}

fn strip_html_tags(input: &str) -> String {
    let mut result = String::with_capacity(input.len());
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }
    result
}

fn decode_common_html_entities(input: &str) -> String {
    input
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

fn detect_sound_fields(field_names: &[String], sample_notes: &[AnkiRawNote]) -> Vec<String> {
    field_names
        .iter()
        .filter(|field_name| {
            sample_notes.iter().any(|note| {
                note.fields
                    .get(*field_name)
                    .is_some_and(|val| extract_sound_filename(val).is_some())
            })
        })
        .cloned()
        .collect()
}

fn find_matching_field(
    candidates: &[String],
    patterns: &[&str],
    exclude: Option<&str>,
) -> Option<String> {
    for pattern in patterns {
        if let Some(exact) = candidates.iter().find(|name| {
            exclude != Some(name.as_str()) && name.trim().eq_ignore_ascii_case(pattern)
        }) {
            return Some(exact.clone());
        }
    }
    for pattern in patterns {
        if let Some(partial) = candidates.iter().find(|name| {
            exclude != Some(name.as_str()) && name.to_ascii_lowercase().contains(pattern)
        }) {
            return Some(partial.clone());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_classic_collection_db(path: &Path) -> Result<()> {
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "
            CREATE TABLE col (
                id INTEGER PRIMARY KEY,
                models TEXT NOT NULL,
                decks TEXT NOT NULL
            );
            CREATE TABLE notes (
                id INTEGER PRIMARY KEY,
                mid INTEGER NOT NULL,
                flds TEXT NOT NULL
            );
            CREATE TABLE cards (
                id INTEGER PRIMARY KEY,
                nid INTEGER NOT NULL,
                did INTEGER NOT NULL,
                ord INTEGER NOT NULL,
                queue INTEGER NOT NULL
            );
            ",
        )?;

        let models = serde_json::json!({
            "10": {
                "id": 10,
                "name": "Japanese Core",
                "flds": [
                    { "name": "Expression", "ord": 0 },
                    { "name": "Reading", "ord": 1 },
                    { "name": "WordAudio", "ord": 2 },
                    { "name": "Sentence", "ord": 3 },
                    { "name": "SentenceAudio", "ord": 4 }
                ]
            }
        })
        .to_string();

        let decks = serde_json::json!({
            "1": { "id": 1, "name": "Default" },
            "200": { "id": 200, "name": "Core 2k" },
            "201": { "id": 201, "name": "Core 2k::Step 1" }
        })
        .to_string();

        conn.execute(
            "INSERT INTO col (id, models, decks) VALUES (1, ?1, ?2)",
            [models, decks],
        )?;

        conn.execute(
            "INSERT INTO notes (id, mid, flds) VALUES (1001, 10, ?1)",
            ["靴\x1fくつ\x1f[sound:kutsu.mp3]\x1f<b>新しい靴</b>を買った。\x1f[sound:kutsu_ex.mp3]"],
        )?;
        conn.execute(
            "INSERT INTO notes (id, mid, flds) VALUES (1002, 10, ?1)",
            ["橋\x1fはし\x1f[sound:hashi.mp3]\x1f\x1f"],
        )?;
        conn.execute(
            "INSERT INTO notes (id, mid, flds) VALUES (1003, 10, ?1)",
            ["雨\x1fあめ\x1f[sound:ame.mp3]\x1f雨が降る。\x1f"],
        )?;

        // Note 1001 has two review templates (ord 0 & 1) -> must deduplicate to card_id 5001.
        conn.execute(
            "INSERT INTO cards (id, nid, did, ord, queue) VALUES (5001, 1001, 200, 0, 2)",
            [],
        )?;
        conn.execute(
            "INSERT INTO cards (id, nid, did, ord, queue) VALUES (5002, 1001, 200, 1, 2)",
            [],
        )?;
        // Note 1002 is in subdeck Core 2k::Step 1 and learned (queue = 2).
        conn.execute(
            "INSERT INTO cards (id, nid, did, ord, queue) VALUES (5003, 1002, 201, 0, 2)",
            [],
        )?;
        // Note 1003 is new (queue = 0) -> must be excluded from learned queries.
        conn.execute(
            "INSERT INTO cards (id, nid, did, ord, queue) VALUES (5004, 1003, 200, 0, 0)",
            [],
        )?;
        Ok(())
    }

    fn create_modern_collection_db(path: &Path) -> Result<()> {
        let conn = Connection::open(path)?;
        conn.create_collation("unicase", |a, b| a.to_lowercase().cmp(&b.to_lowercase()))?;
        conn.execute_batch(
            "
            CREATE TABLE col (id INTEGER PRIMARY KEY, models TEXT NOT NULL, decks TEXT NOT NULL);
            CREATE TABLE decks (id INTEGER PRIMARY KEY, name TEXT NOT NULL COLLATE unicase);
            CREATE TABLE notetypes (id INTEGER PRIMARY KEY, name TEXT NOT NULL COLLATE unicase);
            CREATE TABLE fields (
                ntid INTEGER NOT NULL,
                ord INTEGER NOT NULL,
                name TEXT NOT NULL COLLATE unicase,
                PRIMARY KEY (ntid, ord)
            );
            CREATE TABLE notes (id INTEGER PRIMARY KEY, mid INTEGER NOT NULL, flds TEXT NOT NULL);
            CREATE TABLE cards (id INTEGER PRIMARY KEY, nid INTEGER NOT NULL, did INTEGER NOT NULL, ord INTEGER NOT NULL, queue INTEGER NOT NULL);
            ",
        )?;

        conn.execute("INSERT INTO col (id, models, decks) VALUES (1, '{}', '{}')", [])?;
        conn.execute("INSERT INTO decks (id, name) VALUES (300, 'JLPT\x1fN4')", [])?;
        conn.execute("INSERT INTO notetypes (id, name) VALUES (40, 'Vocab')", [])?;
        conn.execute("INSERT INTO fields (ntid, ord, name) VALUES (40, 0, 'Word')", [])?;
        conn.execute("INSERT INTO fields (ntid, ord, name) VALUES (40, 1, 'Kana')", [])?;
        conn.execute("INSERT INTO fields (ntid, ord, name) VALUES (40, 2, 'Audio')", [])?;

        conn.execute(
            "INSERT INTO notes (id, mid, flds) VALUES (2001, 40, '猫\x1fねこ\x1f[sound:neko.ogg]')",
            [],
        )?;
        conn.execute(
            "INSERT INTO cards (id, nid, did, ord, queue) VALUES (6001, 2001, 300, 0, 2)",
            [],
        )?;
        Ok(())
    }

    #[test]
    fn sound_extraction_and_html_cleaning_handle_anki_markup() {
        assert_eq!(
            extract_sound_filename(" [SOUND: voice_01.mp3 ] "),
            Some("voice_01.mp3".to_string())
        );
        assert_eq!(
            extract_sound_filename("[sound: ] [sound:backup.ogg]"),
            Some("backup.ogg".to_string())
        );
        assert_eq!(extract_sound_filename("no audio here"), None);

        let raw = "<div>新（あたら）しい<br/>靴 &amp; 傘&nbsp;[sound:clip.mp3]</div>";
        assert_eq!(clean_anki_field_text(raw), "新（あたら）しい 靴 & 傘");
    }

    #[test]
    fn classic_sqlite_reader_copies_safely_and_filters_only_learned_cards() -> Result<()> {
        let dir = TempDir::new()?;
        let db_path = dir.path().join("collection.anki2");
        let media_dir = infer_media_dir(&db_path);
        assert_eq!(media_dir, dir.path().join("collection.media"));
        create_classic_collection_db(&db_path)?;

        // Hold an exclusive write transaction open on the original DB while reading temp copy.
        let mut lock_conn = Connection::open(&db_path)?;
        let tx = lock_conn.transaction()?;
        tx.execute("INSERT INTO notes (id, mid, flds) VALUES (9999, 10, 'x\x1fx\x1fx\x1fx\x1fx')", [])?;

        let summary = inspect_local_collection(&db_path, &media_dir)?;
        let core_deck = summary
            .decks
            .iter()
            .find(|deck| deck.name == "Core 2k")
            .context("Expected Core 2k deck in summary")?;
        assert_eq!(core_deck.learned_card_count, 2);
        assert_eq!(core_deck.total_card_count, 4);

        let notes = load_local_learned_notes(&db_path, "Core 2k", None)?;
        assert_eq!(notes.len(), 2);
        assert_eq!(notes[0].card_id, 5001);
        assert_eq!(notes[1].card_id, 5003);

        let field_names = summary.note_types[0].field_names.clone();
        let mapping = suggest_field_mapping(&field_names, &notes);
        assert_eq!(mapping.word_field, "Expression");
        assert_eq!(mapping.kana_field, "Reading");
        assert_eq!(mapping.word_audio_field, "WordAudio");
        assert_eq!(mapping.sentence_field.as_deref(), Some("Sentence"));
        assert_eq!(
            mapping.sentence_audio_field.as_deref(),
            Some("SentenceAudio")
        );

        let cards = import_local_learned_cards(&db_path, "Core 2k", &mapping)?;
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].word, "靴");
        assert_eq!(cards[0].kana, "くつ");
        assert_eq!(cards[0].sentence.as_deref(), Some("新しい靴を買った。"));
        assert_eq!(cards[0].word_audio, "kutsu.mp3");
        assert_eq!(cards[0].sentence_audio.as_deref(), Some("kutsu_ex.mp3"));
        assert!(cards.iter().all(AnkiCard::is_learned));

        tx.rollback()?;
        Ok(())
    }

    #[test]
    fn modern_sqlite_schema_with_unicase_collation_reads_decks_and_fields() -> Result<()> {
        let dir = TempDir::new()?;
        let db_path = dir.path().join("collection.anki2");
        let media_dir = infer_media_dir(&db_path);
        create_modern_collection_db(&db_path)?;

        let summary = inspect_local_collection(&db_path, &media_dir)?;
        assert_eq!(summary.decks.len(), 1);
        assert_eq!(summary.decks[0].name, "JLPT::N4");
        assert_eq!(summary.decks[0].learned_card_count, 1);

        let notes = load_local_learned_notes(&db_path, "JLPT::N4", None)?;
        assert_eq!(notes.len(), 1);
        let mapping = suggest_field_mapping(&summary.note_types[0].field_names, &notes);
        let card = build_anki_card(&notes[0], &mapping);
        assert_eq!(card.word, "猫");
        assert_eq!(card.kana, "ねこ");
        assert_eq!(card.word_audio, "neko.ogg");
        Ok(())
    }

    #[test]
    fn missing_or_non_anki_database_returns_clean_error() -> Result<()> {
        let dir = TempDir::new()?;
        let missing = dir.path().join("missing.anki2");
        assert!(inspect_local_collection(&missing, dir.path()).is_err());

        let corrupt = dir.path().join("not_sqlite.anki2");
        fs::write(&corrupt, b"not a sqlite file")?;
        assert!(inspect_local_collection(&corrupt, dir.path()).is_err());
        Ok(())
    }

    #[test]
    fn ankiconnect_cards_info_parser_maps_ordered_fields() -> Result<()> {
        let payload = vec![serde_json::json!({
            "cardId": 77,
            "note": 88,
            "deckName": "Immersion",
            "modelName": "Mining",
            "queue": 2,
            "fields": {
                "Audio": { "value": "[sound:mizu.mp3]", "order": 2 },
                "Expression": { "value": "水", "order": 0 },
                "Reading": { "value": "みず", "order": 1 }
            }
        })];

        let parsed = parse_ankiconnect_cards_info(&payload)?;
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].card_id, 77);
        assert_eq!(parsed[0].note_id, 88);
        assert_eq!(parsed[0].deck_name, "Immersion");
        assert_eq!(parsed[0].fields.get("Expression").map(String::as_str), Some("水"));
        assert_eq!(parsed[0].fields.get("Audio").map(String::as_str), Some("[sound:mizu.mp3]"));
        Ok(())
    }
}
