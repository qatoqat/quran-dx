//! Search: header search box with suggestions, results page.

use dioxus::prelude::*;

#[component]
pub fn SearchBox() -> Element {
    rsx! { input { r#type: "search", placeholder: "Search" } } // TODO(search agent)
}

#[component]
pub fn SearchPage(q: String) -> Element {
    rsx! { h1 { "Search: {q}" } } // TODO(search agent)
}
