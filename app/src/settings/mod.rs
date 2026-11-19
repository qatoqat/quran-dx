//! User settings: shared state, persistence, the settings panel, the theme and
//! the reading pointer, and verse translation text with footnotes.

use dioxus::prelude::*;
use serde::{Deserialize, Serialize};

pub mod html;
pub mod storage;
pub mod translation;

mod panel;
mod pointer;
pub use panel::SettingsPanel;
pub use pointer::use_reading_pointer;

/// Colours of the app. `Auto` follows the system's light or dark setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Theme {
    #[default]
    Auto,
    Light,
    Dark,
    Sepia,
    /// The reader's own background and accent ([`CustomTheme`]); every other
    /// colour is a shade worked out from those two.
    Custom,
}

/// The two colours a custom theme is built from, as `#rrggbb`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomTheme {
    pub background: String,
    pub accent: String,
}

pub const DEFAULT_CUSTOM_BACKGROUND: &str = "#101a2b";
pub const DEFAULT_ACCENT: &str = "#2ca4ab";

impl Default for CustomTheme {
    fn default() -> Self {
        Self { background: DEFAULT_CUSTOM_BACKGROUND.into(), accent: DEFAULT_ACCENT.into() }
    }
}

/// Relative luminance (WCAG) of a `#rrggbb` colour: 0 = black, 1 = white.
pub fn luminance(color: &str) -> Option<f64> {
    let hex = sanitize_color(color)?;
    let channel = |i: usize| {
        let c = f64::from(u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0)) / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    Some(0.2126 * channel(1) + 0.7152 * channel(3) + 0.0722 * channel(5))
}

impl CustomTheme {
    fn background(&self) -> String {
        sanitize_color(&self.background).unwrap_or_else(|| DEFAULT_CUSTOM_BACKGROUND.into())
    }

    /// A background on which white text reads better than black.
    pub fn is_dark(&self) -> bool {
        luminance(&self.background()).unwrap_or(0.0) < 0.179
    }

    /// The whole palette from the two colours: text is near white or near black
    /// (whichever contrasts with the background) with a touch of the background
    /// in it; cards, borders and muted text are mixes of the text and the
    /// background; text on the accent is black or white by the accent's lightness.
    fn css(&self) -> String {
        let bg = self.background();
        let accent = sanitize_color(&self.accent).unwrap_or_else(|| DEFAULT_ACCENT.into());
        let on_accent = if luminance(&accent).unwrap_or(0.0) < 0.35 { "#fff" } else { "#111" };
        let (scheme, ink, lift, tip) = if self.is_dark() {
            ("dark", "white", 6, "color-mix(in oklab, var(--bg) 82%, white)")
        } else {
            ("light", "black", 45, "var(--fg)")
        };
        format!(
            ":root {{ color-scheme: {scheme}; --bg: {bg}; --fg: color-mix(in oklab, {bg} 10%, {ink}); \
             --card: color-mix(in oklab, {bg}, white {lift}%); --border: color-mix(in oklab, var(--fg) 15%, var(--bg)); \
             --muted: color-mix(in oklab, var(--fg) 60%, var(--bg)); --accent: {accent}; --on-accent: {on_accent}; \
             --tip-bg: {tip}; }}"
        )
    }
}

/// Sepia: the light colours on warm paper.
const SEPIA_CSS: &str = ":root { color-scheme: light; --bg: #f4ecd8; --fg: #3b2f20; --muted: #7b6a55; \
    --border: #dccfb2; --card: #fbf5e4; --ornament: #96690f; --tip-bg: #3b2f20; }";

impl Theme {
    pub const ALL: [Theme; 5] = [Theme::Auto, Theme::Light, Theme::Dark, Theme::Sepia, Theme::Custom];

