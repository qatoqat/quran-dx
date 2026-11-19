//! App shell: the header (home link, the view's title or the search box,
//! surah / juz / page toggle, settings) around the current view.

use dioxus::prelude::*;
use quran_api::{Chapter, Juz};

use crate::reader::position::{self, LAST_READ, POSITION, Position};
use crate::reader::{find_chapter, juz_of, parse, use_chapters, use_juzs};
use crate::route::{Anchor, Mode};
use crate::search::SearchBox;
use crate::settings::SettingsPanel;
use crate::Route;

#[component]
pub fn Shell() -> Element {
    #[cfg(test)]
    crate::mount_tests::use_test_navigation();
    position::use_position_tracking();
    crate::settings::use_reading_pointer();
    #[cfg(not(target_arch = "wasm32"))]
    use_saved_location();
    let mut search_open = use_signal(|| false);
    let mut settings_open = use_signal(|| false);
    rsx! {
        header { class: "site-header",
            Link { class: "brand", to: Route::Home {}, "The Noble Quran" }
            // The middle shows what is being read, or the search box while searching
            // (mounted, like the settings panel, only while open).
            if search_open() {
                SearchBox { onclose: move |_| search_open.set(false) }
            } else {
                ViewTitle {}
            }
            div { class: "header-tools",
                button {
                    r#type: "button",
                    class: "icon-button",
                    aria_label: "Search",
                    aria_expanded: search_open(),
                    // Keep the focus where it is: the search box closes itself when it loses it.
                    onmousedown: move |e| e.prevent_default(),
                    onclick: move |_| search_open.toggle(),
                    svg { class: "icon", view_box: "0 0 24 24", "aria-hidden": "true",
                        circle { cx: "11", cy: "11", r: "7" }
                        path { d: "m20 20-4-4" }
                    }
                }
                ModeToggle {}
                button {
                    r#type: "button",
                    class: "icon-button",
                    aria_label: "Settings",
                    aria_expanded: settings_open(),
                    aria_controls: "settings-panel",
                    onclick: move |_| settings_open.toggle(),
                    svg { class: "icon", view_box: "0 0 24 24", "aria-hidden": "true",
                        circle { cx: "12", cy: "12", r: "3" }
                        path { d: GEAR }
                    }
                }
            }
        }
        main { class: "site-main", Outlet::<Route> {} }
        footer { class: "site-footer",
            p { "Text, translations and data from the Quran.com API." }
        }
        if settings_open() {
            SettingsPanel { onclose: move |_| settings_open.set(false) }
        }
    }
}

/// "Surah Al-Baqarah", "Juz 2" or "Page 22": the heading of a reader view.
fn view_title(route: &Route, chapters: &[Chapter]) -> Option<String> {
    let surah = |n: u16| match find_chapter(chapters, n) {
        Some(c) => format!("Surah {}", c.name_simple),
        None => format!("Surah {n}"),
    };
    match route {
        Route::MushafPage { page, .. } if parse::valid_mushaf_page(*page) => Some(format!("Page {page}")),
        Route::JuzPage { juz, .. } if parse::valid_juz(*juz) => Some(format!("Juz {juz}")),
        Route::ChapterPage { chapter, .. } => parse::parse_chapter_segment(chapter).ok().map(|r| surah(r.chapter)),
        Route::ChapterRange { chapter, .. } => parse::parse_chapter_number(chapter).ok().map(surah),
        _ => None,
    }
}

#[component]
fn ViewTitle() -> Element {
    let route = use_route::<Route>();
    let chapters = use_chapters();
    let chapters = chapters.read();
    let chapters: &[Chapter] = chapters.as_ref().and_then(|r| r.as_ref().ok()).map_or(&[], |list| list.as_slice());
    rsx! {
        if let Some(title) = view_title(&route, chapters) {
            h1 { class: "view-title", "{title}" }
        }
    }
}

