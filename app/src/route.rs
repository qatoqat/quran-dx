//! URL routes. Mirrors quran.com URLs so links keep working:
//! `/`, `/2`, `/2/255`, `/2/1-5`, `/2:255` (handled inside `ChapterPage`),
//! `/page/3`, `/juz/30`, `/search?q=...`; plus the `/juz` list.
//!
//! The URL holds everything a reload needs: the path is the view (surah, juz or
//! page) and the hash of a reader URL is the reading position, e.g. `/2#2:142`.

// The `Routable` derive of dioxus 0.7.10 expands to a call this rustc flags as unreachable.
#![allow(unreachable_code)]

use dioxus::prelude::*;
use dioxus::router::routable::FromHashFragment;

use crate::layout::{NotFound, Shell};
use crate::reader::{ChapterPage, ChapterRange, Home, JuzIndex, JuzPage, MushafPage};
use crate::search::SearchPage;

#[derive(Routable, Clone, PartialEq, Debug)]
#[rustfmt::skip]
pub enum Route {
    #[layout(Shell)]
        #[route("/")]
        Home {},
        #[route("/juz")]
        JuzIndex {},
        // There is no list of pages: `/page` opens the first one.
        #[redirect("/page", || Route::page(1))]
        /// Full-query segment so a search containing `&` isn't cut off (the router
        /// percent-decodes the whole query before splitting `?a=..&b=..` pairs).
        #[route("/search?:..q")]
        SearchPage { q: SearchQuery },
        #[route("/page/:page#:at")]
        MushafPage { page: u16, at: Anchor },
        #[route("/juz/:juz#:at")]
        JuzPage { juz: u8, at: Anchor },
        /// `chapter` is a number ("2") or a legacy key form ("2:255", "2:1-5", "2-1-5").
        #[route("/:chapter#:at")]
        ChapterPage { chapter: String, at: Anchor },
        /// `range` is "255" or "1-5".
        #[route("/:chapter/:range#:at")]
        ChapterRange { chapter: String, range: String, at: Anchor },
        #[route("/:..segments")]
        NotFound { segments: Vec<String> },
}

/// How the Quran is divided for reading and browsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Surah,
    Juz,
    Page,
}

impl Route {
    pub fn page(page: u16) -> Self {
        Route::MushafPage { page, at: Anchor::default() }
    }

    pub fn juz(juz: u8) -> Self {
        Route::JuzPage { juz, at: Anchor::default() }
    }

    pub fn surah(chapter: impl ToString) -> Self {
        Route::ChapterPage { chapter: chapter.to_string(), at: Anchor::default() }
    }

    pub fn verses(chapter: impl ToString, range: impl ToString) -> Self {
        Route::ChapterRange { chapter: chapter.to_string(), range: range.to_string(), at: Anchor::default() }
    }

    /// The list of surahs or of ajzāʾ; pages have none.
    pub fn index(mode: Mode) -> Option<Self> {
        match mode {
            Mode::Surah => Some(Route::Home {}),
            Mode::Juz => Some(Route::JuzIndex {}),
            Mode::Page => None,
        }
    }

    /// The mode a view belongs to (`None` for search and unknown URLs).
    pub fn mode(&self) -> Option<Mode> {
        match self {
            Route::Home {} | Route::ChapterPage { .. } | Route::ChapterRange { .. } => Some(Mode::Surah),
            Route::JuzIndex {} | Route::JuzPage { .. } => Some(Mode::Juz),
            Route::MushafPage { .. } => Some(Mode::Page),
            Route::SearchPage { .. } | Route::NotFound { .. } => None,
        }
    }

    /// The same view at another reading position (views without one are unchanged).
    pub fn with_anchor(mut self, anchor: Anchor) -> Self {
        match &mut self {
            Route::MushafPage { at, .. }
            | Route::JuzPage { at, .. }
            | Route::ChapterPage { at, .. }
            | Route::ChapterRange { at, .. } => *at = anchor,
            _ => {}
        }
        self
    }
}

/// A word of the Quran: `2:142:1` is the first word of verse 2:142.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WordRef {
    pub chapter: u16,
    pub verse: u16,
    pub word: u16,
}