    pub fn label(self) -> &'static str {
        match self {
            Theme::Auto => "Auto",
            Theme::Light => "Light",
            Theme::Dark => "Dark",
            Theme::Sepia => "Sepia",
            Theme::Custom => "Custom",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum PointerShape {
    /// A filled disc.
    #[default]
    Circle,
    /// An outline.
    Ring,
}

/// A reading pointer that replaces the mouse cursor below the header.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pointer {
    pub enabled: bool,
    pub shape: PointerShape,
    /// `#rrggbb`.
    pub color: String,
    /// Diameter in tenths of a mushaf line's height, 1..=10.
    pub size: u8,
    /// Percent, 5..=100.
    pub opacity: u8,
    /// Number of fading copies that follow it, 0..=10.
    pub trail: u8,
    /// How far those copies fall behind, 1..=10 (5 = default look).
    pub trail_distance: u8,
    /// Softness of its edge, 0..=10.
    pub blur: u8,
    /// How much the colours of the text under it are intensified, 0..=10.
    pub glow: u8,
    /// How much it shrinks while the mouse moves fast, 0..=10.
    pub shrink: u8,
}

pub const DEFAULT_POINTER_COLOR: &str = "#ffffff";

impl Default for Pointer {
    fn default() -> Self {
        Self {
            enabled: false,
            shape: PointerShape::Circle,
            color: DEFAULT_POINTER_COLOR.into(),
            size: 5,
            opacity: 50,
            trail: 0,
            trail_distance: 5,
            blur: 0,
            glow: 0,
            shrink: 0,
        }
    }
}

/// `#rrggbb` (lower case) if `color` is one, so it can go into a stylesheet as is.
pub fn sanitize_color(color: &str) -> Option<String> {
    let hex = color.trim().strip_prefix('#')?;
    (hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit())).then(|| format!("#{}", hex.to_ascii_lowercase()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum WbwDisplay {
    /// Show word translation/transliteration on hover/tap.
    Tooltip,
    /// Show it under each word (default: like quran.com's word-by-word view).
    #[default]
    Inline,
}

/// Which King Fahd (KFGQPC) page fonts draw the 15-line Madani mushaf
/// (Uthman Taha's calligraphy). Both use the API's `code_v2` glyph codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MushafFont {
    /// QCF V4 tajweed-coloured fonts (COLRv1).
    #[default]
    TajweedV4,
    /// QCF V2 single-colour fonts.
    MadaniV2,
}

impl MushafFont {
    /// `mushaf=` id for the API, so `page_number`/`line_number` match the font's layout.
    pub fn mushaf_id(self) -> u32 {
        match self {
            MushafFont::TajweedV4 => quran_api::query::mushaf::QCF_TAJWEED_V4,
            MushafFont::MadaniV2 => quran_api::query::mushaf::QCF_V2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MushafFont::TajweedV4 => "Tajweed colours (QCF V4)",
            MushafFont::MadaniV2 => "Plain (QCF V2)",
        }
    }
}

/// Where the QCF fonts are fetched from, tried in order. Each must contain the
/// quran.com `fonts/quran/...` layout (`hafs/v4/colrv1/woff2/p1.woff2`, ...).
/// A user-set `Settings::font_base_url` is tried first.
pub const DEFAULT_FONT_BASES: &[&str] =
    &["https://verses.quran.foundation/fonts/quran", "https://quran.com/fonts/quran"];

/// Language of chapter names, word meanings and search (the app is English-only).
pub const LANGUAGE: &str = "en";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WordByWord {
    pub show_translation: bool,
    pub show_transliteration: bool,
    pub display: WbwDisplay,
}

impl Default for WordByWord {
    fn default() -> Self {
        Self { show_translation: true, show_transliteration: false, display: WbwDisplay::Inline }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Selected translation resource ids, in display order.
    pub translations: Vec<u32>,
    /// 1..=10 scale for the Arabic text.
    pub arabic_font_scale: u8,
    /// 1..=10 scale for translations.
    pub translation_font_scale: u8,
    pub word_by_word: WordByWord,
    /// Font set for the 15-line mushaf pages.
    pub mushaf_font: MushafFont,
    /// Show each page's verse translations under the page.
    pub show_page_translation: bool,
    /// Optional custom font base URL (e.g. a self-hosted copy); empty = defaults.
    pub font_base_url: String,
    /// Space between words on mushaf lines, 0..=10 (2 = default look).
    pub word_spacing: u8,
    /// Space under each mushaf line (below its word meanings, when shown), 0..=10 (4 = default look).
    pub line_spacing: u8,
    /// Space between a word and its meaning underneath, 0..=10 (4 = default look).
    pub meaning_spacing: u8,
    /// Extra width of the content column, 0..=10 steps (0 = default width).
    pub content_width: u8,
    pub theme: Theme,
    pub custom_theme: CustomTheme,
    pub pointer: Pointer,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            // Saheeh International (the API no longer serves 131, The Clear Quran).
            translations: vec![20],
            arabic_font_scale: 4,
            translation_font_scale: 3,
            word_by_word: WordByWord::default(),
            mushaf_font: MushafFont::TajweedV4,
            show_page_translation: false,
            font_base_url: String::new(),
            word_spacing: DEFAULT_WORD_SPACING,
            line_spacing: DEFAULT_LINE_SPACING,
            meaning_spacing: DEFAULT_MEANING_SPACING,
            content_width: 0,
            theme: Theme::default(),
            custom_theme: CustomTheme::default(),
            pointer: Pointer::default(),
        }
    }
}

impl Settings {
    /// Query to use for every `/verses/by_*` request.
    pub fn verse_query(&self) -> quran_api::VerseQuery {
        let strings = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        quran_api::VerseQuery {
            language: LANGUAGE.into(),
            translations: self.translations.clone(),
            words: true,
            fields: strings(&["text_uthmani", "qpc_uthmani_hafs"]),
            // The API has no per-word `qpc_uthmani_hafs`; the verse-level text is split instead.
            word_fields: strings(&["code_v2", "v2_page", "page_number", "line_number", "text_uthmani"]),
            translation_fields: strings(&["resource_name", "language_name"]),
            page: 1,
            per_page: 50,
            mushaf: Some(self.mushaf_font.mushaf_id()),
        }
    }

