//! Word-by-word rendering and translation text with footnotes.
//! Used by `reader::verse::VerseView`.

use dioxus::prelude::*;
use quran_api::{Translation, Word};

use super::html::{Segment, display_segments};
use super::{QuranScript, WbwDisplay, is_rtl_language, use_settings};
use crate::layout::{ErrorBox, Loading};

/// Text of `word` for display: the selected script's field, then `text_uthmani`,
/// `text`, `text_indopak`, `text_imlaei`. `None` only if every field is empty.
pub fn word_text(word: &Word, script: QuranScript) -> Option<String> {
    let preferred = match script {
        QuranScript::Uthmani => &word.text_uthmani,
        QuranScript::IndoPak => &word.text_indopak,
    };
    [preferred, &word.text_uthmani, &word.text, &word.text_indopak, &word.text_imlaei]
        .into_iter()
        .filter_map(|t| t.as_deref())
        .map(str::trim)
        .find(|t| !t.is_empty())
        .map(String::from)
}

/// Placeholder shown when a word has no text in any field (never render nothing).
pub const MISSING_WORD: &str = "⟨?⟩";

/// Convert Arabic-Indic / Extended Arabic-Indic digits to ASCII (for labels).
pub fn ascii_digits(s: &str) -> String {
    s.chars()
        .filter_map(|c| match c {
            '0'..='9' => Some(c),
            '\u{660}'..='\u{669}' => char::from_u32(c as u32 - 0x660 + '0' as u32),
            '\u{6F0}'..='\u{6F9}' => char::from_u32(c as u32 - 0x6F0 + '0' as u32),
            _ => None,
        })
        .collect()
}

/// `255` -> `٢٥٥`
fn arabic_indic(n: u16) -> String {
    n.to_string().chars().map(|c| c.to_digit(10).and_then(|d| char::from_u32(0x0660 + d)).unwrap_or(c)).collect()
}

fn non_empty(t: &Option<quran_api::WordText>) -> Option<String> {
    t.as_ref()?.text.as_deref().map(str::trim).filter(|s| !s.is_empty()).map(String::from)
}

/// One Arabic word, with word-by-word translation/transliteration per settings.
#[component]
pub fn WordView(
    word: Word,
    /// Used for the ayah-number marker if the API's end-marker word has no text.
    #[props(default)]
    verse_number: Option<u16>,
) -> Element {
    let settings = use_settings();
    let (script, wbw, locale) = {
        let s = settings.read();
        (s.script, s.word_by_word.clone(), s.locale.clone())
    };
    let mut open = use_signal(|| false);
    let position = word.position;

    if word.is_end_marker() {
        let text = word_text(&word, script)
            .or_else(|| verse_number.map(arabic_indic))
            .unwrap_or_default();
        let number = ascii_digits(&text);
        return rsx! {
            span { class: "ayah-marker", role: "img", aria_label: "end of ayah {number}",
                span { class: "ayah-marker-num", "{text}" }
            }
            " "
        };
    }

    let Some(text) = word_text(&word, script) else {
        return rsx! {
            span {
                class: "word word-missing",
                "data-position": "{position}",
                title: "word text unavailable",
                "{MISSING_WORD}"
            }
            " "
        };
    };

    // Word-by-word extras never replace the Arabic word; missing data just omits a line.
    let translation = if wbw.show_translation { non_empty(&word.translation) } else { None };
    let transliteration =
        if wbw.show_transliteration { non_empty(&word.transliteration) } else { None };
    let has_extra = translation.is_some() || transliteration.is_some();

    if !has_extra {
        return rsx! {
            span { class: "word", "data-position": "{position}", "{text}" }
            " "
        };
    }

    let extra_lines = rsx! {
        if let Some(tr) = transliteration.clone() {
            span { class: "wbw-transliteration", lang: "en", dir: "ltr", "{tr}" }
        }
        if let Some(tr) = translation.clone() {
            span { class: "wbw-translation", lang: "{locale}", dir: "auto", "{tr}" }
        }
    };

    match wbw.display {
        WbwDisplay::Inline => rsx! {
            span { class: "word word-inline", "data-position": "{position}",
                span { class: "word-text", "{text}" }
                {extra_lines}
            }
            " "
        },
        WbwDisplay::Tooltip => {
            let tip_id = format!("wbw-tip-{}", word.id);
            let class = if open() { "word has-tooltip is-open" } else { "word has-tooltip" };
            rsx! {
                span {
                    class,
                    "data-position": "{position}",
                    tabindex: 0,
                    aria_describedby: "{tip_id}",
                    // Tap toggles (touch devices may not focus on tap); hover/focus are CSS.
                    onclick: move |_| open.toggle(),
                    onblur: move |_| open.set(false),
                    onkeydown: move |e: KeyboardEvent| {
                        match e.key() {
                            Key::Escape => open.set(false),
                            Key::Enter => open.toggle(),
                            _ => {}
                        }
                    },
                    span { class: "word-text", "{text}" }
                    span { class: "word-tooltip", id: "{tip_id}", role: "tooltip", {extra_lines} }
                }
                " "
            }
        }
    }
}

