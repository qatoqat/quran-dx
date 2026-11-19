//! Header search box: an ARIA combobox with navigation shortcuts and
//! debounced verse suggestions.

use std::cell::Cell;
use std::rc::Rc;

use dioxus::prelude::*;
use quran_api::SearchResult;

use super::nav::{self, NavSuggestion};
use super::{Highlighted, describe_error, use_chapters, verse_route};
use crate::Route;

const DEBOUNCE_MS: u32 = 300;
/// Delay before closing on blur, so clicks on options land first.
const BLUR_CLOSE_MS: u32 = 200;
const SUGGESTION_COUNT: u32 = 5;
const LISTBOX_ID: &str = "search-suggestions";

#[derive(Debug, Clone, PartialEq)]
enum VerseState {
    Idle,
    Loading,
    Ready(Vec<SearchResult>),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
enum Item {
    Nav(NavSuggestion),
    Verse(SearchResult),
    /// "Search for …" — opens the full results page.
    All,
}

impl Item {
    fn route(&self, query: &str) -> Route {
        match self {
            Item::Nav(s) => s.route(),
            Item::Verse(r) => verse_route(&r.verse_key),
            Item::All => Route::SearchPage { q: crate::route::SearchQuery(query.trim().to_string()) },
        }
    }
}

fn option_id(i: usize) -> String {
    format!("search-opt-{i}")
}

/// Next highlighted index for ArrowDown (`down`) / ArrowUp, wrapping around.
fn step(active: Option<usize>, count: usize, down: bool) -> Option<usize> {
    if count == 0 {
        return None;
    }
    Some(match (active, down) {
        (None, true) => 0,
        (None, false) => count - 1,
        (Some(i), true) => (i + 1) % count,
        (Some(i), false) => (i + count - 1) % count,
    })
}

/// Only hit the API for queries that contain letters (Latin or Arabic);
/// `2:255` or `18` are pure navigation.
fn wants_verse_search(q: &str) -> bool {
    q.chars().any(char::is_alphabetic)
}

#[component]
pub fn SearchBox() -> Element {
    let client = crate::use_api();
    let settings = crate::settings::use_settings();
    let chapters = use_chapters();
    let navigator = use_navigator();

    let mut query = use_signal(String::new);
    let mut open = use_signal(|| false);
    let mut active = use_signal(|| None::<usize>);
    let mut verses = use_signal(|| VerseState::Idle);
    // Non-reactive counters: request generation (drops stale responses) and
    // focus generation (cancels a pending blur-close when focus returns).
    let generation = use_hook(|| Rc::new(Cell::new(0u64)));
    let focus_gen = use_hook(|| Rc::new(Cell::new(0u64)));

    // Mirror the query of the results page into the box (e.g. after a reload of
    // `/search?q=...` or Back/Forward between searches).
    let route_q = match use_route::<Route>() {
        Route::SearchPage { q } => Some(q.0),
        _ => None,
    };
    use_effect(use_reactive!(|route_q| {
        if let Some(q) = route_q {
            query.set(q);
        }
    }));

    let q = query();
    let chapter_list = chapters.read().clone().unwrap_or_default();
    let mut items: Vec<Item> = nav::parse(&q, &chapter_list).into_iter().map(Item::Nav).collect();
    if let VerseState::Ready(results) = &*verses.read() {
        items.extend(results.iter().cloned().map(Item::Verse));
    }
    if !q.trim().is_empty() {
        items.push(Item::All);
    }
    let count = items.len();
    let expanded = open() && !q.trim().is_empty();
    let active_idx = if expanded { active().filter(|i| *i < count) } else { None };

    // Navigate somewhere and close the dropdown.
    let mut go = move |route: Route| {
        open.set(false);
        active.set(None);
        navigator.push(route);
    };

    let oninput = {
        let generation = generation.clone();
        move |evt: FormEvent| {
            let value = evt.value();
            query.set(value.clone());
            open.set(true);
            active.set(None);
            let my = generation.get() + 1;
            generation.set(my);
            if !wants_verse_search(&value) {
                verses.set(VerseState::Idle);
                return;
            }
            let client = client.clone();
            let generation = generation.clone();
            let locale = settings.read().locale.clone();
            spawn(async move {
                crate::util::sleep_ms(DEBOUNCE_MS).await;
                if generation.get() != my {
                    return;
                }
                verses.set(VerseState::Loading);
                let res = client.search(value.trim(), SUGGESTION_COUNT, 1, &locale).await;
                if generation.get() != my {
                    return; // a newer query superseded this one
                }
                verses.set(match res {
                    Ok(search) => VerseState::Ready(search.results),
                    Err(e) => VerseState::Failed(describe_error(&e)),
                });
            });
        }
    };

    let onkeydown = {
        let items = items.clone();
        let q = q.clone();
        move |evt: KeyboardEvent| match evt.key() {
            Key::ArrowDown | Key::ArrowUp => {
                evt.prevent_default();
                if q.trim().is_empty() {
                    return;
                }
                let down = evt.key() == Key::ArrowDown;
                let current = if open() { active() } else { None };
                open.set(true);
                active.set(step(current, count, down));
            }
            Key::Enter => {
                evt.prevent_default();
                let selected = active_idx.and_then(|i| items.get(i));
                match selected {
                    Some(item) => go(item.route(&q)),
                    None if !q.trim().is_empty() => go(Item::All.route(&q)),
                    None => {}
                }
            }
            Key::Escape if open() => {
                evt.prevent_default();
                open.set(false);
                active.set(None);
            }
            _ => {}
        }
    };

    let onfocusin = {
        let focus_gen = focus_gen.clone();
        move |_| {
            focus_gen.set(focus_gen.get() + 1);
            open.set(true);
        }
    };
    let onfocusout = {
        let focus_gen = focus_gen.clone();
        move |_| {
            let my = focus_gen.get() + 1;
            focus_gen.set(my);
            let focus_gen = focus_gen.clone();
            spawn(async move {
                crate::util::sleep_ms(BLUR_CLOSE_MS).await;
                if focus_gen.get() == my {
                    open.set(false);
                    active.set(None);
                }
            });
        }
    };

    let verse_state = verses.read().clone();
    let nav_count = items.iter().filter(|i| matches!(i, Item::Nav(_))).count();

    rsx! {
        div { class: "search-box", role: "search", onfocusin, onfocusout,
            input {
                class: "search-input",
                r#type: "search",
                placeholder: "Search, or jump to 2:255, page 3, Al-Kahf…",
                autocomplete: "off",
                spellcheck: "false",
                role: "combobox",
                aria_label: "Search the Quran",
                aria_autocomplete: "list",
                aria_controls: LISTBOX_ID,
                aria_expanded: if expanded { "true" } else { "false" },
                aria_activedescendant: active_idx.map(option_id),
                value: "{q}",
                oninput,
                onkeydown,
            }
            if expanded {
                div {
                    class: "search-dropdown",
                    // Keep focus in the input so the click on an option lands
                    // before the blur handler closes the dropdown.
                    onmousedown: move |evt| evt.prevent_default(),
                    ul { id: LISTBOX_ID, class: "search-options", role: "listbox", aria_label: "Suggestions",
                        for (i, item) in items.iter().cloned().enumerate() {
                            if i == 0 && nav_count > 0 {
                                li { class: "search-group", role: "presentation", "Jump to" }
                            }
                            if i == nav_count && nav_count < count - 1 {
                                li { class: "search-group", role: "presentation", "Verses" }
                            }
                            li {
                                id: option_id(i),
                                role: "option",
                                class: if active_idx == Some(i) { "search-option active" } else { "search-option" },
                                aria_selected: if active_idx == Some(i) { "true" } else { "false" },
                                onmouseenter: move |_| active.set(Some(i)),
                                onclick: {
                                    let route = item.route(&q);
                                    move |_| go(route.clone())
                                },
                                OptionBody { item: item.clone(), query: q.clone() }
                            }
                        }
                    }
                    {match verse_state {
                        VerseState::Loading => rsx! {
                            div { class: "search-status", role: "status", "Searching verses…" }
                        },
                        VerseState::Failed(msg) => rsx! {
                            div { class: "search-status search-status-error", role: "status", "{msg}" }
                        },
                        VerseState::Ready(r) if r.is_empty() => rsx! {
                            div { class: "search-status", role: "status", "No matching verses." }
                        },
                        _ => rsx! {},
                    }}
                }
            }
        }
    }
}

#[component]
fn OptionBody(item: Item, query: String) -> Element {
    match item {
        Item::Nav(s) => rsx! {
            span { class: "search-badge", "{s.target.badge()}" }
            span { class: "search-option-text", "{s.label}" }
        },
        Item::Verse(r) => {
            let arabic = r.highlighted.clone().unwrap_or_else(|| r.text.clone());
            let translation = r.translations.first().map(|t| t.text.clone());
            rsx! {
                span { class: "search-badge", "{r.verse_key}" }
                span { class: "search-option-text",
                    span { class: "search-option-arabic", lang: "ar", dir: "rtl",
                        Highlighted { html: arabic }
                    }
                    if let Some(t) = translation {
                        span { class: "search-option-translation", dir: "auto",
                            Highlighted { html: t }
                        }
                    }
                }
            }
        }
        Item::All => rsx! {
            span { class: "search-badge search-badge-all", "Search" }
            span { class: "search-option-text", "Show all results for “{query.trim()}”" }
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stepping_wraps() {
        assert_eq!(step(None, 0, true), None);
        assert_eq!(step(None, 3, true), Some(0));
        assert_eq!(step(None, 3, false), Some(2));
        assert_eq!(step(Some(2), 3, true), Some(0));
        assert_eq!(step(Some(0), 3, false), Some(2));
        assert_eq!(step(Some(1), 3, true), Some(2));
    }

    #[test]
    fn verse_search_only_for_words() {
        assert!(!wants_verse_search("2:255"));
        assert!(!wants_verse_search("18"));
        assert!(wants_verse_search("mercy"));
        assert!(wants_verse_search("نور"));
    }

    #[test]
    fn item_routes() {
        assert_eq!(Item::All.route(" light "), Route::SearchPage { q: crate::route::SearchQuery("light".into()) });
    }
}
