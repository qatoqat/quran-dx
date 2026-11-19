//! The settings page: mushaf font, word by word, English translations, text
//! size and the font source. The app is English-only, so there is no language
//! or script picker.

use dioxus::prelude::*;
use quran_api::{Translation, TranslationResource};

use super::storage::sanitize_font_base_url;
use super::wbw::TranslationText;
use super::{MushafFont, Settings, WbwDisplay, use_settings};
use crate::layout::Loading;

/// `(id, name)` of English translations, used when `/resources/translations` fails.
pub const FALLBACK_TRANSLATIONS: &[(u32, &str)] = &[
    (131, "The Clear Quran (Dr. Mustafa Khattab)"),
    (20, "Saheeh International"),
    (85, "Abdul Haleem"),
    (84, "Mufti Taqi Usmani"),
    (19, "Pickthall"),
    (22, "Yusuf Ali"),
];

pub fn fallback_translations() -> Vec<TranslationResource> {
    FALLBACK_TRANSLATIONS
        .iter()
        .map(|(id, name)| TranslationResource {
            id: *id,
            name: (*name).into(),
            author_name: None,
            slug: None,
            language_name: "english".into(),
            translated_name: None,
        })
        .collect()
}

pub fn display_name(t: &TranslationResource) -> &str {
    t.translated_name.as_ref().map(|n| n.name.trim()).filter(|n| !n.is_empty()).unwrap_or(t.name.trim())
}

/// English translations only (by `language_name`, case-insensitive), without
/// duplicate ids, sorted by display name.
pub fn english_translations(all: &[TranslationResource]) -> Vec<TranslationResource> {
    let mut out: Vec<TranslationResource> =
        all.iter().filter(|t| t.language_name.trim().eq_ignore_ascii_case("english")).cloned().collect();
    out.sort_by_cached_key(|t| (display_name(t).to_lowercase(), t.id));
    let mut seen = std::collections::HashSet::new();
    out.retain(|t| seen.insert(t.id));
    out
}

/// Case-insensitive match on display name, name or author.
pub fn matches_filter(t: &TranslationResource, filter: &str) -> bool {
    let needle = filter.trim().to_lowercase();
    needle.is_empty()
        || [Some(t.name.as_str()), Some(display_name(t)), t.author_name.as_deref()]
            .into_iter()
            .flatten()
            .any(|s| s.to_lowercase().contains(&needle))
}

/// Toggle `id` in the ordered selection (appending when added).
pub fn toggle_id(list: &mut Vec<u32>, id: u32, on: bool) {
    if on {
        if !list.contains(&id) {
            list.push(id);
        }
    } else {
        list.retain(|x| *x != id);
    }
}

/// Label for a selected id: its name if known, else `Translation #id` (e.g. a
/// non-English id kept from an older version, which the user can remove).
pub fn selected_label(id: u32, known: &[TranslationResource]) -> String {
    known.iter().find(|t| t.id == id).map(|t| display_name(t).to_string()).unwrap_or_else(|| format!("Translation #{id}"))
}

/// Bismillah in QPC Hafs text (what the `UthmanicHafs` face expects), with
/// English word meanings and transliterations.
const PREVIEW_WORDS: &[(&str, &str, &str)] = &[
    ("بِسۡمِ", "In (the) name", "bis'mi"),
    ("ٱللَّهِ", "(of) Allah", "l-lahi"),
    ("ٱلرَّحۡمَٰنِ", "the Most Gracious", "l-raḥmāni"),
    ("ٱلرَّحِيمِ", "the Most Merciful", "l-raḥīmi"),
];
const PREVIEW_TRANSLATION: &str = "In the Name of Allah—the Most Compassionate, Most Merciful.";

fn preview_translation() -> Translation {
    Translation {
        id: None,
        resource_id: 0,
        text: PREVIEW_TRANSLATION.into(),
        resource_name: Some("Preview".into()),
        language_name: Some("english".into()),
    }
}

#[component]
pub fn SettingsPage() -> Element {
    let mut settings = use_settings();

    rsx! {
        document::Title { "Settings – The Noble Quran" }
        section { class: "settings-page",
            div { class: "settings-header",
                h1 { "Settings" }
                button {
                    r#type: "button",
                    class: "settings-reset",
                    onclick: move |_| settings.set(Settings::default()),
                    "Reset to defaults"
                }
            }
            p { class: "settings-note", "Changes apply immediately and are saved on this device." }
            MushafFontSection {}
            WordByWordSection {}
            TranslationsSection {}
            FontSizeSection {}
            AdvancedSection {}
        }
    }
}