/// Outline of a cog wheel (Feather icons, MIT).
const GEAR: &str = "M19.4 15a1.65 1.65 0 0 0 .33 1.82l.06.06a2 2 0 0 1 0 2.83 2 2 0 0 1-2.83 0l-.06-.06a1.65 1.65 0 0 0-1.82-.33 \
    1.65 1.65 0 0 0-1 1.51V21a2 2 0 0 1-2 2 2 2 0 0 1-2-2v-.09A1.65 1.65 0 0 0 9 19.4a1.65 1.65 0 0 0-1.82.33l-.06.06a2 2 0 0 1-2.83 0 \
    2 2 0 0 1 0-2.83l.06-.06a1.65 1.65 0 0 0 .33-1.82 1.65 1.65 0 0 0-1.51-1H3a2 2 0 0 1-2-2 2 2 0 0 1 2-2h.09A1.65 1.65 0 0 0 4.6 9 \
    a1.65 1.65 0 0 0-.33-1.82l-.06-.06a2 2 0 0 1 0-2.83 2 2 0 0 1 2.83 0l.06.06a1.65 1.65 0 0 0 1.82.33H9a1.65 1.65 0 0 0 1-1.51V3 \
    a2 2 0 0 1 2-2 2 2 0 0 1 2 2v.09a1.65 1.65 0 0 0 1 1.51 1.65 1.65 0 0 0 1.82-.33l.06-.06a2 2 0 0 1 2.83 0 2 2 0 0 1 0 2.83l-.06.06 \
    a1.65 1.65 0 0 0-.33 1.82V9a1.65 1.65 0 0 0 1.51 1H21a2 2 0 0 1 2 2 2 2 0 0 1-2 2h-.09a1.65 1.65 0 0 0-1.51 1z";

/// What the toggle knows about the reader: the view on screen, the place being
/// read (if any), the last place read, and the lists it needs for names.
struct Reading<'a> {
    route: &'a Route,
    here: Option<Position>,
    last: Option<Position>,
    juzs: &'a [Juz],
    chapters: &'a [Chapter],
}

impl Reading<'_> {
    /// The view of `mode` that shows the place `p`, opened at it.
    fn view(&self, mode: Mode, p: Position) -> Option<Route> {
        let view = match mode {
            Mode::Surah => Route::surah(p.at.chapter),
            Mode::Juz => Route::juz(juz_of(self.juzs, p.at.chapter, p.at.verse)?),
            Mode::Page => Route::page(Some(p.page).filter(|page| *page > 0)?),
        };
        Some(view.with_anchor(Anchor(Some(p.at))))
    }

    /// Where the toggle's button for `mode` leads, and its tooltip.
    ///
    /// - The current mode's button flips between its list and reading: from a
    ///   view to the list, from the list back to the last place read (or to the
    ///   beginning, if nothing was read yet). Pages have no list, so there the
    ///   button stays where it is.
    /// - Another mode's button opens the place being read in that mode's view;
    ///   when nothing is being read, that mode's list, or for pages the last
    ///   place read.
    fn target(&self, mode: Mode) -> (Route, String) {
        let name = match mode {
            Mode::Surah => "surah",
            Mode::Juz => "juz",
            Mode::Page => "page",
        };
        let title = |to: &Route| view_title(to, self.chapters).unwrap_or_else(|| name.to_string());
        let list = Route::index(mode).map(|list| (list, format!("Show the {name} list")));
        let resume = || match self.last.and_then(|p| self.view(mode, p)) {
            Some(to) => {
                let tip = format!("Back to {}", title(&to));
                (to, tip)
            }
            None => {
                let to = match mode {
                    Mode::Surah => Route::surah(1),
                    Mode::Juz => Route::juz(1),
                    Mode::Page => Route::page(1),
                };
                let tip = format!("Open {}", title(&to));
                (to, tip)
            }
        };
        if self.route.mode() != Some(mode) {
            return match self.here.and_then(|p| self.view(mode, p)) {
                Some(to) => (to, format!("Read here by {name}")),
                None => list.unwrap_or_else(resume),
            };
        }
        match list {
            Some((list, tip)) if *self.route != list => (list, tip),
            Some(_) => resume(),
            None => (self.route.clone(), format!("Reading by {name}")),
        }
    }
}

