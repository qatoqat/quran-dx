//! Rendering of a single verse (Arabic words + translations).

use dioxus::prelude::*;
use quran_api::Verse;

use crate::settings::wbw::{TranslationText, WordView};

#[component]
pub fn VerseView(verse: Verse) -> Element {
    rsx! {
        article { class: "verse", id: "verse-{verse.verse_key}",
            div { class: "verse-arabic", dir: "rtl", lang: "ar",
                for w in verse.words.iter().cloned() {
                    WordView { key: "{w.id}", word: w }
                }
            }
            for t in verse.translations.iter().cloned() {
                TranslationText { translation: t }
            }
        }
    }
}