impl WordRef {
    /// `"2:142"` (the verse's first word) or `"2:142:7"`; `None` for anything else.
    pub fn parse(s: &str) -> Option<Self> {
        let mut parts = s.split(':').map(|n| n.parse::<u16>().ok().filter(|n| *n > 0));
        let (chapter, verse) = (parts.next()??, parts.next()??);
        let word = parts.next().unwrap_or(Some(1))?;
        parts.next().is_none().then_some(WordRef { chapter, verse, word })
    }

    pub fn verse_key(&self) -> String {
        format!("{}:{}", self.chapter, self.verse)
    }
}

impl std::fmt::Display for WordRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}", self.chapter, self.verse)?;
        if self.word > 1 {
            write!(f, ":{}", self.word)?;
        }
        Ok(())
    }
}

/// The reading position in a URL hash: the word that starts the line at the top
/// of the window. Empty at the top of a view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Anchor(pub Option<WordRef>);

impl FromHashFragment for Anchor {
    fn from_hash_fragment(hash: &str) -> Self {
        Anchor(WordRef::parse(hash))
    }
}

impl std::fmt::Display for Anchor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.0 {
            Some(word) => word.fmt(f),
            None => Ok(()),
        }
    }
}

/// The `q` of `/search?q=...`, taken as everything after `q=` so `&` is kept.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchQuery(pub String);

impl From<&str> for SearchQuery {
    fn from(query: &str) -> Self {
        // `query` arrives already percent-decoded.
        let q = query.strip_prefix("q=").unwrap_or(if query.contains('=') { "" } else { query });
        SearchQuery(q.to_string())
    }
}

impl std::fmt::Display for SearchQuery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `&`, `#`, `%` and `+` must be escaped here; the router encodes the rest.
        let escaped = self
            .0
            .replace('%', "%25")
            .replace('&', "%26")
            .replace('#', "%23")
            .replace('+', "%2B");
        write!(f, "q={escaped}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_route_round_trips_ampersand() {
        for q in ["light & dark", "mercy", "50% & more #1 a+b", "نور"] {
            let route = Route::SearchPage { q: SearchQuery(q.into()) };
            let url = route.to_string();
            let parsed: Route = url.parse().unwrap_or_else(|_| panic!("parse {url}"));
            assert_eq!(parsed, route, "url was {url}");
        }
    }

    #[test]
    fn reading_position_lives_in_the_hash() {
        let at = Anchor(Some(WordRef { chapter: 2, verse: 142, word: 1 }));
        for (url, route) in [
            ("/2#2:142", Route::surah(2).with_anchor(at)),
            ("/juz/2#2:142", Route::juz(2).with_anchor(at)),
            ("/page/22#2:142", Route::page(22).with_anchor(at)),
            ("/2/140-150#2:142", Route::verses(2, "140-150").with_anchor(at)),
            ("/2", Route::surah(2)),
            ("/juz", Route::JuzIndex {}),
        ] {
            assert_eq!(route.to_string(), url);
            assert_eq!(url.parse::<Route>().unwrap(), route, "{url}");
        }
        let mid = Anchor(Some(WordRef { chapter: 2, verse: 282, word: 45 }));
        assert_eq!(Route::surah(2).with_anchor(mid).to_string(), "/2#2:282:45");
        // A hash that isn't a position is ignored, never an error.
        for url in ["/2#nonsense", "/2#0:1", "/2#2:5:1:9", "/2#"] {
            assert_eq!(url.parse::<Route>().unwrap(), Route::surah(2), "{url}");
        }
        assert_eq!(Route::Home {}.with_anchor(at), Route::Home {});
    }

    #[test]
    fn views_know_their_mode() {
        assert_eq!(Route::surah(2).mode(), Some(Mode::Surah));
        assert_eq!(Route::juz(2).mode(), Some(Mode::Juz));
        assert_eq!(Route::page(3).mode(), Some(Mode::Page));
        assert_eq!("/page".parse::<Route>().unwrap(), Route::page(1), "/page opens the first page");
        assert_eq!(Route::SearchPage { q: SearchQuery("x".into()) }.mode(), None);
        assert_eq!((Route::index(Mode::Juz), Route::index(Mode::Page)), (Some(Route::JuzIndex {}), None));
    }

    #[test]
    fn search_query_from_plain_url() {
        let parsed: Route = "/search?q=light%20%26%20dark".parse().unwrap();
        assert_eq!(parsed, Route::SearchPage { q: SearchQuery("light & dark".into()) });
    }
}
