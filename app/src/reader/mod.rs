//! Core reader: surah list, chapter + verse ranges, mushaf page, juz.
//!
//! - `parse`: pure URL parsing / pagination math (unit-tested).
//! - `home`: surah grid + juz list.
//! - `chapter`: `/2`, `/2/255`, `/2/1-5` (+ legacy `/2:255` redirects).
//! - `mushaf`: `/page/N` and `/juz/N`.
//! - `feed`: paged verse loading shared by all views ("Load more" + auto-load).
//! - `integrity`: checks that no verse or word goes missing.
//! - `verse`: one verse card (falls back to verse text when words are incomplete).

use dioxus::prelude::*;
use quran_api::Chapter;

use crate::Route;
use crate::settings::use_settings;

mod chapter;
mod feed;
mod home;
pub mod integrity;
mod mushaf;
pub mod parse;
pub mod verse;

pub use chapter::{ChapterPage, ChapterRange};
pub use home::Home;
pub use mushaf::{JuzPage, MushafPage};

/// Chapter list cache: `(locale, chapters)`. The list is small and static, so
/// every view shares one request per locale.
pub(crate) static CHAPTERS: GlobalSignal<Option<(String, Vec<Chapter>)>> = Signal::global(|| None);

/// All 114 chapters in the user's locale (cached). Errors are stringified.
pub(crate) fn use_chapters() -> Resource<Result<Vec<Chapter>, String>> {
    let client = crate::use_api();
    let settings = use_settings();
    let locale = use_memo(move || settings.read().locale.clone());
    use_resource(move || {
        let client = client.clone();
        let locale = locale();
        async move {
            if let Some((cached_locale, list)) = CHAPTERS.peek().as_ref()
                && *cached_locale == locale
            {
                return Ok(list.clone());
            }
            let list = client.chapters(&locale).await.map_err(|e| e.to_string())?;
            *CHAPTERS.write() = Some((locale, list.clone()));
            Ok(list)
        }
    })
}

/// Find a chapter by number in a chapter list.
pub(crate) fn find_chapter(list: &[Chapter], id: u16) -> Option<&Chapter> {
    list.iter().find(|c| c.id == id)
}

/// Replace the current URL with `to` (used for legacy / non-canonical URLs).
/// Render it with a `key` derived from the target so it re-runs on change.
#[component]
pub(crate) fn Redirect(to: Route) -> Element {
    use_effect(move || {
        navigator().replace(to.clone());
    });
    rsx! { crate::layout::Loading {} }
}

/// Scroll the window to the top once when the calling component mounts.
pub(crate) fn use_scroll_to_top() {
    use_hook(|| {
        document::eval("window.scrollTo(0, 0);");
    });
}

/// Route for a chapter, optionally narrowed to a span of verses.
pub(crate) fn chapter_route(chapter: u16, span: Option<parse::VerseSpan>) -> Route {
    match span {
        None => Route::ChapterPage { chapter: chapter.to_string() },
        Some(s) => Route::ChapterRange { chapter: chapter.to_string(), range: s.segment() },
    }
}

/// Previous / next navigation bar. `None` hides that side.
#[component]
pub(crate) fn PrevNext(
    prev: Option<(Route, String)>,
    next: Option<(Route, String)>,
    #[props(default)] middle: Option<(Route, String)>,
) -> Element {
    rsx! {
        nav { class: "reader-nav", "aria-label": "Reader navigation",
            div { class: "reader-nav-slot prev",
                if let Some((to, label)) = prev {
                    Link { class: "reader-nav-btn", to, rel: "prev", "← {label}" }
                }
            }
            div { class: "reader-nav-slot middle",
                if let Some((to, label)) = middle {
                    Link { class: "reader-nav-btn subtle", to, "{label}" }
                }
            }
            div { class: "reader-nav-slot next",
                if let Some((to, label)) = next {
                    Link { class: "reader-nav-btn", to, rel: "next", "{label} →" }
                }
            }
        }
    }
}

/// The Bismillah shown before every surah except Al-Fatihah (where it is
/// verse 1) and At-Tawbah.
pub(crate) const BISMILLAH: &str = "بِسْمِ ٱللَّهِ ٱلرَّحْمَٰنِ ٱلرَّحِيمِ";

/// Surah title block: names, revelation place, verse count, optional Bismillah.
/// `continued` marks a heading for a surah that started on an earlier page.
#[component]
pub(crate) fn SurahHeading(
    chapter: Chapter,
    #[props(default)] bismillah: bool,
    #[props(default)] continued: bool,
    #[props(default)] large: bool,
) -> Element {
    let place = parse::revelation_label(&chapter.revelation_place);
    let to = chapter_route(chapter.id, None);
    rsx! {
        header { class: if large { "surah-heading large" } else { "surah-heading" },
            div { class: "surah-heading-names",
                div { class: "surah-heading-latin",
                    if large {
                        h1 { "{chapter.id}. {chapter.name_simple}" }
                    } else {
                        h2 {
                            Link { to, "{chapter.id}. {chapter.name_simple}" }
                            if continued { span { class: "continued", " (continued)" } }
                        }
                    }
                    p { class: "surah-heading-translated", "{chapter.translated_name.name}" }
                }
                div { class: "surah-heading-arabic", dir: "rtl", lang: "ar", "{chapter.name_arabic}" }
            }
            p { class: "surah-heading-meta",
                if !place.is_empty() { span { "{place}" } }
                span { "{chapter.verses_count} verses" }
            }
            if bismillah {
                p { class: "bismillah", dir: "rtl", lang: "ar", "{BISMILLAH}" }
            }
        }
    }
}
