//! Mount every route in a VirtualDom (no network) and render it to HTML.
//!
//! These catch runtime-only Dioxus mistakes that compile fine, such as calling a
//! hook inside another hook's initializer, which panics and leaves a view blank.
//! Network requests never complete here, so views are checked in their first,
//! loading state.

use std::rc::Rc;

use dioxus::history::MemoryHistory;
use dioxus::prelude::*;

use crate::Route;

#[derive(Props, Clone, PartialEq)]
struct HarnessProps {
    path: String,
}

#[allow(non_snake_case)]
fn Harness(props: HarnessProps) -> Element {
    use_context_provider(|| quran_api::Client::new("http://127.0.0.1:9/unreachable"));
    // Pre-seed the chapter cache (as if the chapter list had loaded) so chapter views
    // get past their lookup and mount the verse list.
    use_hook(|| {
        let list: quran_api::ChaptersResponse =
            serde_json::from_str(include_str!("../../crates/quran-api/tests/fixtures/chapters.json")).unwrap();
        *crate::reader::CHAPTERS.write() = Some(("en".into(), list.chapters));
    });
    crate::settings::provide_settings();
    let path = props.path.clone();
    use_hook(move || dioxus::history::provide_history_context(Rc::new(MemoryHistory::with_initial_path(path))));
    rsx! { Router::<Route> {} }
}

/// Render `path`; panics (failing the test) if any component panicked.
fn render(path: &str) -> String {
    let mut dom = VirtualDom::new_with_props(Harness, HarnessProps { path: path.to_string() });
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        dom.rebuild_in_place();
        dioxus_ssr::render(&dom)
    }));
    match caught {
        Ok(html) => html,
        Err(_) => panic!("rendering {path} panicked"),
    }
}

/// Mount `path`, then let async work (resources, the failing requests to the
/// unreachable API, effects) run for a short while before rendering.
async fn render_settled(path: &str) -> String {
    let mut dom = VirtualDom::new_with_props(Harness, HarnessProps { path: path.to_string() });
    dom.rebuild_in_place();
    for _ in 0..20 {
        if tokio::time::timeout(std::time::Duration::from_millis(50), dom.wait_for_work()).await.is_err() {
            continue;
        }
        dom.render_immediate(&mut dioxus::dioxus_core::NoOpMutations);
    }
    dioxus_ssr::render(&dom)
}

/// Like `render`, but also requires `needle` (a marker of the expected view) in the output,
/// so a component that errored out and rendered nothing is caught too.
fn assert_renders(path: &str, needle: &str) {
    let html = render(path);
    assert!(html.contains(needle), "{path}: expected {needle:?} in output:\n{html}");
}

#[test]
fn home_mounts() {
    assert_renders("/", "site-header");
}

#[test]
fn mushaf_page_mounts_verse_list() {
    // VerseList renders immediately here (no chapter lookup needed): it must show
    // its loading state, not panic and not claim "No verses found".
    let html = render("/page/1");
    assert!(html.contains("mushaf-view"), "{html}");
    assert!(html.contains("loading"), "verse list did not render:\n{html}");
    assert!(!html.contains("No verses found"), "{html}");
    assert!(!html.contains("integrity-banner"), "{html}");
}

#[test]
fn juz_mounts_verse_list() {
    let html = render("/juz/30");
    assert!(html.contains("juz-view"), "{html}");
    assert!(html.contains("loading"), "verse list did not render:\n{html}");
    assert!(!html.contains("integrity-banner"), "{html}");
}

#[test]
fn chapter_routes_mount() {
    for path in ["/2", "/2/255", "/2/1-5", "/2:255"] {
        let _ = render(path);
    }
}

#[test]
fn invalid_routes_show_errors() {
    assert_renders("/page/999", "does not exist");
    assert_renders("/juz/31", "does not exist");
    assert_renders("/no/such/route/here", "Page not found");
}

#[test]
fn search_and_settings_mount() {
    assert_renders("/search?q=light%20%26%20dark", "site-header");
    assert_renders("/settings", "Settings");
}

#[tokio::test(flavor = "current_thread")]
async fn chapter_view_mounts_verse_list_and_reports_errors() {
    // The fixture has surah 1 (7 verses). Requests fail (unreachable API), so the
    // verse list must end in a visible error with a retry button, never a blank view
    // or a false "missing verses" banner.
    for path in ["/1", "/1/2-5"] {
        let html = render_settled(path).await;
        assert!(html.contains("chapter-view"), "{path}:\n{html}");
        assert!(html.contains("Could not load verses"), "{path}: no error shown:\n{html}");
        assert!(html.contains("Try again"), "{path}:\n{html}");
        assert!(!html.contains("integrity-banner"), "{path}:\n{html}");
    }
}

#[tokio::test(flavor = "current_thread")]
async fn out_of_range_verse_redirects_or_errors() {
    // Surah 1 has 7 verses: a range starting past the end is an error.
    let html = render_settled("/1/9-12").await;
    assert!(html.contains("reader-error") || html.contains("error-box"), "{html}");
}