#[component]
fn MushafFontSection() -> Element {
    let mut settings = use_settings();
    let current = settings.read().mushaf_font;
    let options = [
        (MushafFont::TajweedV4, "Colour-coded tajweed rules, like quran.com's default mushaf."),
        (MushafFont::MadaniV2, "Single-colour text in the same 15-line Madani layout."),
    ];
    rsx! {
        fieldset { class: "settings-card",
            legend { "Mushaf font" }
            p { class: "settings-help",
                "The 15-line Madani mushaf in the King Fahd Complex fonts (Uthman Taha's script)."
            }
            div { class: "choice-cards", role: "radiogroup", aria_label: "Mushaf font",
                for (font, desc) in options {
                    label {
                        key: "{font.label()}",
                        class: if font == current { "choice-card selected" } else { "choice-card" },
                        input {
                            r#type: "radio",
                            name: "mushaf-font",
                            checked: font == current,
                            onchange: move |_| settings.write().mushaf_font = font,
                        }
                        span { class: "choice-title", "{font.label()}" }
                        span { class: "choice-desc", "{desc}" }
                    }
                }
            }
        }
    }
}

#[component]
fn WordByWordSection() -> Element {
    let mut settings = use_settings();
    let wbw = settings.read().word_by_word.clone();
    let positions = [(WbwDisplay::Inline, "Under each word"), (WbwDisplay::Tooltip, "On hover or tap")];
    rsx! {
        fieldset { class: "settings-card",
            legend { "Word by word" }
            label { class: "settings-check",
                input {
                    r#type: "checkbox",
                    checked: wbw.show_translation,
                    onchange: move |e| settings.write().word_by_word.show_translation = e.checked(),
                }
                span { "Show English meaning" }
            }
            label { class: "settings-check",
                input {
                    r#type: "checkbox",
                    checked: wbw.show_transliteration,
                    onchange: move |e| settings.write().word_by_word.show_transliteration = e.checked(),
                }
                span { "Show transliteration" }
            }
            div { class: "settings-radios", role: "radiogroup", aria_label: "Position",
                span { class: "settings-radios-label", "Position" }
                for (mode, label) in positions {
                    label { key: "{label}", class: "settings-check",
                        input {
                            r#type: "radio",
                            name: "wbw-display",
                            checked: wbw.display == mode,
                            disabled: !wbw.show_translation && !wbw.show_transliteration,
                            onchange: move |_| settings.write().word_by_word.display = mode,
                        }
                        span { "{label}" }
                    }
                }
            }
        }
    }
}