/// A verse translation; `text` may contain `<sup foot_note=ID>n</sup>` footnote markers.
/// API HTML is tokenized into plain text nodes and never injected as markup.
#[component]
pub fn TranslationText(translation: Translation) -> Element {
    let mut open_note: Signal<Option<u32>> = use_signal(|| None);
    let segments = display_segments(&translation.text);
    let language = translation.language_name.clone().unwrap_or_default();
    let dir = if is_rtl_language(&language) { "rtl" } else { "ltr" };
    let uid = translation.id.unwrap_or(translation.resource_id);

    rsx! {
        div { class: "translation", dir, "data-resource": "{translation.resource_id}",
            p { class: "translation-text", {render_segments(&segments, Some((uid, open_note)))} }
            if let Some(name) = translation.resource_name.as_ref().filter(|n| !n.is_empty()) {
                p { class: "translation-source", "— {name}" }
            }
            if let Some(id) = open_note() {
                FootNoteBox {
                    key: "{id}",
                    id,
                    dom_id: format!("footnote-{uid}-{id}"),
                    rtl: dir == "rtl",
                    on_close: move |_| open_note.set(None),
                }
            }
        }
    }
}

/// Render tokenized segments. When `footnotes` is given, markers become toggle buttons.
fn render_segments(segments: &[Segment], footnotes: Option<(u32, Signal<Option<u32>>)>) -> Element {
    rsx! {
        {segments.iter().cloned().enumerate().map(|(i, seg)| render_segment(i, seg, footnotes))}
    }
}

fn render_segment(i: usize, seg: Segment, footnotes: Option<(u32, Signal<Option<u32>>)>) -> Element {
    match seg {
        Segment::Text { text, style } => match (style.italic, style.bold) {
            (true, true) => rsx! { em { key: "{i}", strong { "{text}" } } },
            (true, false) => rsx! { em { key: "{i}", "{text}" } },
            (false, true) => rsx! { strong { key: "{i}", "{text}" } },
            (false, false) => rsx! { span { key: "{i}", "{text}" } },
        },
        Segment::LineBreak => rsx! { br { key: "{i}" } },
        Segment::FootNote { id, label } => {
            let label = if label.is_empty() { "*".to_string() } else { label };
            let Some((uid, mut open)) = footnotes else {
                return rsx! { sup { key: "{i}", "{label}" } };
            };
            let expanded = open() == Some(id);
            rsx! {
                sup { key: "{i}", class: "footnote-ref",
                    button {
                        r#type: "button",
                        class: if expanded { "footnote-btn active" } else { "footnote-btn" },
                        aria_expanded: "{expanded}",
                        aria_controls: "footnote-{uid}-{id}",
                        aria_label: "Footnote {label}",
                        onclick: move |_| open.set(if expanded { None } else { Some(id) }),
                        "{label}"
                    }
                }
            }
        }
    }
}

#[component]
fn FootNoteBox(id: u32, dom_id: String, rtl: bool, on_close: EventHandler<()>) -> Element {
    let client = crate::use_api();
    let note = use_resource(move || {
        let client = client.clone();
        async move { client.footnote(id).await }
    });

    let body = match &*note.read() {
        None => rsx! { Loading {} },
        Some(Err(e)) => rsx! { ErrorBox { message: format!("Could not load footnote: {e}") } },
        Some(Ok(n)) => {
            let segs = display_segments(&n.text);
            let lang_rtl = n.language_name.as_deref().map(is_rtl_language).unwrap_or(rtl);
            rsx! {
                div { class: "footnote-text", dir: if lang_rtl { "rtl" } else { "ltr" },
                    {render_segments(&segs, None)}
                }
            }
        }
    };

    rsx! {
        aside { class: "footnote-box", id: "{dom_id}", role: "note", aria_label: "Footnote",
            div { class: "footnote-head",
                span { class: "footnote-title", "Footnote" }
                button {
                    r#type: "button",
                    class: "footnote-close",
                    aria_label: "Close footnote",
                    onclick: move |_| on_close.call(()),
                    "×"
                }
            }
            {body}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(uthmani: Option<&str>, indopak: Option<&str>, text: Option<&str>) -> Word {
        Word {
            id: 1,
            position: 1,
            char_type_name: "word".into(),
            text_uthmani: uthmani.map(Into::into),
            text_indopak: indopak.map(Into::into),
            text_imlaei: None,
            text: text.map(Into::into),
            audio_url: None,
            page_number: None,
            line_number: None,
            translation: None,
            transliteration: None,
        }
    }

    #[test]
    fn script_selection_with_fallback() {
        let w = word(Some("U"), Some("I"), Some("T"));
        assert_eq!(word_text(&w, QuranScript::Uthmani).as_deref(), Some("U"));
        assert_eq!(word_text(&w, QuranScript::IndoPak).as_deref(), Some("I"));
        // IndoPak missing → uthmani.
        let w = word(Some("U"), None, Some("T"));
        assert_eq!(word_text(&w, QuranScript::IndoPak).as_deref(), Some("U"));
        // Selected + uthmani missing/blank → text.
        let w = word(Some("  "), Some(""), Some("T"));
        assert_eq!(word_text(&w, QuranScript::IndoPak).as_deref(), Some("T"));
        // Only indopak present while Uthmani selected.
        let w = word(None, Some("I"), None);
        assert_eq!(word_text(&w, QuranScript::Uthmani).as_deref(), Some("I"));
        // Only imlaei.
        let mut w = word(None, None, None);
        w.text_imlaei = Some("M".into());
        assert_eq!(word_text(&w, QuranScript::Uthmani).as_deref(), Some("M"));
        // Nothing at all → None (caller renders the visible placeholder).
        let w = word(Some(" "), None, Some(""));
        assert_eq!(word_text(&w, QuranScript::Uthmani), None);
        assert!(!MISSING_WORD.is_empty());
    }

    #[test]
    fn digits() {
        assert_eq!(ascii_digits("\u{662}\u{665}\u{665}"), "255");
        assert_eq!(ascii_digits("\u{6F7}"), "7");
        assert_eq!(ascii_digits("12 "), "12");
    }
}
