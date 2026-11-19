//! Mushaf page (`/page/1..=604`) and juz (`/juz/1..=30`) readers.

use dioxus::prelude::*;

use super::feed::{VerseList, VerseSource};
use super::parse::{JUZ_COUNT, MUSHAF_PAGES, valid_juz, valid_mushaf_page};
use super::{PrevNext, reference, use_juzs, use_scroll_to_top};
use crate::Route;
use crate::layout::ErrorBox;
use crate::route::Anchor;

#[component]
pub fn MushafPage(page: u16, at: Anchor) -> Element {
    if !valid_mushaf_page(page) {
        return rsx! {
            section { class: "reader-error",
                ErrorBox { message: "Page {page} does not exist; the mushaf has pages 1 to {MUSHAF_PAGES}." }
                p { Link { to: Route::page(1), "Go to page 1" } }
            }
        };
    }
    rsx! { PageView { key: "page-{page}", page, at } }
}

#[component]
fn PageView(page: u16, at: Anchor) -> Element {
    use_scroll_to_top(page);
    let prev = (page > 1).then(|| (Route::page(page - 1), format!("Page {}", page - 1)));
    let next =
        (page < MUSHAF_PAGES).then(|| (Route::page(page + 1), format!("Page {}", page + 1)));
    rsx! {
        document::Title { "Page {page} – The Noble Quran" }
        PrevNext { prev: prev.clone(), next: next.clone() }
        // Every verse that begins on the page must arrive, including its first and last.
        VerseList { source: VerseSource::Page(page), expected: reference::verses_on_page(page), start: at }
        PrevNext { prev, next }
    }
}

#[component]
pub fn JuzPage(juz: u8, at: Anchor) -> Element {
    if !valid_juz(juz) {
        return rsx! {
            section { class: "reader-error",
                ErrorBox { message: "Juz {juz} does not exist; there are {JUZ_COUNT} ajzāʾ." }
                p { Link { to: Route::juz(1), "Go to juz 1" } }
            }
        };
    }
    rsx! { JuzView { key: "juz-{juz}", juz, at } }
}

#[component]
fn JuzView(juz: u8, at: Anchor) -> Element {
    use_scroll_to_top(juz);
    // The juz's verse ranges per surah, so a missing verse at either end of the juz
    // (where the page-to-page checks can't see it) is still reported.
    let juzs = use_juzs();
    // `juz` via use_reactive: the view is reused when navigating juz → juz.
    let expected = use_memo(use_reactive!(|juz| match &*juzs.read() {
        Some(Ok(list)) => list
            .iter()
            .find(|j| j.juz_number == juz)
            .map(|j| super::parse::juz_mapping(&j.verse_mapping))
            .unwrap_or_default(),
        _ => Vec::new(),
    }));
    let prev = (juz > 1).then(|| (Route::juz(juz - 1), format!("Juz {}", juz - 1)));
    let next = (juz < JUZ_COUNT).then(|| (Route::juz(juz + 1), format!("Juz {}", juz + 1)));
    rsx! {
        document::Title { "Juz {juz} – The Noble Quran" }
        PrevNext { prev: prev.clone(), next: next.clone() }
        VerseList { source: VerseSource::Juz(juz), expected: expected(), start: at }
        PrevNext { prev, next }
    }
}
