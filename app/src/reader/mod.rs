//! Core reader: surah list, chapter + verse ranges, mushaf page, juz.
//!
//! - `parse`: pure URL parsing / pagination math (unit-tested).
//! - `home`: the surah and juz lists.
//! - `chapter`: `/2`, `/2/255`, `/2/1-5` (+ legacy `/2:255` redirects).
//! - `mushaf`: `/page/N` and `/juz/N`.
//! - `feed`: paged verse loading shared by all views ("Load more" + auto-load).
//! - `integrity`: checks that no verse or word goes missing (incl. mushaf layout data).
//! - `mushaf_layout`: pure grouping of words into 15-line mushaf pages and lines.
//! - `mushaf_view`: renders those pages (QCF glyphs, word-by-word, headers, Bismillah).
//! - `glyph`: glyph vs Unicode-text choice per word (never nothing).
//! - `fonts`: per-page QCF font loading, and the surah banner fonts.
//! - `render_audit`: shrink-to-fit of mushaf lines and the on-screen word audit.
//! - `reference`: compiled-in verse, word and page counts the loaded data is checked against.
//! - `position`: the reading position (top line), kept in the URL and restored from it.
//! - `verse`: per-verse copy action and plain-text helpers.

use std::rc::Rc;

use dioxus::prelude::*;
use quran_api::{Chapter, Juz};

use crate::Route;
use crate::settings::LANGUAGE;

mod chapter;
mod feed;
pub mod fonts;
pub mod glyph;
mod home;
pub mod integrity;
mod mushaf;
pub mod mushaf_layout;
pub(crate) mod mushaf_view;
pub mod parse;
pub(crate) mod position;
pub mod reference;
pub mod render_audit;
pub mod verse;

pub use chapter::{ChapterPage, ChapterRange};
pub use home::{Home, JuzIndex};
pub use mushaf::{JuzPage, MushafPage};

/// Chapter list cache. The list is small and static, so every view shares one request.
pub(crate) static CHAPTERS: GlobalSignal<Option<Rc<Vec<Chapter>>>> = Signal::global(|| None);

/// All 114 chapters (cached). Errors are stringified.
pub(crate) fn use_chapters() -> Resource<Result<Rc<Vec<Chapter>>, String>> {
    let client = crate::use_api();
    use_resource(move || {
        let client = client.clone();
        async move {
            if let Some(list) = CHAPTERS.peek().as_ref() {
                return Ok(list.clone());
            }
            let list = Rc::new(client.chapters(LANGUAGE).await.map_err(|e| e.to_string())?);
            *CHAPTERS.write() = Some(list.clone());
            Ok(list)
        }
    })
}

/// Juz list cache (each juz's verse ranges), shared like the chapter list.
static JUZS: GlobalSignal<Option<Rc<Vec<Juz>>>> = Signal::global(|| None);

/// All 30 ajzāʾ in order (cached). Errors are stringified.
pub(crate) fn use_juzs() -> Resource<Result<Rc<Vec<Juz>>, String>> {
    let client = crate::use_api();
    use_resource(move || {
        let client = client.clone();
        async move {
            if let Some(list) = JUZS.peek().as_ref() {
                return Ok(list.clone());
            }
            // The API has been known to return each juz more than once; keep the first.
            let mut list = client.juzs().await.map_err(|e| e.to_string())?;
            list.sort_by_key(|j| j.juz_number);
            list.dedup_by_key(|j| j.juz_number);
            let list = Rc::new(list);
            *JUZS.write() = Some(list.clone());
            Ok(list)
        }
    })
}

/// The juz that contains a verse.
pub(crate) fn juz_of(juzs: &[Juz], chapter: u16, verse: u16) -> Option<u8> {
    juzs.iter()
        .find(|j| parse::juz_mapping(&j.verse_mapping).iter().any(|(c, span)| *c == chapter && span.contains(verse)))
        .map(|j| j.juz_number)
}

/// Find a chapter by number in a chapter list.
pub(crate) fn find_chapter(list: &[Chapter], id: u16) -> Option<&Chapter> {
    list.iter().find(|c| c.id == id)
}

/// Replace the current URL with `to` (used for legacy / non-canonical URLs).
/// Render it with a `key` derived from the target so it re-runs on change.
#[component]
pub(crate) fn Redirect(to: Route) -> Element {
    // Reactive on `to`: a reused Redirect with a new target must navigate again.
    use_effect(use_reactive!(|to| {
        navigator().replace(to);
    }));
    rsx! { crate::layout::Loading {} }
}

/// Scroll the window to the top when the calling view mounts and whenever `key`
/// changes (views are reused when navigating page → page, surah → surah). A view
/// opened at a reading position scrolls there once its verses are loaded.
pub(crate) fn use_scroll_to_top<K: PartialEq + Clone + 'static>(key: K) {
    use_effect(use_reactive!(|key| {
        let _ = key;
        document::eval("window.scrollTo(0, 0);");
    }));
}

/// Route for a chapter, optionally narrowed to a span of verses.
pub(crate) fn chapter_route(chapter: u16, span: Option<parse::VerseSpan>) -> Route {
    match span {
        None => Route::surah(chapter),
        Some(s) => Route::verses(chapter, s.segment()),
    }
}

/// Previous / next navigation bar. `None` leaves that side empty.
///
/// The Quran reads right to left, so "next" is on the left and "previous" on
/// the right, each with an arrow pointing the way it leads.
#[component]
pub(crate) fn PrevNext(
    prev: Option<(Route, String)>,
    next: Option<(Route, String)>,
    #[props(default)] middle: Option<(Route, String)>,
) -> Element {
    rsx! {
        nav { class: "reader-nav", "aria-label": "Reader navigation",
            if let Some((to, label)) = prev {
                Link { class: "prev", to, rel: "prev", "Previous: {label} →" }
            }
            if let Some((to, label)) = middle {
                Link { class: "middle", to, "{label}" }
            }
            if let Some((to, label)) = next {
                Link { class: "next", to, rel: "next", "← Next: {label}" }
            }
        }
    }
}
