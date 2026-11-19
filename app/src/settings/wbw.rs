//! Word-by-word rendering and translation text with footnotes.
//! Used by `reader::verse::VerseView`.

use dioxus::prelude::*;
use quran_api::{Translation, Word};

/// One Arabic word, with word-by-word translation/transliteration per settings.
#[component]
pub fn WordView(word: Word) -> Element {
    // TODO(settings agent): tooltip / inline wbw, script selection, end markers.
    let text = word.text_uthmani.clone().or(word.text.clone()).unwrap_or_default();
    rsx! { span { class: "word", "{text} " } }
}

/// A verse translation; `text` may contain `<sup foot_note=ID>n</sup>` footnote markers.
#[component]
pub fn TranslationText(translation: Translation) -> Element {
    // TODO(settings agent): safe rendering + clickable footnotes.
    rsx! { p { class: "translation", "{translation.text}" } }
}
