//! Core reader: surah list, chapter + verse ranges, mushaf page, juz.

use dioxus::prelude::*;

pub mod verse;

#[component]
pub fn Home() -> Element {
    rsx! { h1 { "Surahs" } } // TODO(reader agent)
}

#[component]
pub fn ChapterPage(chapter: String) -> Element {
    rsx! { h1 { "Chapter {chapter}" } } // TODO(reader agent)
}

#[component]
pub fn ChapterRange(chapter: String, range: String) -> Element {
    rsx! { h1 { "Chapter {chapter}, {range}" } } // TODO(reader agent)
}

#[component]
pub fn MushafPage(page: u16) -> Element {
    rsx! { h1 { "Page {page}" } } // TODO(reader agent)
}

#[component]
pub fn JuzPage(juz: u8) -> Element {
    rsx! { h1 { "Juz {juz}" } } // TODO(reader agent)
}
