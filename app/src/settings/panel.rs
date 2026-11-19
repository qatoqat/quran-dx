//! The settings panel, a sidebar over the right edge of the window: theme,
//! mushaf font, word by word, English translations, text size, the reading
//! pointer and the font source.
//! It floats over the page, is mounted only while open, and the page behind it
//! is the live preview.

use dioxus::prelude::*;
use quran_api::TranslationResource;

use super::storage::sanitize_font_base_url;
use super::{MushafFont, PointerShape, Settings, Theme, WbwDisplay, sanitize_color, use_settings};
use crate::layout::Loading;

/// `(id, name)` of English translations, used when `/resources/translations` fails.
pub const FALLBACK_TRANSLATIONS: &[(u32, &str)] = &[
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

#[component]
pub fn SettingsPanel(onclose: EventHandler) -> Element {
    let mut settings = use_settings();

    rsx! {
        aside {
            id: "settings-panel",
            class: "settings-panel",
            role: "dialog",
            aria_label: "Settings",
            onkeydown: move |e| if e.key() == Key::Escape { onclose.call(()) },
            header {
                h2 { "Settings" }
                button {
                    r#type: "button",
                    class: "link-button",
                    onclick: move |_| settings.set(Settings::default()),
                    "Reset to defaults"
                }
                button {
                    r#type: "button",
                    class: "settings-close",
                    aria_label: "Close settings",
                    onmounted: move |e| async move {
                        let _ = e.set_focus(true).await;
                    },
                    onclick: move |_| onclose.call(()),
                    "×"
                }
            }
            p { class: "settings-help", "Changes apply immediately and are saved on this device." }
            ThemeSection {}
            MushafFontSection {}
            WordByWordSection {}
            TranslationsSection {}
            FontSizeSection {}
            PointerSection {}
            AdvancedSection {}
        }
    }
}

#[component]
fn ThemeSection() -> Element {
    let mut settings = use_settings();
    let (current, custom) = {
        let s = settings.read();
        (s.theme, s.custom_theme.clone())
    };
    rsx! {
        fieldset {
            legend { "Theme" }
            div { class: "segmented", role: "radiogroup", aria_label: "Theme",
                for theme in Theme::ALL {
                    label { key: "{theme.label()}",
                        input {
                            r#type: "radio",
                            name: "theme",
                            checked: theme == current,
                            onchange: move |_| settings.write().theme = theme,
                        }
                        span { "{theme.label()}" }
                    }
                }
            }
            if current == Theme::Custom {
                div { class: "settings-inline",
                    label { class: "settings-check",
                        input {
                            r#type: "color",
                            value: "{custom.background}",
                            oninput: move |e| if let Some(c) = sanitize_color(&e.value()) { settings.write().custom_theme.background = c },
                        }
                        span { "Background" }
                    }
                    label { class: "settings-check",
                        input {
                            r#type: "color",
                            value: "{custom.accent}",
                            oninput: move |e| if let Some(c) = sanitize_color(&e.value()) { settings.write().custom_theme.accent = c },
                        }
                        span { "Accent" }
                    }
                }
                p { class: "settings-help",
                    "Text, cards, borders and tooltips are shades worked out from these two colours; a dark background gets light text and the dark tajweed colours."
                }
            }
        }
    }
}

/// One slider of the pointer section.
#[component]
fn PointerRange(id: String, label: String, min: u8, max: u8, step: u8, value: u8, unit: String, onset: EventHandler<u8>) -> Element {
    rsx! {
        label { class: "settings-row", r#for: "{id}",
            span { "{label}" }
            input {
                id: "{id}",
                r#type: "range",
                min: "{min}",
                max: "{max}",
                step: "{step}",
                value: "{value}",
                oninput: move |e| if let Ok(v) = e.value().parse::<u8>() { onset.call(v.clamp(min, max)) },
            }
            output { class: "range-value", "{value}{unit}" }
        }
    }
}

#[component]
fn PointerSection() -> Element {
    let mut settings = use_settings();
    let pointer = settings.read().pointer.clone();
    let shapes = [(PointerShape::Circle, "Circle"), (PointerShape::Ring, "Ring")];
    rsx! {
        fieldset {
            legend { "Pointer" }
            label { class: "settings-check",
                input {
                    r#type: "checkbox",
                    checked: pointer.enabled,
                    onchange: move |e| settings.write().pointer.enabled = e.checked(),
                }
                span { "Show a reading pointer" }
            }
            p { class: "settings-help",
                "A dot or ring in place of the mouse cursor below the top bar. The cursor comes back over the top bar and these settings."
            }
            if pointer.enabled {
                // The colour swatch comes first: the browser opens its picker to the
                // right of it, and at the panel's right edge that would not fit the window.
                div { class: "settings-inline",
                    span { "Colour and shape" }
                    input {
                        r#type: "color",
                        aria_label: "Pointer colour",
                        value: "{pointer.color}",
                        oninput: move |e| if let Some(color) = sanitize_color(&e.value()) { settings.write().pointer.color = color },
                    }
                    div { class: "segmented", role: "radiogroup", aria_label: "Pointer shape",
                        for (shape , label) in shapes {
                            label { key: "{label}",
                                input {
                                    r#type: "radio",
                                    name: "pointer-shape",
                                    checked: shape == pointer.shape,
                                    onchange: move |_| settings.write().pointer.shape = shape,
                                }
                                span { "{label}" }
                            }
                        }
                    }
                }
                PointerRange {
                    id: "pointer-size", label: "Size (tenths of a line's height)", min: 1, max: 10, step: 1,
                    value: pointer.size, unit: "",
                    onset: move |v| settings.write().pointer.size = v,
                }
                PointerRange {
                    id: "pointer-opacity", label: "Opacity", min: 5, max: 100, step: 5,
                    value: pointer.opacity, unit: "%",
                    onset: move |v| settings.write().pointer.opacity = v,
                }
                PointerRange {
                    id: "pointer-trail", label: "Trail", min: 0, max: 10, step: 1,
                    value: pointer.trail, unit: "",
                    onset: move |v| settings.write().pointer.trail = v,
                }
                if pointer.trail > 0 {
                    PointerRange {
                        id: "pointer-trail-distance", label: "Trail distance", min: 1, max: 10, step: 1,
                        value: pointer.trail_distance, unit: "",
                        onset: move |v| settings.write().pointer.trail_distance = v,
                    }
                }
                PointerRange {
                    id: "pointer-blur", label: "Blur", min: 0, max: 10, step: 1,
                    value: pointer.blur, unit: "",
                    onset: move |v| settings.write().pointer.blur = v,
                }
                PointerRange {
                    id: "pointer-shrink", label: "Shrink with speed", min: 0, max: 10, step: 1,
                    value: pointer.shrink, unit: "",
                    onset: move |v| settings.write().pointer.shrink = v,
                }
                PointerRange {
                    id: "pointer-glow", label: "Glow (stronger colours under the pointer; blur softens its edge)", min: 0, max: 10, step: 1,
                    value: pointer.glow, unit: "",
                    onset: move |v| settings.write().pointer.glow = v,
                }
            }
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
        fieldset {
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
        fieldset {
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
        fieldset {
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

#[component]
fn FontSizeSection() -> Element {
    let mut settings = use_settings();
    let (arabic, translation, word_spacing, meaning_spacing, line_spacing, content_width) = {
        let s = settings.read();
        (s.arabic_font_scale, s.translation_font_scale, s.word_spacing, s.meaning_spacing, s.line_spacing, s.content_width)
    };
    let parse = |v: String| v.parse::<u8>().ok().map(|n| n.clamp(1, 10));
    let parse0 = |v: String| v.parse::<u8>().ok().map(|n| n.min(10));

    rsx! {
        fieldset {
            legend { "Text size and layout" }
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
            label { class: "settings-row", r#for: "word-spacing",
                span { "Space between words" }
                input {
                    id: "word-spacing",
                    r#type: "range",
                    min: "0",
                    max: "10",
                    step: "1",
                    value: "{word_spacing}",
                    aria_valuetext: "{word_spacing} of 10",
                    oninput: move |e| if let Some(v) = parse0(e.value()) { settings.write().word_spacing = v },
                }
                output { class: "range-value", "{word_spacing}" }
            }
            label { class: "settings-row", r#for: "meaning-spacing",
                span { "Space above word meanings" }
                input {
                    id: "meaning-spacing",
                    r#type: "range",
                    min: "0",
                    max: "10",
                    step: "1",
                    value: "{meaning_spacing}",
                    aria_valuetext: "{meaning_spacing} of 10",
                    oninput: move |e| if let Some(v) = parse0(e.value()) { settings.write().meaning_spacing = v },
                }
                output { class: "range-value", "{meaning_spacing}" }
            }
            label { class: "settings-row", r#for: "line-spacing",
                span { "Line spacing" }
                input {
                    id: "line-spacing",
                    r#type: "range",
                    min: "0",
                    max: "10",
                    step: "1",
                    value: "{line_spacing}",
                    aria_valuetext: "{line_spacing} of 10",
                    oninput: move |e| if let Some(v) = parse0(e.value()) { settings.write().line_spacing = v },
                }
                output { class: "range-value", "{line_spacing}" }
            }
            label { class: "settings-row", r#for: "content-width",
                span { "Content width · {super::content_width_px(content_width)} px" }
                input {
                    id: "content-width",
                    r#type: "range",
                    min: "0",
                    max: "10",
                    step: "1",
                    value: "{content_width}",
                    aria_valuetext: "{super::content_width_px(content_width)} pixels",
                    oninput: move |e| if let Some(v) = parse0(e.value()) { settings.write().content_width = v },
                }
                output { class: "range-value", "{content_width}" }
            }
            p { class: "settings-help",
                "Spacing applies to the mushaf pages. Line spacing is the space under each line, below the word meanings when they are shown. A page whose lines don't fit the window is scaled down to fit, never wrapped or cut."
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
        fieldset {
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
                        placeholder: super::DEFAULT_FONT_BASES[0],
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
        assert_eq!(list.iter().map(|t| t.id).collect::<Vec<_>>(), [20, 85, 84, 19, 22]);
        assert!(list.iter().any(|t| crate::settings::Settings::default().translations.contains(&t.id)));
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
