//! `/search?q=` results page.

use dioxus::prelude::*;
use quran_api::{Search, SearchResult};

use super::{Highlighted, describe_error, nav, use_chapter_list, verse_route};
use crate::Route;
use crate::settings::LANGUAGE;
use crate::layout::{ErrorBox, Loading};

const PER_PAGE: u32 = 20;

/// 1-based inclusive `(first, last)` result numbers shown on `page`.
fn shown_range(page: u32, per_page: u32, shown: usize, total: u32) -> (u32, u32) {
    if shown == 0 {
        return (0, 0);
    }
    let first = page.saturating_sub(1) * per_page + 1;
    let last = (first + shown as u32 - 1).min(total.max(first));
    (first, last)
}

fn plural(n: u32, one: &str, many: &str) -> String {
    if n == 1 { format!("{n} {one}") } else { format!("{n} {many}") }
}

#[component]
pub fn SearchPage(q: crate::route::SearchQuery) -> Element {
    let q = q.0;
    // Keyed by query so pagination state resets to page 1 whenever `q` changes.
    rsx! { SearchResults { key: "{q}", q: q.clone() } }
}

#[component]
fn SearchResults(q: String) -> Element {
    let client = crate::use_api();
    let chapter_list = use_chapter_list();
    let mut page = use_signal(|| 1u32);

    let query = q.trim().to_string();
    // The component is reused when the query changes (a new search from this
    // page), so the query must be a reactive input, and paging restarts at 1.
    let query_sig = use_memo(use_reactive!(|query| query));
    use_effect(move || {
        let _ = query_sig.read();
        page.set(1);
    });
    let results = use_resource({
        move || {
            let client = client.clone();
            let query = query_sig();
            let page = page();
            async move {
                if query.is_empty() {
                    return Ok(None);
                }
                client.search(&query, PER_PAGE, page, LANGUAGE).await.map(Some).map_err(|e| describe_error(&e))
            }
        }
    });

    let shortcuts = nav::parse(&query, &chapter_list);
    let pending = results.pending();
    let title = if query.is_empty() {
        "Search - The Noble Quran".to_string()
    } else {
        format!("{query} - Search - The Noble Quran")
    };

    let body = match &*results.read() {
        None => rsx! { Loading {} },
        Some(Err(msg)) => rsx! { ErrorBox { message: msg.clone() } },
        Some(Ok(None)) => rsx! {
            p { class: "search-empty", "Type a word or phrase in the search box above, or a verse key such as 2:255." }
        },
        Some(Ok(Some(search))) => rsx! {
            ResultsList { search: search.clone(), page: page(), pending, on_page: move |p: u32| {
                page.set(p);
                // Back to the top of the list for the new page.
                document::eval("window.scrollTo(0, 0)");
            } }
        },
    };

    rsx! {
        document::Title { "{title}" }
        section { class: "search-page",
            h1 { class: "search-title",
                if query.is_empty() { "Search" } else { "Search results for “{query}”" }
            }
            if !shortcuts.is_empty() {
                div { class: "search-jump",
                    h2 { "Jump to" }
                    ul {
                        for s in shortcuts {
                            li { key: "{s.target.badge()}",
                                Link { to: s.route(),
                                    span { class: "search-badge", "{s.target.badge()}" }
                                    span { "{s.label}" }
                                }
                            }
                        }
                    }
                }
            }
            {body}
        }
    }
}

#[component]
fn ResultsList(search: Search, page: u32, pending: bool, on_page: EventHandler<u32>) -> Element {
    if search.total_results == 0 || search.results.is_empty() {
        return rsx! {
            div { class: "search-empty",
                p { "No verses found for “{search.query}”." }
                p { class: "search-hint", "Try fewer or different words, check the spelling, or search in Arabic." }
            }
        };
    }
    let (first, last) = shown_range(page, PER_PAGE, search.results.len(), search.total_results);
    let total_pages = search.total_pages.max(page);
    let total = plural(search.total_results, "result", "results");
    let has_prev = page > 1;
    let has_next = page < total_pages;

    rsx! {
        p { class: "search-count", role: "status", "Showing {first}–{last} of {total}" }
        ol { class: if pending { "search-results pending" } else { "search-results" },
            start: "{first}",
            for r in search.results.iter().cloned() {
                ResultItem { key: "{r.verse_key}", result: r }
            }
        }
        if has_prev || has_next {
            nav { class: "search-pagination", aria_label: "Search result pages",
                button {
                    r#type: "button",
                    disabled: !has_prev || pending,
                    onclick: move |_| on_page.call(page - 1),
                    "← Previous"
                }
                span { class: "search-page-number", "Page {page} of {total_pages}" }
                button {
                    r#type: "button",
                    disabled: !has_next || pending,
                    onclick: move |_| on_page.call(page + 1),
                    "Next →"
                }
            }
        }
    }
}

#[component]
fn ResultItem(result: SearchResult) -> Element {
    let route: Route = verse_route(&result.verse_key);
    let arabic = result.highlighted.clone().unwrap_or_else(|| result.text.clone());
    rsx! {
        li { class: "search-result",
            Link { class: "search-badge", to: route, "{result.verse_key}" }
            p { class: "search-arabic", lang: "ar", dir: "rtl",
                Highlighted { html: arabic }
            }
            for (i, t) in result.translations.iter().cloned().enumerate() {
                div { key: "{i}", class: "search-translation",
                    p { dir: "auto", Highlighted { html: t.text } }
                    if let Some(name) = t.name {
                        p { class: "search-translation-name", "— {name}" }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges() {
        assert_eq!(shown_range(1, 20, 20, 57), (1, 20));
        assert_eq!(shown_range(3, 20, 17, 57), (41, 57));
        assert_eq!(shown_range(1, 20, 0, 0), (0, 0));
        // total smaller than what the page holds (inconsistent API data)
        assert_eq!(shown_range(1, 20, 5, 3), (1, 3));
    }

    #[test]
    fn plurals() {
        assert_eq!(plural(1, "result", "results"), "1 result");
        assert_eq!(plural(0, "result", "results"), "0 results");
    }
}
