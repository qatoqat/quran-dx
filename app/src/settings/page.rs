//! The settings page: language, translations, script, font sizes, word-by-word.

use dioxus::prelude::*;
use quran_api::{Language, Translation, TranslationResource, Word, WordText};

use super::wbw::{TranslationText, WordView};
use super::{QuranScript, Settings, WbwDisplay, use_settings};
use crate::layout::{ErrorBox, Loading};

/// `(iso_code, English name, native name)` used when `/resources/languages` fails.
const FALLBACK_LANGUAGES: &[(&str, &str, &str)] = &[
    ("en", "English", "English"),
    ("ar", "Arabic", "العربية"),
    ("bn", "Bengali", "বাংলা"),
    ("zh", "Chinese", "中文"),
    ("nl", "Dutch", "Nederlands"),
    ("fr", "French", "Français"),
    ("de", "German", "Deutsch"),
    ("hi", "Hindi", "हिन्दी"),
    ("id", "Indonesian", "Bahasa Indonesia"),
    ("it", "Italian", "Italiano"),
    ("ja", "Japanese", "日本語"),
    ("ko", "Korean", "한국어"),
    ("ms", "Malay", "Bahasa Melayu"),
    ("fa", "Persian", "فارسی"),
    ("pt", "Portuguese", "Português"),
    ("ru", "Russian", "Русский"),
    ("es", "Spanish", "Español"),
    ("sw", "Swahili", "Kiswahili"),
    ("ta", "Tamil", "தமிழ்"),
    ("tr", "Turkish", "Türkçe"),
    ("ur", "Urdu", "اردو"),
];

/// A language choice for the picker.
#[derive(Debug, Clone, PartialEq)]
pub struct LangOption {
    pub code: String,
    pub label: String,
}

fn fallback_languages() -> Vec<LangOption> {
    FALLBACK_LANGUAGES
        .iter()
        .map(|(code, name, native)| LangOption { code: (*code).into(), label: label_for(name, Some(native)) })
        .collect()
}

fn label_for(name: &str, native: Option<&str>) -> String {
    match native.map(str::trim).filter(|n| !n.is_empty() && !n.eq_ignore_ascii_case(name)) {
        Some(native) => format!("{native} ({name})"),
        None => name.to_string(),
    }
}

/// Picker options from the API list (or the fallback list), sorted by English
/// name, always including the current locale.
pub fn language_options(api: Option<&[Language]>, current: &str) -> Vec<LangOption> {
    let mut opts: Vec<(String, LangOption)> = match api.filter(|l| !l.is_empty()) {
        Some(langs) => langs
            .iter()
            .filter(|l| !l.iso_code.trim().is_empty())
            .map(|l| {
                let opt = LangOption { code: l.iso_code.clone(), label: label_for(&l.name, l.native_name.as_deref()) };
                (l.name.to_lowercase(), opt)
            })
            .collect(),
        None => FALLBACK_LANGUAGES
            .iter()
            .zip(fallback_languages())
            .map(|((_, name, _), opt)| (name.to_lowercase(), opt))
            .collect(),
    };
    opts.sort_by(|a, b| a.0.cmp(&b.0));
    opts.dedup_by(|a, b| a.1.code == b.1.code);
    let mut opts: Vec<LangOption> = opts.into_iter().map(|(_, o)| o).collect();
    if !opts.iter().any(|o| o.code == current) {
        opts.insert(0, LangOption { code: current.to_string(), label: current.to_string() });
    }
    opts
}

/// Translations of one language, for the grouped multi-select.
#[derive(Debug, Clone, PartialEq)]
pub struct TranslationGroup {
    pub language: String,
    pub items: Vec<TranslationResource>,
}

fn display_name(t: &TranslationResource) -> &str {
    t.translated_name.as_ref().map(|n| n.name.as_str()).filter(|n| !n.is_empty()).unwrap_or(&t.name)
}