/// Surah | Juz | Page, each with a tooltip saying where it leads (see [`Reading::target`]).
#[component]
fn ModeToggle() -> Element {
    let route = use_route::<Route>();
    let juzs = use_juzs();
    let chapters = use_chapters();
    let (juzs, chapters) = (juzs.read(), chapters.read());
    let reading = Reading {
        route: &route,
        here: POSITION(),
        last: LAST_READ(),
        juzs: juzs.as_ref().and_then(|r| r.as_ref().ok()).map_or(&[], |list| list.as_slice()),
        chapters: chapters.as_ref().and_then(|r| r.as_ref().ok()).map_or(&[], |list| list.as_slice()),
    };
    let active = route.mode();
    rsx! {
        nav { class: "mode-toggle", aria_label: "Browse by",
            for (mode , label) in [(Mode::Surah, "Surah"), (Mode::Juz, "Juz"), (Mode::Page, "Page")] {
                {
                    let (to, tip) = reading.target(mode);
                    rsx! {
                        Link {
                            key: "{label}",
                            class: if active == Some(mode) { "active" } else { "" },
                            to,
                            "data-tip": "{tip}",
                            "aria-description": "{tip}",
                            "{label}"
                        }
                    }
                }
            }
        }
    }
}

/// Desktop: remember the current view and reading position for the next start.
#[cfg(not(target_arch = "wasm32"))]
fn use_saved_location() {
    let route = use_route::<Route>();
    use_effect(use_reactive!(|route| {
        let here = match POSITION() {
            Some(p) => route.with_anchor(Anchor((!p.top).then_some(p.at))),
            None => route,
        };
        crate::settings::storage::save_location(&here.to_string());
    }));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::route::WordRef;

    fn juz(n: u8, chapter: u16, range: &str) -> Juz {
        serde_json::from_value(serde_json::json!({
            "id": n, "juz_number": n, "verse_mapping": { chapter.to_string(): range }
        }))
        .unwrap()
    }

    #[test]
    fn toggle_flips_between_list_and_reading() {
        let juzs = [juz(1, 2, "1-141"), juz(2, 2, "142-252")];
        let at = WordRef { chapter: 2, verse: 142, word: 1 };
        let place = Some(Position { at, page: 22, top: false });
        let anchor = Anchor(Some(at));
        let reading = |route: &Route, here, last| {
            let r = Reading { route, here, last, juzs: &juzs, chapters: &[] };
            [Mode::Surah, Mode::Juz, Mode::Page].map(|m| r.target(m))
        };
        let tip = |to: Route, tip: &str| (to, tip.to_string());

        // Reading juz 2: Juz shows the list, the others open the same line in their view.
        let route = Route::juz(2);
        assert_eq!(
            reading(&route, place, place),
            [
                tip(Route::surah(2).with_anchor(anchor), "Read here by surah"),
                tip(Route::JuzIndex {}, "Show the juz list"),
                tip(Route::page(22).with_anchor(anchor), "Read here by page"),
            ]
        );
        // Pages have no list: while reading by page, the Page button stays put.
        let by_page = Route::page(22);
        assert_eq!(reading(&by_page, place, place)[2], tip(by_page.clone(), "Reading by page"));
        // On the juz list afterwards: Juz and Page lead back, Surah to its list.
        assert_eq!(
            reading(&Route::JuzIndex {}, None, place),
            [
                tip(Route::Home {}, "Show the surah list"),
                tip(Route::juz(2).with_anchor(anchor), "Back to Juz 2"),
                tip(Route::page(22).with_anchor(anchor), "Back to Page 22"),
            ]
        );
        // Nothing read yet: the buttons that lead to reading start at the beginning.
        assert_eq!(
            reading(&Route::Home {}, None, None),
            [
                tip(Route::surah(1), "Open Surah 1"),
                tip(Route::JuzIndex {}, "Show the juz list"),
                tip(Route::page(1), "Open Page 1"),
            ]
        );
        // A view whose position isn't known yet (still loading) offers the other list.
        assert_eq!(reading(&route, None, None)[0], tip(Route::Home {}, "Show the surah list"));
        // Search and unknown URLs belong to no mode.
        let search = Route::SearchPage { q: crate::route::SearchQuery("x".into()) };
        assert_eq!(reading(&search, None, place)[1], tip(Route::JuzIndex {}, "Show the juz list"));
    }
}
