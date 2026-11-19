//! Chapter reader: `/2`, `/2/255`, `/2/1-5`; legacy `/2:255`, `/2:1-5`,
//! `/2:1:5`, `/2-1-5` are redirected to the canonical URL.
//!
//! quran.com-style slugs (`/an-najm`, `/al-baqarah/255`, `/al-baqarah/1-5`) are
//! matched against the chapter list and redirected to the numeric URL (`/53`,
//! `/2/255`, ...), so every view has exactly one canonical address.

use dioxus::prelude::*;
use quran_api::Chapter;

use super::feed::{VerseList, VerseSource};
use super::mushaf_layout::Trim;
use super::parse::{self, CHAPTER_COUNT, VerseRef, VerseSpan};
use super::{PrevNext, Redirect, chapter_route, find_chapter, use_chapters, use_scroll_to_top};
use crate::Route;
use crate::layout::{ErrorBox, Loading};
use crate::route::Anchor;

#[component]
pub fn ChapterPage(chapter: String, at: Anchor) -> Element {
    let parsed = parse::parse_chapter_segment(&chapter);
    if parsed.is_err() && parse::looks_like_slug(&chapter) {
        return rsx! { SlugRedirect { key: "{chapter}", slug: chapter.clone(), range: None } };
    }
    resolve(parsed, &chapter, at)
}

#[component]
pub fn ChapterRange(chapter: String, range: String, at: Anchor) -> Element {
    if parse::parse_chapter_number(&chapter).is_err() && parse::looks_like_slug(&chapter) {
        let key = format!("{chapter}/{range}");
        return rsx! { SlugRedirect { key: "{key}", slug: chapter.clone(), range: Some(range.clone()) } };
    }
    resolve(parse::parse_chapter_and_range(&chapter, &range), &format!("{chapter}/{range}"), at)
}

/// `/an-najm[/range]` → `/53[/range]` once the chapter list is known.
#[component]
fn SlugRedirect(slug: String, range: Option<String>) -> Element {
    let chapters = use_chapters();
    let list = match &*chapters.read() {
        None => return rsx! { Loading {} },
        Some(Err(e)) => return rsx! { ErrorBox { message: "Could not load surah information: {e}" } },
        Some(Ok(list)) => list.clone(),
    };
    let names = list.iter().map(|c| (c.id, c.name_simple.as_str(), c.name_complex.as_str()));
    match parse::match_slug(&slug, names) {
        None => rsx! { InvalidReference { message: "No surah called “{slug}” was found." } },
        Some(id) => {
            let to = match range {
                None => Route::surah(id),
                Some(range) => Route::verses(id, range),
            };
            rsx! { Redirect { key: "{to}", to } }
        }
    }
}

/// Show the reference, or redirect when the URL isn't in canonical form.
fn resolve(parsed: Result<VerseRef, parse::ParseError>, path: &str, at: Anchor) -> Element {
    match parsed {
        Err(e) => rsx! { InvalidReference { message: e.to_string() } },
        Ok(r) => {
            let canonical = r.canonical_path();
            if canonical != path {
                rsx! { Redirect { key: "{canonical}", to: chapter_route(r.chapter, r.span).with_anchor(at) } }
            } else {
                rsx! { ChapterView { key: "{canonical}", chapter: r.chapter, span: r.span, at } }
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
fn ChapterView(chapter: u16, span: Option<VerseSpan>, at: Anchor) -> Element {
    use_scroll_to_top((chapter, span));
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
    // A range may start/end mid-line: don't stretch those lines edge to edge.
    let trim = Trim {
        start: span.is_some_and(|s| s.from > 1),
        end: span.is_some_and(|s| s.to < info.verses_count),
        page: None,
    };
    let (prev, next, middle) = navigation(&info, &list, span);

    rsx! {
        document::Title { "{info.name_simple} – The Noble Quran" }
        // The surah's name, place and verse count are on its banner inside the mushaf
        // page; a range that starts later has no banner, so it is named here.
        if let Some(s) = span {
            p { class: "range-info",
                if s.is_single() {
                    "{info.name_simple} · Verse {s.from} of {info.verses_count} · "
                } else {
                    "{info.name_simple} · Verses {s.from}–{s.to} of {info.verses_count} · "
                }
                Link { to: chapter_route(chapter, None), "Read the full surah" }
            }
        }
        VerseList { source, expected: vec![(chapter, span.unwrap_or(full))], trim, start: at }
        PrevNext { prev, next, middle }
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
        let name = find_chapter(list, n).map_or(n.to_string(), |c| c.name_simple.clone());
        (chapter_route(n, None), format!("Surah {name}"))
    };
    let prev = (id > 1).then(|| surah(id - 1));
    let next = (id < CHAPTER_COUNT).then(|| surah(id + 1));
    (prev, next, full)
}
