//! Mushaf page (`/page/1..=604`) and juz (`/juz/1..=30`) readers.

use dioxus::prelude::*;

use super::feed::{VerseList, VerseSource};
use super::parse::{JUZ_COUNT, MUSHAF_PAGES, valid_juz, valid_mushaf_page};
use super::{PrevNext, use_scroll_to_top};
use crate::Route;
use crate::layout::ErrorBox;

#[component]
pub fn MushafPage(page: u16) -> Element {
    if !valid_mushaf_page(page) {
        return rsx! {
            section { class: "reader-error",
                ErrorBox { message: "Page {page} does not exist; the mushaf has pages 1 to {MUSHAF_PAGES}." }
                p { Link { to: Route::MushafPage { page: 1 }, "Go to page 1" } }
            }
        };
    }
    rsx! { PageView { key: "page-{page}", page } }
}

#[component]
fn PageView(page: u16) -> Element {
    use_scroll_to_top();
    let prev = (page > 1).then(|| (Route::MushafPage { page: page - 1 }, format!("Page {}", page - 1)));
    let next =
        (page < MUSHAF_PAGES).then(|| (Route::MushafPage { page: page + 1 }, format!("Page {}", page + 1)));
    rsx! {
        section { class: "mushaf-view",
            header { class: "view-heading",
                h1 { "Page {page}" }
                p { class: "view-subtitle", "of {MUSHAF_PAGES} in the Madani mushaf" }
            }
            PrevNext { prev: prev.clone(), next: next.clone() }
            VerseList { source: VerseSource::Page(page) }
            PrevNext { prev, next }
        }
    }
}

#[component]
pub fn JuzPage(juz: u8) -> Element {
    if !valid_juz(juz) {
        return rsx! {
            section { class: "reader-error",
                ErrorBox { message: "Juz {juz} does not exist; there are {JUZ_COUNT} ajzāʾ." }
                p { Link { to: Route::JuzPage { juz: 1 }, "Go to juz 1" } }
            }
        };
    }
    rsx! { JuzView { key: "juz-{juz}", juz } }
}

#[component]
fn JuzView(juz: u8) -> Element {
    use_scroll_to_top();
    // The juz's verse ranges per surah, so a missing verse at either end of the juz
    // (where the page-to-page checks can't see it) is still reported.
    let client = crate::use_api();
    let juzs = use_resource(move || {
        let client = client.clone();
        async move { client.juzs().await.ok() }
    });
    let expected = use_memo(move || match &*juzs.read() {
        Some(Some(list)) => list
            .iter()
            .find(|j| j.juz_number == juz)
            .map(|j| super::parse::juz_mapping(&j.verse_mapping))
            .unwrap_or_default(),
        _ => Vec::new(),
    });
    let prev = (juz > 1).then(|| (Route::JuzPage { juz: juz - 1 }, format!("Juz {}", juz - 1)));
    let next = (juz < JUZ_COUNT).then(|| (Route::JuzPage { juz: juz + 1 }, format!("Juz {}", juz + 1)));
    rsx! {
        section { class: "juz-view",
            header { class: "view-heading",
                h1 { "Juz {juz}" }
                p { class: "view-subtitle", "of {JUZ_COUNT}" }
            }
            PrevNext { prev: prev.clone(), next: next.clone() }
            VerseList { source: VerseSource::Juz(juz), expected: expected() }
            PrevNext { prev, next }
        }
    }
}