/// Filter (case-insensitive on name, author and language) and group translations
/// by language. Groups with a selected translation come first, then the rest
/// alphabetically; inside a group, selected items come first.
pub fn arrange_translations(all: &[TranslationResource], selected: &[u32], filter: &str) -> Vec<TranslationGroup> {
    let needle = filter.trim().to_lowercase();
    let matches = |t: &TranslationResource| {
        needle.is_empty()
            || [Some(t.name.as_str()), Some(display_name(t)), t.author_name.as_deref(), Some(t.language_name.as_str())]
                .into_iter()
                .flatten()
                .any(|s| s.to_lowercase().contains(&needle))
    };
    let mut groups: Vec<TranslationGroup> = Vec::new();
    for t in all.iter().filter(|t| matches(t)) {
        let lang = capitalize(&t.language_name);
        match groups.iter_mut().find(|g| g.language == lang) {
            Some(g) => g.items.push(t.clone()),
            None => groups.push(TranslationGroup { language: lang, items: vec![t.clone()] }),
        }
    }
    let is_sel = |t: &TranslationResource| selected.contains(&t.id);
    for g in &mut groups {
        g.items.sort_by(|a, b| {
            is_sel(b).cmp(&is_sel(a)).then_with(|| display_name(a).to_lowercase().cmp(&display_name(b).to_lowercase()))
        });
    }
    groups.sort_by(|a, b| {
        let sa = a.items.iter().any(is_sel);
        let sb = b.items.iter().any(is_sel);
        sb.cmp(&sa).then_with(|| a.language.to_lowercase().cmp(&b.language.to_lowercase()))
    });
    groups
}

fn capitalize(s: &str) -> String {
    let s = s.trim();
    let mut chars = s.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => "Other".into(),
    }
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

const PREVIEW_UTHMANI: &str = "بِسْمِ ٱللَّهِ ٱلرَّحْمَـٰنِ ٱلرَّحِيمِ";
const PREVIEW_INDOPAK: &str = "بِسۡمِ اللّٰہِ الرَّحۡمٰنِ الرَّحِيۡمِ";
const PREVIEW_TRANSLATION: &str =
    "In the name of Allah, the Entirely Merciful, the Especially Merciful.";

fn preview_translation() -> Translation {
    Translation {
        id: None,
        resource_id: 0,
        text: PREVIEW_TRANSLATION.into(),
        resource_name: Some("Preview".into()),
        language_name: Some("english".into()),
    }
}

fn preview_word() -> Word {
    Word {
        id: 0,
        position: 1,
        char_type_name: "word".into(),
        text_uthmani: Some("بِسْمِ".into()),
        text_indopak: Some("بِسۡمِ".into()),
        text_imlaei: None,
        text: Some("بِسْمِ".into()),
        audio_url: None,
        page_number: None,
        line_number: None,
        translation: Some(WordText { text: Some("In (the) name".into()), language_name: Some("english".into()) }),
        transliteration: Some(WordText { text: Some("bis'mi".into()), language_name: Some("english".into()) }),
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
            LanguageSection {}
            ScriptSection {}
            FontSizeSection {}
            WordByWordSection {}
            TranslationsSection {}
        }
    }
}

#[component]
fn LanguageSection() -> Element {
    let mut settings = use_settings();
    let client = crate::use_api();
    let languages = use_resource(move || {
        let client = client.clone();
        async move { client.languages("en").await }
    });
    let current = settings.read().locale.clone();

    let (options, failed) = match &*languages.read() {
        Some(Ok(list)) => (language_options(Some(list), &current), false),
        Some(Err(_)) => (language_options(None, &current), true),
        None => (language_options(None, &current), false),
    };

    rsx! {
        fieldset { class: "settings-card",
            legend { "Language" }
            p { class: "settings-help",
                "Used for chapter names and word-by-word translations. Also lists that language's translations first."
            }
            label { class: "settings-row", r#for: "settings-locale",
                span { "Language" }
                select {
                    id: "settings-locale",
                    value: "{current}",
                    onchange: move |e| {
                        let code = e.value();
                        if !code.is_empty() {
                            settings.write().locale = code;
                        }
                    },
                    for opt in options {
                        option { key: "{opt.code}", value: "{opt.code}", selected: opt.code == current, "{opt.label}" }
                    }
                }
            }
            if failed {
                p { class: "settings-help", "Couldn’t load the full language list; showing common languages." }
            }
        }
    }
}

