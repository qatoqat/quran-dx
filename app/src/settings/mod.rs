//! User settings: shared state (CONTRACT), persistence, settings page,
//! word-by-word rendering and footnotes.
//!
//! CONTRACT for other modules: `Settings` fields/methods below, `use_settings()`,
//! `provide_settings()`, `FontLoader`, `SettingsPage`, `wbw::WordView`,
//! `wbw::TranslationText`. Add fields freely; don't rename/remove.

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

pub mod html;
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
    #[allow(dead_code)] // used by the reader module
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
    #[allow(dead_code)] // used by the reader module
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

/// Google Fonts stylesheet for both supported scripts. Browsers only download a
/// face once text actually uses it, so loading both families is cheap.
const GOOGLE_FONTS_CSS: &str =
    "https://fonts.googleapis.com/css2?family=Amiri+Quran&family=Noto+Nastaliq+Urdu:wght@400;700&display=swap";

impl QuranScript {
    /// CSS `font-family` value used for `--arabic-font`.
    pub fn font_family(self) -> &'static str {
        match self {
            QuranScript::Uthmani => {
                r#""Amiri Quran", "KFGQPC Uthmanic Script HAFS", "Scheherazade New", "Amiri", serif"#
            }
            QuranScript::IndoPak => {
                r#""Noto Nastaliq Urdu", "Jameel Noori Nastaleeq", "Scheherazade New", serif"#
            }
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            QuranScript::Uthmani => "Uthmani",
            QuranScript::IndoPak => "IndoPak (Nastaliq)",
        }
    }
}

/// Format thousandths of a rem as a CSS length, e.g. `1050` → `1.05rem`.
fn fmt_rem(milli: u32) -> String {
    let whole = milli / 1000;
    let frac = milli % 1000;
    if frac == 0 {
        format!("{whole}rem")
    } else {
        let frac = format!("{frac:03}");
        format!("{whole}.{}rem", frac.trim_end_matches('0'))
    }
}

/// Arabic text size for a 1..=10 scale (clamped). 4 (default) → 2rem, 1 → 1.1rem, 10 → 3.8rem.
pub fn arabic_font_size(scale: u8) -> String {
    let s = u32::from(scale.clamp(1, 10));
    fmt_rem(1100 + 300 * (s - 1))
}

/// Translation text size for a 1..=10 scale (clamped). 3 (default) → 1.05rem, 1 → 0.8rem, 10 → 1.925rem.
pub fn translation_font_size(scale: u8) -> String {
    let s = u32::from(scale.clamp(1, 10));
    fmt_rem(800 + 125 * (s - 1))
}

/// CSS custom properties derived from settings.
pub fn css_variables(settings: &Settings) -> String {
    format!(
        ":root {{ --arabic-size: {}; --translation-size: {}; --arabic-font: {}; }}",
        arabic_font_size(settings.arabic_font_scale),
        translation_font_size(settings.translation_font_scale),
        settings.script.font_family(),
    )
}

/// Right-to-left languages, matched by ISO code (`ur`, `ar-SA`) or by English
/// name as used in the API's `language_name` (`urdu`, `Arabic`).
pub fn is_rtl_language(lang: &str) -> bool {
    const CODES: &[&str] = &["ar", "ur", "fa", "he", "iw", "ps", "sd", "ug", "dv", "yi", "ckb", "ks", "bal"];
    const NAMES: &[&str] = &[
        "arabic", "urdu", "persian", "farsi", "hebrew", "pashto", "sindhi", "uyghur", "uighur",
        "divehi", "dhivehi", "yiddish", "kurdish", "sorani", "kashmiri", "balochi",
    ];
    let lang = lang.trim().to_ascii_lowercase();
    let code = lang.split(['-', '_']).next().unwrap_or("");
    CODES.contains(&code) || NAMES.contains(&lang.as_str())
}

/// Sanitize a locale for use as an HTML `lang` value (and inside a JS string).
pub fn safe_lang_tag(locale: &str) -> String {
    let ok = !locale.is_empty()
        && locale.len() <= 16
        && locale.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok { locale.replace('_', "-") } else { "en".into() }
}

/// Injects web fonts and CSS variables derived from settings, and sets the
/// document `lang`/`dir` from the locale.
#[component]
pub fn FontLoader() -> Element {
    let settings = use_settings();
    let css = use_memo(move || css_variables(&settings.read()));
    let locale = use_memo(move || safe_lang_tag(&settings.read().locale));

    use_effect(move || {
        let lang = locale();
        let dir = if is_rtl_language(&lang) { "rtl" } else { "ltr" };
        // `lang` is restricted to [A-Za-z0-9-], so it is safe inside the JS string.
        let _ = document::eval(&format!(
            "document.documentElement.lang = '{lang}'; document.documentElement.dir = '{dir}';"
        ));
    });

    rsx! {
        document::Link { rel: "preconnect", href: "https://fonts.googleapis.com" }
        document::Link { rel: "preconnect", href: "https://fonts.gstatic.com", crossorigin: "anonymous" }
        document::Link { rel: "stylesheet", href: GOOGLE_FONTS_CSS }
        // A plain <style> element (unlike `document::Style`) updates reactively.
        style { id: "quran-settings-vars", {css()} }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn font_scale_mapping() {
        assert_eq!(arabic_font_size(4), "2rem");
        assert_eq!(arabic_font_size(1), "1.1rem");
        assert_eq!(arabic_font_size(10), "3.8rem");
        assert_eq!(arabic_font_size(0), "1.1rem");
        assert_eq!(arabic_font_size(200), "3.8rem");
        assert_eq!(translation_font_size(3), "1.05rem");
        assert_eq!(translation_font_size(1), "0.8rem");
        assert_eq!(translation_font_size(2), "0.925rem");
        assert_eq!(translation_font_size(10), "1.925rem");
        // Strictly increasing.
        for s in 1..10u8 {
            let px = |v: String| v.trim_end_matches("rem").parse::<f64>().unwrap();
            assert!(px(arabic_font_size(s)) < px(arabic_font_size(s + 1)));
            assert!(px(translation_font_size(s)) < px(translation_font_size(s + 1)));
        }
    }

    #[test]
    fn css_vars_follow_settings() {
        let mut s = Settings::default();
        let css = css_variables(&s);
        assert!(css.contains("--arabic-size: 2rem;"));
        assert!(css.contains("--translation-size: 1.05rem;"));
        assert!(css.contains("Amiri Quran"));
        s.script = QuranScript::IndoPak;
        assert!(css_variables(&s).contains("Noto Nastaliq Urdu"));
    }

    #[test]
    fn rtl_detection() {
        for l in ["ar", "ur", "fa", "ar-SA", "Urdu", "arabic", "Persian", " urdu "] {
            assert!(is_rtl_language(l), "{l}");
        }
        for l in ["en", "id", "english", "French", "", "bangla", "turkish"] {
            assert!(!is_rtl_language(l), "{l}");
        }
    }

    #[test]
    fn lang_tag_sanitized() {
        assert_eq!(safe_lang_tag("ur"), "ur");
        assert_eq!(safe_lang_tag("pt_BR"), "pt-BR");
        assert_eq!(safe_lang_tag("x'; alert(1); '"), "en");
        assert_eq!(safe_lang_tag(""), "en");
    }
}
