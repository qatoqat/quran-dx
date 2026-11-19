//! Chapter reader: `/2`, `/2/255`, `/2/1-5`; legacy `/2:255`, `/2:1-5`,
//! `/2:1:5`, `/2-1-5` are redirected to the canonical URL.

use dioxus::prelude::*;
use quran_api::Chapter;

use super::feed::{VerseList, VerseSource};
use super::parse::{self, CHAPTER_COUNT, VerseRef, VerseSpan};
use super::{PrevNext, Redirect, SurahHeading, chapter_route, find_chapter, use_chapters, use_scroll_to_top};
use crate::Route;
use crate::layout::{ErrorBox, Loading};

#[component]
pub fn ChapterPage(chapter: String) -> Element {
    resolve(parse::parse_chapter_segment(&chapter), &chapter)
}

#[component]
pub fn ChapterRange(chapter: String, range: String) -> Element {
    resolve(parse::parse_chapter_and_range(&chapter, &range), &format!("{chapter}/{range}"))
}

/// Show the reference, or redirect when the URL isn't in canonical form.
fn resolve(parsed: Result<VerseRef, parse::ParseError>, path: &str) -> Element {
    match parsed {
        Err(e) => rsx! { InvalidReference { message: e.to_string() } },
        Ok(r) => {
            let canonical = r.canonical_path();
            if canonical != path {
                rsx! { Redirect { key: "{canonical}", to: chapter_route(r.chapter, r.span) } }
            } else {
                rsx! { ChapterView { key: "{canonical}", chapter: r.chapter, span: r.span } }
            }
        }
    }
}

#[component]
fn InvalidReference(message: String) -> Element {
    rsx! {
        section { class: "reader-error",
            ErrorBox { message }
            p { Link { to: Route::Home {}, "Browse all surahs" } }
        }
    }
}

#[component]
fn ChapterView(chapter: u16, span: Option<VerseSpan>) -> Element {
    use_scroll_to_top();
    let chapters = use_chapters();
    let list = match &*chapters.read() {
        None => return rsx! { Loading {} },
        Some(Err(e)) => {
            return rsx! { ErrorBox { message: "Could not load surah information: {e}" } };
        }
        Some(Ok(list)) => list.clone(),
    };
    let Some(info) = find_chapter(&list, chapter).cloned() else {
        return rsx! { InvalidReference { message: "Surah {chapter} was not found." } };
    };

    // Validate the span against the real verse count.
    let span = match span.map(|s| (s, s.clamp_to(info.verses_count))) {
        None => None,
        Some((_, Err(e))) => {
            return rsx! {
                section { class: "reader-error",
                    ErrorBox { message: "{e}" }
                    p { Link { to: chapter_route(chapter, None), "Read all of {info.name_simple}" } }
                }
            };
        }
        Some((orig, Ok(clamped))) if clamped != orig => {
            let to = chapter_route(chapter, Some(clamped));
            return rsx! { Redirect { key: "{clamped.segment()}", to } };
        }
        Some((_, Ok(s))) => Some(s),
    };

    let full = VerseSpan { from: 1, to: info.verses_count.max(1) };
    let source = VerseSource::Chapter { chapter, span };
    let source_key = format!("{chapter}-{}", span.map(|s| s.segment()).unwrap_or_default());
    let bismillah = info.bismillah_pre && span.is_none_or(|s| s.from == 1);
    let (prev, next, middle) = navigation(&info, &list, span);

    rsx! {
        section { class: "chapter-view",
            SurahHeading { chapter: info.clone(), bismillah, large: true }
            if let Some(s) = span {
                p { class: "range-info",
                    if s.is_single() {
                        "Verse {s.from} of {info.verses_count} · "
                    } else {
                        "Verses {s.from}–{s.to} of {info.verses_count} · "
                    }
                    Link { to: chapter_route(chapter, None), "Read the full surah" }
                }
            }
            VerseList { key: "{source_key}", source, expected: vec![(chapter, span.unwrap_or(full))] }
            PrevNext { prev, next, middle }
        }
    }
}

type NavLink = Option<(Route, String)>;

/// Prev/next links: neighbouring verses for a single verse, neighbouring
/// surahs otherwise; plus a "full surah" link for ranges.
fn navigation(info: &Chapter, list: &[Chapter], span: Option<VerseSpan>) -> (NavLink, NavLink, NavLink) {
    let id = info.id;
    let full = span.map(|_| (chapter_route(id, None), format!("All of {}", info.name_simple)));
    if let Some(s) = span.filter(VerseSpan::is_single) {
        let verse = |n: u16| VerseSpan::single(n).ok().map(|v| chapter_route(id, Some(v)));
        let prev =
            (s.from > 1).then(|| verse(s.from - 1)).flatten().map(|r| (r, format!("Verse {}", s.from - 1)));
        let next = (s.from < info.verses_count)
            .then(|| verse(s.from + 1))
            .flatten()
            .map(|r| (r, format!("Verse {}", s.from + 1)));
        return (prev, next, full);
    }
    let surah = |n: u16| {
        let name =
            find_chapter(list, n).map(|c| c.name_simple.clone()).unwrap_or_else(|| format!("Surah {n}"));
        (chapter_route(n, None), name)
    };
    let prev = (id > 1).then(|| surah(id - 1));
    let next = (id < CHAPTER_COUNT).then(|| surah(id + 1));
    (prev, next, full)
}