#[component]
fn ScriptSection() -> Element {
    let mut settings = use_settings();
    let current = settings.read().script;
    rsx! {
        fieldset { class: "settings-card",
            legend { "Quran script" }
            div { class: "script-options", role: "radiogroup",
                for (script, sample, font) in [
                    (QuranScript::Uthmani, PREVIEW_UTHMANI, QuranScript::Uthmani.font_family()),
                    (QuranScript::IndoPak, PREVIEW_INDOPAK, QuranScript::IndoPak.font_family()),
                ] {
                    label {
                        key: "{script.label()}",
                        class: if script == current { "script-option selected" } else { "script-option" },
                        input {
                            r#type: "radio",
                            name: "quran-script",
                            checked: script == current,
                            onchange: move |_| settings.write().script = script,
                        }
                        span { class: "script-name", "{script.label()}" }
                        span { class: "script-sample", dir: "rtl", lang: "ar", style: "font-family: {font}", "{sample}" }
                    }
                }
            }
        }
    }
}

#[component]
fn FontSizeSection() -> Element {
    let mut settings = use_settings();
    let (arabic, translation, script) = {
        let s = settings.read();
        (s.arabic_font_scale, s.translation_font_scale, s.script)
    };
    let sample = match script {
        QuranScript::Uthmani => PREVIEW_UTHMANI,
        QuranScript::IndoPak => PREVIEW_INDOPAK,
    };
    let parse = |v: String| v.parse::<u8>().ok().map(|n| n.clamp(1, 10));

    rsx! {
        fieldset { class: "settings-card",
            legend { "Font size" }
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
                p { class: "preview-arabic", dir: "rtl", lang: "ar", "{sample}" }
                TranslationText { translation: preview_translation() }
            }
        }
    }
}

#[component]
fn WordByWordSection() -> Element {
    let mut settings = use_settings();
    let wbw = settings.read().word_by_word.clone();
    rsx! {
        fieldset { class: "settings-card",
            legend { "Word by word" }
            label { class: "settings-check",
                input {
                    r#type: "checkbox",
                    checked: wbw.show_translation,
                    onchange: move |e| settings.write().word_by_word.show_translation = e.checked(),
                }
                span { "Show word translation" }
            }
            label { class: "settings-check",
                input {
                    r#type: "checkbox",
                    checked: wbw.show_transliteration,
                    onchange: move |e| settings.write().word_by_word.show_transliteration = e.checked(),
                }
                span { "Show word transliteration" }
            }
            div { class: "settings-radios", role: "radiogroup", aria_label: "Display",
                for (mode, label) in [(WbwDisplay::Tooltip, "As tooltip (hover, focus or tap)"), (WbwDisplay::Inline, "Under each word")] {
                    label { key: "{label}", class: "settings-check",
                        input {
                            r#type: "radio",
                            name: "wbw-display",
                            checked: wbw.display == mode,
                            onchange: move |_| settings.write().word_by_word.display = mode,
                        }
                        span { "{label}" }
                    }
                }
            }
            div { class: "wbw-preview verse-arabic", dir: "rtl", lang: "ar",
                span { class: "settings-help", dir: "ltr", "Preview: " }
                WordView { word: preview_word() }
            }
        }
    }
}

