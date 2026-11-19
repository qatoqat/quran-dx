//! User settings: shared state (CONTRACT), persistence, settings page,
//! word-by-word rendering and footnotes.
//!
//! CONTRACT for other modules: `Settings` fields/methods below, `use_settings()`,
//! `provide_settings()`, `FontLoader`, `SettingsPage`, `wbw::WordView`,
//! `wbw::TranslationText`. Add fields freely; don't rename/remove.

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

pub mod storage;
pub mod wbw;

mod page;
pub use page::SettingsPage;

/// Which Arabic script/text field to render.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum QuranScript {
    #[default]
    Uthmani,
    IndoPak,
}

impl QuranScript {
    /// API field name for this script (verse `fields` and `word_fields`).
    pub fn field(self) -> &'static str {
        match self {
            QuranScript::Uthmani => "text_uthmani",
            QuranScript::IndoPak => "text_indopak",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WbwDisplay {
    /// Show word translation/transliteration on hover/tap.
    #[default]
    Tooltip,
    /// Show it under each word.
    Inline,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WordByWord {
    pub show_translation: bool,
    pub show_transliteration: bool,
    pub display: WbwDisplay,
}

impl Default for WordByWord {
    fn default() -> Self {
        Self { show_translation: true, show_transliteration: false, display: WbwDisplay::Tooltip }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// ISO code used for chapter names and word-by-word language (`en`, `ur`, `id`, ...).
    pub locale: String,
    /// Selected translation resource ids, in display order.
    pub translations: Vec<u32>,
    pub script: QuranScript,
    /// 1..=10 scale for the Arabic text.
    pub arabic_font_scale: u8,
    /// 1..=10 scale for translations.
    pub translation_font_scale: u8,
    pub word_by_word: WordByWord,
    /// Saved for future audio support (7 = Mishari Rashid al-Afasy).
    pub reciter: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            locale: "en".into(),
            translations: vec![131],
            script: QuranScript::Uthmani,
            arabic_font_scale: 4,
            translation_font_scale: 3,
            word_by_word: WordByWord::default(),
            reciter: 7,
        }
    }
}

impl Settings {
    /// Query to use for every `/verses/by_*` request.
    pub fn verse_query(&self) -> quran_api::VerseQuery {
        quran_api::VerseQuery {
            language: self.locale.clone(),
            translations: self.translations.clone(),
            words: true,
            fields: vec![self.script.field().into()],
            word_fields: vec![self.script.field().into()],
            translation_fields: vec!["resource_name".into(), "language_name".into()],
            page: 1,
            per_page: 50,
        }
    }
}

/// Call once in `App`. Loads persisted settings and saves on every change.
pub fn provide_settings() -> Signal<Settings> {
    let settings = use_context_provider(|| Signal::new(storage::load().unwrap_or_default()));
    use_effect(move || storage::save(&settings.read()));
    settings
}

pub fn use_settings() -> Signal<Settings> {
    use_context::<Signal<Settings>>()
}

/// Injects Arabic font faces / CSS variables derived from settings.
#[component]
pub fn FontLoader() -> Element {
    // TODO(settings agent): web fonts + CSS variables for font scales.
    rsx! {}
}