#[component]
fn TranslationsSection() -> Element {
    let mut settings = use_settings();
    let client = crate::use_api();
    let list = use_resource(move || {
        let client = client.clone();
        async move { client.translations("en").await }
    });
    let mut filter = use_signal(String::new);
    let (show, selected) = {
        let s = settings.read();
        (s.show_page_translation, s.translations.clone())
    };

    let (available, status) = match &*list.read() {
        None => (None, None),
        Some(Ok(all)) => {
            let en = english_translations(all);
            if en.is_empty() {
                (Some(fallback_translations()), Some("No English translations were listed; showing common ones."))
            } else {
                (Some(en), None)
            }
        }
        Some(Err(_)) => {
            (Some(fallback_translations()), Some("Couldn’t load the translation list; showing common English translations."))
        }
    };
    let known = available.clone().unwrap_or_else(fallback_translations);
    // Chips show the display order, and let unknown (e.g. non-English) ids be removed.
    let has_unknown = selected.iter().any(|id| !known.iter().any(|t| t.id == *id));
    let chips: Vec<(u32, String)> = if selected.len() > 1 || has_unknown {
        selected.iter().map(|id| (*id, selected_label(*id, &known))).collect()
    } else {
        Vec::new()
    };

    rsx! {
        fieldset { class: "settings-card",
            legend { "Translation" }
            label { class: "settings-check",
                input {
                    r#type: "checkbox",
                    checked: show,
                    onchange: move |e| settings.write().show_page_translation = e.checked(),
                }
                span { "Show verse translations under each page" }
            }
            div { class: if show { "translation-picker" } else { "translation-picker is-off" },
                div { class: "translations-toolbar",
                    span { class: "settings-radios-label", "English translations" }
                    span { class: "settings-help", "{selected.len()} selected" }
                    if !selected.is_empty() {
                        button {
                            r#type: "button",
                            class: "link-button",
                            onclick: move |_| settings.write().translations.clear(),
                            "Clear all"
                        }
                    }
                }
                if !chips.is_empty() {
                    ul { class: "selected-chips", aria_label: "Selected translations",
                        for (id, name) in chips {
                            li { key: "{id}", class: "chip",
                                span { "{name}" }
                                button {
                                    r#type: "button",
                                    aria_label: "Remove {name}",
                                    onclick: move |_| toggle_id(&mut settings.write().translations, id, false),
                                    "×"
                                }
                            }
                        }
                    }
                }
                if let Some(msg) = status {
                    p { class: "settings-help", "{msg}" }
                }
                {
                    match available {
                        None => rsx! { Loading {} },
                        Some(items) => {
                            let shown: Vec<TranslationResource> =
                                items.iter().filter(|t| matches_filter(t, &filter.read())).cloned().collect();
                            rsx! {
                                if items.len() > 8 {
                                    input {
                                        r#type: "search",
                                        class: "translations-filter",
                                        placeholder: "Filter by name or author",
                                        aria_label: "Filter translations",
                                        value: "{filter}",
                                        oninput: move |e| filter.set(e.value()),
                                    }
                                }
                                if shown.is_empty() {
                                    p { class: "settings-help", "No translations match “{filter}”." }
                                }
                                div { class: "translation-list",
                                    for t in shown {
                                        label { key: "{t.id}", class: "settings-check translation-option",
                                            input {
                                                r#type: "checkbox",
                                                checked: selected.contains(&t.id),
                                                onchange: move |e| toggle_id(&mut settings.write().translations, t.id, e.checked()),
                                            }
                                            span { class: "translation-name", "{display_name(&t)}" }
                                            if let Some(author) = t.author_name.as_ref().filter(|a| !a.trim().is_empty() && a.trim() != display_name(&t)) {
                                                span { class: "translation-author", "{author}" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Tooltip for a preview word in hover mode (`None` when inline or nothing is shown).
fn hover_title(inline: bool, wbw: &super::WordByWord, meaning: &str, translit: &str) -> Option<String> {
    let parts: Vec<&str> = [(wbw.show_translation, meaning), (wbw.show_transliteration, translit)]
        .into_iter()
        .filter_map(|(on, text)| on.then_some(text))
        .collect();
    (!inline && !parts.is_empty()).then(|| parts.join(" · "))
}

#[component]
fn FontSizeSection() -> Element {
    let mut settings = use_settings();
    let (arabic, translation, wbw) = {
        let s = settings.read();
        (s.arabic_font_scale, s.translation_font_scale, s.word_by_word.clone())
    };
    let parse = |v: String| v.parse::<u8>().ok().map(|n| n.clamp(1, 10));
    let inline = wbw.display == WbwDisplay::Inline;

    rsx! {
        fieldset { class: "settings-card",
            legend { "Text size" }
            label { class: "settings-row", r#for: "arabic-size",
                span { "Arabic text" }
                input {
                    id: "arabic-size",
                    r#type: "range",
                    min: "1",
                    max: "10",
                    step: "1",
                    value: "{arabic}",
                    aria_valuetext: "{arabic} of 10",
                    oninput: move |e| if let Some(v) = parse(e.value()) { settings.write().arabic_font_scale = v },
                }
                output { class: "range-value", "{arabic}" }
            }
            label { class: "settings-row", r#for: "translation-size",
                span { "Translation text" }
                input {
                    id: "translation-size",
                    r#type: "range",
                    min: "1",
                    max: "10",
                    step: "1",
                    value: "{translation}",
                    aria_valuetext: "{translation} of 10",
                    oninput: move |e| if let Some(v) = parse(e.value()) { settings.write().translation_font_scale = v },
                }
                output { class: "range-value", "{translation}" }
            }
            div { class: "font-preview", aria_label: "Preview",
                p { class: "preview-arabic", dir: "rtl", lang: "ar",
                    for (i, (ar, meaning, translit)) in PREVIEW_WORDS.iter().enumerate() {
                        span {
                            key: "{i}",
                            class: "preview-word",
                            title: hover_title(inline, &wbw, meaning, translit),
                            span { class: "preview-word-ar", "{ar}" }
                            if inline && wbw.show_translation {
                                span { class: "preview-word-en", dir: "ltr", lang: "en", "{meaning}" }
                            }
                            if inline && wbw.show_transliteration {
                                span { class: "preview-word-tr", dir: "ltr", lang: "en", "{translit}" }
                            }
                        }
                    }
                }
                TranslationText { translation: preview_translation() }
                p { class: "settings-help preview-note",
                    "Preview uses a fallback font; the reader draws each page with its mushaf font."
                }
            }
        }
    }
}

#[component]
fn AdvancedSection() -> Element {
    let mut settings = use_settings();
    let committed = use_memo(move || settings.read().font_base_url.clone());
    let mut draft = use_signal(|| committed.peek().clone());
    // Follow outside changes (reset buttons, a sanitized value after applying).
    use_effect(move || draft.set(committed()));

    let draft_val = draft();
    let invalid = !draft_val.trim().is_empty() && sanitize_font_base_url(&draft_val).is_none();
    let mut apply = move || {
        let raw = draft.peek().clone();
        if raw.trim().is_empty() {
            settings.write().font_base_url.clear();
        } else if let Some(url) = sanitize_font_base_url(&raw) {
            if settings.peek().font_base_url != url {
                settings.write().font_base_url = url.clone();
            }
            draft.set(url);
        }
    };

    rsx! {
        fieldset { class: "settings-card",
            legend { "Advanced" }
            div { class: "settings-field",
                label { r#for: "font-base-url", "Font source URL" }
                div { class: "settings-field-row",
                    input {
                        id: "font-base-url",
                        r#type: "text",
                        inputmode: "url",
                        spellcheck: "false",
                        autocomplete: "off",
                        autocapitalize: "off",
                        placeholder: "https://static.qurancdn.com/fonts/quran",
                        class: if invalid { "invalid" } else { "" },
                        aria_invalid: if invalid { "true" } else { "false" },
                        aria_describedby: "font-base-url-help",
                        value: "{draft_val}",
                        oninput: move |e| draft.set(e.value()),
                        onchange: move |_| apply(),
                    }
                    button {
                        r#type: "button",
                        disabled: committed().is_empty() && draft_val.is_empty(),
                        onclick: move |_| {
                            draft.set(String::new());
                            settings.write().font_base_url.clear();
                        },
                        "Reset"
                    }
                }
            }
            p { id: "font-base-url-help", class: "settings-help",
                "Leave empty to use quran.com's font servers. Set this to a self-hosted copy of quran.com's fonts/quran folder if fonts don't load."
            }
            if invalid {
                p { class: "settings-error", role: "alert",
                    "Enter an http:// or https:// URL, or a path starting with /."
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tr(id: u32, name: &str, lang: &str, author: Option<&str>) -> TranslationResource {
        TranslationResource {
            id,
            name: name.into(),
            author_name: author.map(Into::into),
            slug: None,
            language_name: lang.into(),
            translated_name: None,
        }
    }

    fn sample() -> Vec<TranslationResource> {
        vec![
            tr(20, "Saheeh International", "english", Some("Saheeh International")),
            tr(131, "Clear Quran", "English", Some("Mustafa Khattab")),
            tr(97, "Maududi", "urdu", Some("Abul Ala Maududi")),
            tr(33, "Indonesian Ministry", "indonesian", None),
            tr(85, "Abdul Haleem", " ENGLISH ", Some("Abdul Haleem")),
            tr(20, "Saheeh International (dup)", "english", None),
        ]
    }

    #[test]
    fn only_english_sorted_and_deduped() {
        let ids: Vec<u32> = english_translations(&sample()).iter().map(|t| t.id).collect();
        assert_eq!(ids, [85, 131, 20]);
        assert!(english_translations(&[]).is_empty());
    }

    #[test]
    fn fallback_list_is_english() {
        let list = fallback_translations();
        assert_eq!(list.iter().map(|t| t.id).collect::<Vec<_>>(), [131, 20, 85, 84, 19, 22]);
        assert_eq!(english_translations(&list).len(), list.len());
    }

    #[test]
    fn filter_matches_name_and_author() {
        let en = english_translations(&sample());
        let ids = |f: &str| -> Vec<u32> { en.iter().filter(|t| matches_filter(t, f)).map(|t| t.id).collect() };
        assert_eq!(ids("khattab"), [131]);
        assert_eq!(ids("  SAHEEH "), [20]);
        assert_eq!(ids(""), [85, 131, 20]);
        assert!(ids("nothing-matches").is_empty());
    }

    #[test]
    fn selected_labels_fall_back_to_id() {
        let en = english_translations(&sample());
        assert_eq!(selected_label(131, &en), "Clear Quran");
        assert_eq!(selected_label(97, &en), "Translation #97");
    }

    #[test]
    fn hover_title_follows_settings() {
        let mut wbw = crate::settings::WordByWord::default();
        assert_eq!(hover_title(true, &wbw, "a", "b"), None);
        assert_eq!(hover_title(false, &wbw, "a", "b").as_deref(), Some("a"));
        wbw.show_transliteration = true;
        assert_eq!(hover_title(false, &wbw, "a", "b").as_deref(), Some("a · b"));
        wbw.show_translation = false;
        wbw.show_transliteration = false;
        assert_eq!(hover_title(false, &wbw, "a", "b"), None);
    }

    #[test]
    fn toggle_keeps_order_and_allows_empty() {
        let mut v = vec![131];
        toggle_id(&mut v, 20, true);
        toggle_id(&mut v, 20, true);
        assert_eq!(v, [131, 20]);
        toggle_id(&mut v, 131, false);
        toggle_id(&mut v, 20, false);
        assert!(v.is_empty());
    }
}