    /// Font base URLs to try, in order (custom first).
    pub fn font_bases(&self) -> Vec<String> {
        let custom = self.font_base_url.trim().trim_end_matches('/');
        let mut out: Vec<String> = Vec::new();
        if !custom.is_empty() {
            out.push(custom.to_string());
        }
        out.extend(DEFAULT_FONT_BASES.iter().map(|s| s.to_string()));
        out
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

/// Google Fonts stylesheet: Amiri Quran for Quran text outside the page fonts,
/// Noto Naskh Arabic for surah names.
const GOOGLE_FONTS_CSS: &str =
    "https://fonts.googleapis.com/css2?family=Amiri+Quran&family=Noto+Naskh+Arabic:wght@400;600&display=swap";

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

/// Arabic (mushaf glyph) size for a 1..=10 scale (clamped), linear between
/// 1 → 1.6rem, 4 (default) → 2.4rem and 10 → 4.4rem, rounded to 0.01rem.
pub fn arabic_font_size(scale: u8) -> String {
    let s = u32::from(scale.clamp(1, 10));
    // Hundredths of a rem; `(2x + y) / 2y` rounds x/y to the nearest integer.
    let centi = if s <= 4 { 160 + (2 * 80 * (s - 1) + 3) / 6 } else { 240 + (2 * 200 * (s - 4) + 6) / 12 };
    fmt_rem(centi * 10)
}

/// Translation text size for a 1..=10 scale (clamped). 3 (default) → 1.05rem, 1 → 0.8rem, 10 → 1.925rem.
pub fn translation_font_size(scale: u8) -> String {
    let s = u32::from(scale.clamp(1, 10));
    fmt_rem(800 + 125 * (s - 1))
}

impl Settings {
    /// Whether the theme is dark (`Some`), or follows the system (`None`).
    pub fn dark(&self) -> Option<bool> {
        match self.theme {
            Theme::Auto => None,
            Theme::Dark => Some(true),
            Theme::Light | Theme::Sepia => Some(false),
            Theme::Custom => Some(self.custom_theme.is_dark()),
        }
    }

    /// Overrides of the stylesheet, whose colours are `light-dark()` pairs.
    fn theme_css(&self) -> String {
        match self.theme {
            Theme::Auto => String::new(),
            Theme::Light => ":root { color-scheme: light; }".into(),
            Theme::Dark => ":root { color-scheme: dark; }".into(),
            Theme::Sepia => SEPIA_CSS.into(),
            Theme::Custom => self.custom_theme.css(),
        }
    }
}

/// CSS derived from settings: sizes, spacing and the pointer as custom
/// properties, and the theme's colours.
pub fn css_variables(settings: &Settings) -> String {
    let pointer = &settings.pointer;
    format!(
        ":root {{ --arabic-size: {}; --translation-size: {}; --mushaf-word-gap: {}; --mushaf-line-gap: {}; \
         --mushaf-meaning-gap: {}; --content-width: {}px; --pointer-color: {}; --pointer-size: {}; --pointer-opacity: {}; --pointer-blur: {}; --pointer-glow: {}; }}\n{}",
        arabic_font_size(settings.arabic_font_scale),
        translation_font_size(settings.translation_font_scale),
        word_gap(settings.word_spacing),
        line_gap(settings.line_spacing),
        meaning_gap(settings.meaning_spacing),
        content_width_px(settings.content_width),
        sanitize_color(&pointer.color).unwrap_or_else(|| DEFAULT_POINTER_COLOR.into()),
        f64::from(pointer.size.clamp(1, 10)) / 10.0,
        f64::from(pointer.opacity.clamp(5, 100)) / 100.0,
        f64::from(pointer.blur.min(10)) * 3.0 / 100.0,
        pointer.glow.min(10),
        settings.theme_css(),
    )
}

/// Width of the content column (header and pages) in CSS px: 0 → 960 (default),
/// then 160 more per step, up to 2560 at 10. The window still limits it.
pub fn content_width_px(step: u8) -> u32 {
    960 + 160 * u32::from(step.min(10))
}

pub const DEFAULT_WORD_SPACING: u8 = 2;
pub const DEFAULT_LINE_SPACING: u8 = 4;
pub const DEFAULT_MEANING_SPACING: u8 = 4;

/// Minimum gap between words on a mushaf line, in glyph `em`: 0 → 0em … 10 → 0.6em.
pub fn word_gap(step: u8) -> String {
    format!("{}em", f64::from(step.min(10)) * 6.0 / 100.0)
}

/// Space under a mushaf line, in glyph em (unitless): 0 → 0 … 4 → 0.4 (default) … 10 → 1.
pub fn line_gap(step: u8) -> String {
    format!("{}", f64::from(step.min(10)) / 10.0)
}

/// Space between a word and its meaning, in glyph em (unitless): 0 → -0.12 (tucked
/// under the word) … 4 → 0.12 (default) … 10 → 0.48.
pub fn meaning_gap(step: u8) -> String {
    format!("{}", (f64::from(step.min(10)) * 6.0 - 12.0) / 100.0)
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

/// Injects the web fonts for Arabic outside the mushaf pages and the CSS
/// variables derived from settings.
#[component]
pub fn FontLoader() -> Element {
    let settings = use_settings();
    let css = use_memo(move || css_variables(&settings.read()));
    rsx! {
        document::Link { rel: "preconnect", href: "https://fonts.googleapis.com" }
        document::Link { rel: "preconnect", href: "https://fonts.gstatic.com", crossorigin: "anonymous" }
        document::Link { rel: "stylesheet", href: GOOGLE_FONTS_CSS }
        // A plain <style> element (unlike `document::Style`) updates reactively.
        style { {css()} }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spacing_mapping() {
        assert_eq!(word_gap(0), "0em");
        assert_eq!(word_gap(DEFAULT_WORD_SPACING), "0.12em");
        assert_eq!(word_gap(10), "0.6em");
        assert_eq!(word_gap(99), "0.6em");
        assert_eq!(line_gap(DEFAULT_LINE_SPACING), "0.4");
        assert_eq!((line_gap(0), line_gap(10), line_gap(99)), ("0".into(), "1".into(), "1".into()));
        assert_eq!(meaning_gap(DEFAULT_MEANING_SPACING), "0.12");
        assert_eq!((meaning_gap(0), meaning_gap(2), meaning_gap(10)), ("-0.12".into(), "0".into(), "0.48".into()));
        let css = css_variables(&Settings::default());
        assert_eq!((content_width_px(0), content_width_px(3), content_width_px(99)), (960, 1440, 2560));
        for var in ["--mushaf-word-gap: 0.12em;", "--mushaf-line-gap: 0.4;", "--mushaf-meaning-gap: 0.12;", "--content-width: 960px;"] {
            assert!(css.contains(var), "{css}");
        }
    }

    #[test]
    fn theme_and_pointer_css() {
        let mut s = Settings::default();
        let css = css_variables(&s);
        for var in ["--pointer-color: #ffffff;", "--pointer-size: 0.5;", "--pointer-opacity: 0.5;", "--pointer-blur: 0;", "--pointer-glow: 0;"] {
            assert!(css.contains(var), "{css}");
        }
        assert!(!css.contains("color-scheme"), "Auto leaves the scheme to the system: {css}");
        s.theme = Theme::Dark;
        assert!(css_variables(&s).contains("color-scheme: dark;"));
        s.theme = Theme::Sepia;
        assert!(css_variables(&s).contains("color-scheme: light; --bg: #f4ecd8;"));
        // A colour that isn't one never reaches the stylesheet.
        s.pointer.color = "red; } body { display: none".into();
        s.pointer.blur = 10;
        let css = css_variables(&s);
        assert!(css.contains("--pointer-color: #ffffff;") && css.contains("--pointer-blur: 0.3;"), "{css}");
        assert!(!css.contains("display"), "{css}");
        assert_eq!(sanitize_color(" #AbCdEf "), Some("#abcdef".into()));
        assert_eq!((sanitize_color("#fff"), sanitize_color("abcdef"), sanitize_color("#12345g")), (None, None, None));
    }

    #[test]
    fn custom_theme_derives_its_shades() {
        let mut s = Settings { theme: Theme::Custom, ..Settings::default() };
        assert_eq!(s.dark(), Some(true), "the default custom background is dark");
        let css = css_variables(&s);
        for part in ["color-scheme: dark;", "--bg: #101a2b;", "--fg: color-mix(in oklab, #101a2b 10%, white);", "--accent: #2ca4ab;", "--on-accent: #fff;"] {
            assert!(css.contains(part), "{part} missing in {css}");
        }
        // A light background flips the text to dark; a light accent gets dark text on it.
        s.custom_theme = CustomTheme { background: "#F3E9D2".into(), accent: "#ffd54a".into() };
        assert_eq!(s.dark(), Some(false));
        let css = css_variables(&s);
        for part in ["color-scheme: light;", "--bg: #f3e9d2;", "10%, black);", "--on-accent: #111;", "--tip-bg: var(--fg);"] {
            assert!(css.contains(part), "{part} missing in {css}");
        }
        // Bad colours fall back to the defaults instead of reaching the stylesheet.
        s.custom_theme = CustomTheme { background: "url(x)".into(), accent: "} *{".into() };
        let css = css_variables(&s);
        assert!(css.contains("--bg: #101a2b;") && css.contains("--accent: #2ca4ab;") && !css.contains("url("), "{css}");
        assert_eq!((luminance("#000000"), luminance("#ffffff")), (Some(0.0), Some(1.0)));
        assert!(luminance("nope").is_none());
        let themes = |t| Settings { theme: t, ..Settings::default() }.dark();
        assert_eq!((themes(Theme::Auto), themes(Theme::Dark), themes(Theme::Sepia)), (None, Some(true), Some(false)));
    }

    #[test]
    fn font_scale_mapping() {
        assert_eq!(arabic_font_size(1), "1.6rem");
        assert_eq!(arabic_font_size(4), "2.4rem");
        assert_eq!(arabic_font_size(10), "4.4rem");
        assert_eq!(arabic_font_size(0), "1.6rem");
        assert_eq!(arabic_font_size(200), "4.4rem");
        assert_eq!(arabic_font_size(2), "1.87rem");
        assert_eq!(arabic_font_size(7), "3.4rem");
        assert_eq!(translation_font_size(3), "1.05rem");
        assert_eq!(translation_font_size(1), "0.8rem");
        assert_eq!(translation_font_size(2), "0.925rem");
        assert_eq!(translation_font_size(10), "1.925rem");
        let rem = |v: String| v.trim_end_matches("rem").parse::<f64>().unwrap();
        for s in 1..10u8 {
            // Strictly increasing, in even-ish steps (no jumps over 0.4rem).
            let (a, b) = (rem(arabic_font_size(s)), rem(arabic_font_size(s + 1)));
            assert!(a < b && b - a <= 0.4, "{s}: {a} -> {b}");
            assert!(rem(translation_font_size(s)) < rem(translation_font_size(s + 1)));
        }
    }

    #[test]
    fn css_vars_follow_settings() {
        let mut s = Settings::default();
        let css = css_variables(&s);
        assert!(css.contains("--arabic-size: 2.4rem;"), "{css}");
        assert!(css.contains("--translation-size: 1.05rem;"));
        s.arabic_font_scale = 10;
        assert!(css_variables(&s).contains("--arabic-size: 4.4rem;"));
    }

    #[test]
    fn custom_font_base_is_tried_first() {
        let mut s = Settings::default();
        assert_eq!(s.font_bases(), DEFAULT_FONT_BASES);
        s.font_base_url = " /fonts/quran/ ".into();
        assert_eq!(s.font_bases()[0], "/fonts/quran");
        assert_eq!(s.font_bases().len(), DEFAULT_FONT_BASES.len() + 1);
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
}
