//! URL routes. Mirrors quran.com URLs so links keep working:
//! `/`, `/2`, `/2/255`, `/2/1-5`, `/2:255` (handled inside `ChapterPage`),
//! `/page/3`, `/juz/30`, `/search?q=...`, `/settings`.

use dioxus::prelude::*;

use crate::layout::{NotFound, Shell};
use crate::reader::{ChapterPage, ChapterRange, Home, JuzPage, MushafPage};
use crate::search::SearchPage;
use crate::settings::SettingsPage;

#[derive(Routable, Clone, PartialEq, Debug)]
#[rustfmt::skip]
pub enum Route {
    #[layout(Shell)]
        #[route("/")]
        Home {},
        /// Full-query segment so a search containing `&` isn't cut off (the router
        /// percent-decodes the whole query before splitting `?a=..&b=..` pairs).
        #[route("/search?:..q")]
        SearchPage { q: SearchQuery },
        #[route("/settings")]
        SettingsPage {},
        #[route("/page/:page")]
        MushafPage { page: u16 },
        #[route("/juz/:juz")]
        JuzPage { juz: u8 },
        /// `chapter` is a number ("2") or a legacy key form ("2:255", "2:1-5", "2-1-5").
        #[route("/:chapter")]
        ChapterPage { chapter: String },
        /// `range` is "255" or "1-5".
        #[route("/:chapter/:range")]
        ChapterRange { chapter: String, range: String },
        #[route("/:..segments")]
        NotFound { segments: Vec<String> },
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
    fn search_query_from_plain_url() {
        let parsed: Route = "/search?q=light%20%26%20dark".parse().unwrap();
        assert_eq!(parsed, Route::SearchPage { q: SearchQuery("light & dark".into()) });
    }
}
