//! Quran.com reader in Dioxus. One codebase, two targets:
//! `dx serve --platform web` and `dx serve --platform desktop`.
//!
//! Module ownership (for parallel work):
//! - `route`, `layout`, `util`, `main`: shared scaffold — change only with care.
//! - `reader/*`: core reader (home, chapter, ranges, page, juz, verse rendering).
//! - `settings/*`: settings state/persistence/page, word-by-word, footnotes.
//! - `search/*`: search box, suggestions, results page.

use dioxus::prelude::*;

mod layout;
mod reader;
mod route;
mod search;
mod settings;
mod util;

pub use route::Route;

const BASE_CSS: Asset = asset!("/assets/main.css");
const READER_CSS: Asset = asset!("/assets/reader.css");
const SETTINGS_CSS: Asset = asset!("/assets/settings.css");
const SEARCH_CSS: Asset = asset!("/assets/search.css");

fn main() {
    dioxus::launch(App);
}

#[component]
fn App() -> Element {
    // Shared API client and user settings, available to every component.
    use_context_provider(quran_api::Client::default);
    settings::provide_settings();

    rsx! {
        document::Title { "The Noble Quran" }
        document::Meta { name: "viewport", content: "width=device-width, initial-scale=1" }
        document::Stylesheet { href: BASE_CSS }
        document::Stylesheet { href: READER_CSS }
        document::Stylesheet { href: SETTINGS_CSS }
        document::Stylesheet { href: SEARCH_CSS }
        settings::FontLoader {}
        Router::<Route> {}
    }
}

/// The shared API client.
pub fn use_api() -> quran_api::Client {
    use_context::<quran_api::Client>()
}