#[component]
fn TranslationsSection() -> Element {
    let mut settings = use_settings();
    let client = crate::use_api();
    let locale = use_memo(move || settings.read().locale.clone());
    let list = use_resource(move || {
        let client = client.clone();
        let locale = locale();
        async move { client.translations(&locale).await }
    });
    let mut filter = use_signal(String::new);
    let selected = settings.read().translations.clone();

    let body = match &*list.read() {
        None => rsx! { Loading {} },
        Some(Err(e)) => rsx! {
            ErrorBox { message: format!("Could not load translations: {e}") }
            if !selected.is_empty() {
                p { class: "settings-help", "Selected translation ids: {selected.len()}" }
            }
        },
        Some(Ok(all)) => {
            let groups = arrange_translations(all, &selected, &filter.read());
            let selected_items: Vec<TranslationResource> =
                selected.iter().filter_map(|id| all.iter().find(|t| t.id == *id).cloned()).collect();
            rsx! {
                if !selected_items.is_empty() {
                    ul { class: "selected-chips", aria_label: "Selected translations",
                        for t in selected_items {
                            li { key: "{t.id}", class: "chip",
                                span { "{display_name(&t)}" }
                                button {
                                    r#type: "button",
                                    aria_label: "Remove {display_name(&t)}",
                                    onclick: move |_| toggle_id(&mut settings.write().translations, t.id, false),
                                    "×"
                                }
                            }
                        }
                    }
                }
                if groups.is_empty() {
                    p { class: "settings-help", "No translations match “{filter}”." }
                }
                div { class: "translation-groups",
                    for g in groups {
                        div { key: "{g.language}", class: "translation-group",
                            h3 { "{g.language}" }
                            for t in g.items {
                                label { key: "{t.id}", class: "settings-check translation-option",
                                    input {
                                        r#type: "checkbox",
                                        checked: selected.contains(&t.id),
                                        onchange: move |e| toggle_id(&mut settings.write().translations, t.id, e.checked()),
                                    }
                                    span { class: "translation-name", "{display_name(&t)}" }
                                    if let Some(author) = t.author_name.as_ref().filter(|a| !a.is_empty() && a.as_str() != display_name(&t)) {
                                        span { class: "translation-author", "{author}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    };

    rsx! {
        fieldset { class: "settings-card",
            legend { "Translations" }
            div { class: "translations-toolbar",
                input {
                    r#type: "search",
                    class: "translations-filter",
                    placeholder: "Filter by name, author or language",
                    aria_label: "Filter translations",
                    value: "{filter}",
                    oninput: move |e| filter.set(e.value()),
                }
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
            {body}
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
            tr(1, "Saheeh International", "english", Some("Saheeh International")),
            tr(2, "Clear Quran", "english", Some("Mustafa Khattab")),
            tr(3, "Maududi", "urdu", Some("Abul Ala Maududi")),
            tr(4, "Indonesian Ministry", "indonesian", None),
            tr(5, "Jalandhari", "urdu", Some("Fateh Muhammad Jalandhari")),
        ]
    }

    #[test]
    fn grouping_selected_first() {
        let groups = arrange_translations(&sample(), &[5], "");
        let langs: Vec<_> = groups.iter().map(|g| g.language.as_str()).collect();
        assert_eq!(langs, ["Urdu", "English", "Indonesian"]);
        let urdu: Vec<u32> = groups[0].items.iter().map(|t| t.id).collect();
        assert_eq!(urdu, [5, 3]);
        let en: Vec<u32> = groups[1].items.iter().map(|t| t.id).collect();
        assert_eq!(en, [2, 1], "alphabetical when none selected");
    }

    #[test]
    fn grouping_without_selection_is_alphabetical() {
        let groups = arrange_translations(&sample(), &[], "");
        let langs: Vec<_> = groups.iter().map(|g| g.language.as_str()).collect();
        assert_eq!(langs, ["English", "Indonesian", "Urdu"]);
    }

    #[test]
    fn filter_matches_name_author_language() {
        let ids = |f: &str| -> Vec<u32> {
            arrange_translations(&sample(), &[], f).into_iter().flat_map(|g| g.items).map(|t| t.id).collect()
        };
        assert_eq!(ids("khattab"), [2]);
        assert_eq!(ids("URDU"), [5, 3]);
        assert_eq!(ids("  saheeh "), [1]);
        assert!(ids("nothing-matches").is_empty());
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

    #[test]
    fn language_options_fallback_and_current() {
        let opts = language_options(None, "en");
        assert!(opts.len() > 10);
        assert_eq!(opts[0].code, "ar", "sorted by English name");
        assert!(opts.iter().any(|o| o.code == "ur" && o.label == "اردو (Urdu)"));
        assert!(opts.iter().any(|o| o.code == "en" && o.label == "English"));
        // Unknown current locale is still selectable.
        let opts = language_options(None, "xx");
        assert_eq!(opts[0].code, "xx");
        // API list used when present; empty API list → fallback.
        let api = vec![Language {
            id: 1,
            name: "English".into(),
            iso_code: "en".into(),
            native_name: Some("English".into()),
            direction: Some("ltr".into()),
            translations_count: None,
            translated_name: None,
        }];
        assert_eq!(language_options(Some(&api), "en"), vec![LangOption { code: "en".into(), label: "English".into() }]);
        assert!(language_options(Some(&[]), "en").len() > 10);
    }
}
