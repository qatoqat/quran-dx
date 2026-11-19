//! App shell: header with home link, search box and settings link.

use dioxus::prelude::*;

use crate::Route;
use crate::search::SearchBox;

#[component]
pub fn Shell() -> Element {
    #[cfg(test)]
    crate::mount_tests::use_test_navigation();
    rsx! {
        header { class: "site-header",
            Link { class: "brand", to: Route::Home {}, "The Noble Quran" }
            div { class: "header-search", SearchBox {} }
            nav { class: "header-nav",
                Link { to: Route::JuzPage { juz: 1 }, "Juz" }
                Link { to: Route::MushafPage { page: 1 }, "Pages" }
                Link { to: Route::SettingsPage {}, "Settings" }
            }
        }
        main { class: "site-main", Outlet::<Route> {} }
        footer { class: "site-footer",
            p { "Text, translations and data from the Quran.com API." }
        }
    }
}

#[component]
pub fn NotFound(segments: Vec<String>) -> Element {
    rsx! {
        section { class: "not-found",
            h1 { "Page not found" }
            p { "/{segments.join(\"/\")}" }
            Link { to: Route::Home {}, "Back to all surahs" }
        }
    }
}

/// Shared loading / error UI so all features look consistent.
#[component]
pub fn Loading() -> Element {
    rsx! { div { class: "loading", role: "status", "Loading…" } }
}

#[component]
pub fn ErrorBox(message: String) -> Element {
    rsx! { div { class: "error-box", role: "alert", "{message}" } }
}
