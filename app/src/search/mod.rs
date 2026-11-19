//! Search: header search box with suggestions, results page.
//!
//! - [`nav`]: pure parser for navigation shortcuts (`2:255`, `page 3`, `kahf` ...).
//! - [`highlight`]: pure tokenizer turning API snippet HTML into safe text segments.
//! - [`SearchBox`]: header combobox with debounced suggestions.
//! - [`SearchPage`]: paginated results for `/search?q=`.

use std::rc::Rc;

use dioxus::prelude::*;
use quran_api::Chapter;

use crate::Route;

pub mod highlight;
pub mod nav;
mod results;
mod search_box;

pub use results::SearchPage;
pub use search_box::SearchBox;

/// Chapter list for name matching and labels; empty until loaded or on error
/// (numeric shortcuts still work without it).
fn use_chapter_list() -> Rc<Vec<Chapter>> {
    crate::reader::use_chapters().read().as_ref().and_then(|r| r.clone().ok()).unwrap_or_default()
}

/// Route for a single verse key like `"2:255"`. Unparseable keys fall back to the
/// legacy `/2:255` form, which `ChapterPage` understands.
pub fn verse_route(verse_key: &str) -> Route {
    match verse_key.split_once(':') {
        Some((c, v))
            if c.parse::<u16>().is_ok_and(|n| n > 0) && v.parse::<u16>().is_ok_and(|n| n > 0) =>
        {
            Route::verses(c, v)
        }
        _ => Route::surah(verse_key),
    }
}

/// User-facing message for a failed search request.
fn describe_error(e: &quran_api::Error) -> String {
    match e {
        quran_api::Error::Status { status, .. }
            if matches!(status, 401 | 403 | 404 | 410 | 501) =>
        {
            format!(
                "Search is unavailable right now: the Quran.com search endpoint returned HTTP {status}. \
                 It may have been retired or now require authentication."
            )
        }
        quran_api::Error::Status { status, .. } => {
            format!("Search failed: the server returned HTTP {status}. Please try again later.")
        }
        quran_api::Error::Decode(_) => "Search is unavailable right now: the search endpoint \
             returned a response this app does not understand."
            .to_string(),
        quran_api::Error::Http(_) => {
            "Search failed: could not reach the server. Check your connection and try again."
                .to_string()
        }
    }
}

/// Render API snippet HTML as plain text with `<mark>` around matches.
/// Never injects raw HTML (see [`highlight`]).
#[component]
fn Highlighted(html: String) -> Element {
    let segments = highlight::segments(&html);
    rsx! {
        for seg in segments {
            if seg.highlighted {
                mark { "{seg.text}" }
            } else {
                "{seg.text}"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verse_routes() {
        assert_eq!(
            verse_route("2:255"),
            Route::verses("2", "255")
        );
        assert_eq!(verse_route("2:x"), Route::surah("2:x"));
        assert_eq!(verse_route("0:1"), Route::surah("0:1"));
    }

    #[test]
    fn error_messages() {
        let e = quran_api::Error::Status { status: 403, body: String::new() };
        assert!(describe_error(&e).contains("require authentication"));
        let e = quran_api::Error::Status { status: 500, body: String::new() };
        assert!(describe_error(&e).contains("HTTP 500"));
        let e = quran_api::Error::Decode(serde_json::from_str::<u8>("x").unwrap_err());
        assert!(describe_error(&e).contains("does not understand"));
    }
}
