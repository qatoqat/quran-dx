//! Mount every route in a VirtualDom (no network) and render it to HTML.
//!
//! These catch runtime-only Dioxus mistakes that compile fine, such as calling a
//! hook inside another hook's initializer, which panics and leaves a view blank.
//! Requests go to an unreachable host (views are checked in their loading/error
//! states), except in the mushaf tests, which run a tiny local HTTP server that
//! serves synthetic verses so the full page → line → word markup is rendered.

use std::rc::Rc;

use dioxus::history::MemoryHistory;
use dioxus::prelude::*;

use crate::Route;

#[derive(Props, Clone, PartialEq)]
struct HarnessProps {
    path: String,
    api: String,
}

/// Test-only client-side navigation: set this and the app shell pushes the route
/// through the real router (like clicking a link), without remounting the app.
pub(crate) static TEST_NAV: GlobalSignal<Option<String>> = Signal::global(|| None);

/// Called from `Shell` in test builds only.
pub(crate) fn use_test_navigation() {
    use_effect(|| {
        if let Some(path) = TEST_NAV.read().clone()
            && let Ok(route) = path.parse::<Route>()
        {
            navigator().push(route);
        }
    });
}

const UNREACHABLE: &str = "http://127.0.0.1:9/unreachable";

#[allow(non_snake_case)]
fn Harness(props: HarnessProps) -> Element {
    let api = props.api.clone();
    use_context_provider(move || quran_api::Client::new(api));
    // Pre-seed the chapter cache (as if the chapter list had loaded) so chapter views
    // get past their lookup and mount the verse list.
    use_hook(|| {
        let list: quran_api::ChaptersResponse =
            serde_json::from_str(include_str!("../../crates/quran-api/tests/fixtures/chapters.json")).unwrap();
        *crate::reader::CHAPTERS.write() = Some(Rc::new(list.chapters));
    });
    crate::settings::provide_settings();
    let path = props.path.clone();
    use_hook(move || dioxus::history::provide_history_context(Rc::new(MemoryHistory::with_initial_path(path))));
    rsx! { Router::<Route> {} }
}

/// Render `path`; panics (failing the test) if any component panicked.
fn render(path: &str) -> String {
    let mut dom = VirtualDom::new_with_props(Harness, HarnessProps { path: path.to_string(), api: UNREACHABLE.into() });
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
    render_settled_with(path, UNREACHABLE, 20).await
}

async fn render_settled_with(path: &str, api: &str, rounds: usize) -> String {
    let mut dom = VirtualDom::new_with_props(Harness, HarnessProps { path: path.to_string(), api: api.into() });
    dom.rebuild_in_place();
    for _ in 0..rounds {
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
    assert!(html.contains("Page 1"), "{html}");
    // Page 1 has a next page but no previous one.
    assert!(html.contains("← Next: Page 2") && !html.contains("Previous:"), "{html}");
    assert!(render("/page/3").contains("Previous: Page 2 →"), "no previous link on page 3");
    assert!(html.contains("loading"), "verse list did not render:\n{html}");
    assert!(!html.contains("No verses found"), "{html}");
    assert!(!html.contains("integrity-banner"), "{html}");
}

#[test]
fn juz_mounts_verse_list() {
    let html = render("/juz/30");
    assert!(html.contains("Juz 30"), "{html}");
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
fn header_has_mode_toggle_and_closed_search() {
    let html = render("/");
    for needle in ["aria-label=\"Search\"", "class=\"mode-toggle\"", "aria-label=\"Settings\"", "surah-filter"] {
        assert!(html.contains(needle), "expected {needle:?} in:\n{html}");
    }
    // The search input and the settings panel are mounted only while open.
    assert!(!html.contains("search-input") && !html.contains("id=\"settings-panel\""), "{html}");
    // On the surah list with nothing read yet: Surah and Page start reading, Juz shows its list.
    for href in ["href=\"/1\"", "href=\"/juz\"", "href=\"/page/1\""] {
        assert!(html.contains(href), "expected {href:?} in:\n{html}");
    }
}

#[test]
fn header_names_the_view() {
    for (path, title) in [("/page/22", "Page 22"), ("/juz/2#2:142", "Juz 2"), ("/2", "Surah 2"), ("/2/255", "Surah 2")] {
        let html = render(path);
        assert!(html.contains(&format!("<h1 class=\"view-title\">{title}</h1>")), "{path}: no title {title:?}");
    }
    for path in ["/", "/juz", "/page/999", "/search?q=x"] {
        assert!(!render(path).contains("view-title"), "{path} should have no title");
    }
}

#[test]
fn list_routes_mount() {
    let juz = render("/juz");
    // On its list, the current mode's button leads to reading (here: nothing read yet).
    assert!(juz.contains("href=\"/juz/1\" class=\"active\""), "the toggle does not mark Juz");
    assert!(juz.contains("data-tip=\"Open Juz 1\"") && juz.contains("data-tip=\"Show the surah list\""), "no tooltips");
    // Pages have no list.
    assert!(!juz.contains("href=\"/page\""), "a link to a page list remains");
}

#[test]
fn reader_urls_with_a_position_mount() {
    for path in ["/2#2:142", "/juz/2#2:142", "/page/22#2:142:3", "/2/140-150#2:142", "/2#nonsense"] {
        let html = render(path);
        assert!(html.contains("loading"), "{path}:\n{html}");
    }
}

#[test]
fn search_mounts() {
    assert_renders("/search?q=light%20%26%20dark", "site-header");
}

#[test]
fn settings_panel_is_mounted_only_while_open() {
    #[allow(non_snake_case)]
    fn Panel() -> Element {
        use_context_provider(|| quran_api::Client::new(UNREACHABLE.to_string()));
        crate::settings::provide_settings();
        rsx! { crate::settings::SettingsPanel { onclose: |_| {} } }
    }
    let mut dom = VirtualDom::new(Panel);
    dom.rebuild_in_place();
    let html = dioxus_ssr::render(&dom);
    for needle in ["id=\"settings-panel\"", "Mushaf font", "Word by word", "Text size", "Content width", "Font source URL"] {
        assert!(html.contains(needle), "expected {needle:?} in:\n{html}");
    }
    let closed = render("/page/1");
    assert!(closed.contains("aria-controls=\"settings-panel\"") && !closed.contains("id=\"settings-panel\""), "{closed}");
}

#[tokio::test(flavor = "current_thread")]
async fn chapter_view_mounts_verse_list_and_reports_errors() {
    // The fixture has surah 1 (7 verses). Requests fail (unreachable API), so the
    // verse list must end in a visible error with a retry button, never a blank view
    // or a false "missing verses" banner.
    for path in ["/1", "/1/2-5"] {
        let html = render_settled(path).await;
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

// ---------------------------------------------------------------------------
// Mushaf rendering against a local fake API.

/// A word JSON value on `(page, line)`; `end` makes it the ayah-end marker.
/// As from the real API, words have no QPC text of their own (the verse has it).
fn word_json(pos: u16, page: u16, line: u16, end: bool, meaning: &str) -> serde_json::Value {
    serde_json::json!({
        "id": pos, "position": pos, "char_type_name": if end { "end" } else { "word" },
        "qpc_uthmani_hafs": null, "text_uthmani": format!("ع{pos}"),
        "code_v2": format!("\u{fb50}{pos}"), "v2_page": page, "page_number": page, "line_number": line,
        "location": "", "translation": { "text": meaning }
    })
}

/// Verse `c:v` with `n` words on `line` (+ end marker) of `page`.
fn verse_json(c: u16, v: u16, n: u16, page: u16, line: u16) -> serde_json::Value {
    let mut words: Vec<serde_json::Value> =
        (1..=n).map(|p| word_json(p, page, line, false, &format!("m{c}-{v}-{p}"))).collect();
    words.push(word_json(n + 1, page, line, true, &format!("({v})")));
    let qpc: Vec<String> = (1..=n).map(|p| format!("ق{p}")).collect();
    serde_json::json!({
        "id": v, "verse_number": v, "verse_key": format!("{c}:{v}"), "page_number": page,
        "text_uthmani": "نص", "words": words,
        "qpc_uthmani_hafs": format!("{}\u{a0}{}", qpc.join(" "), crate::reader::glyph::arabic_number(v)),
        "translations": [{ "resource_id": 131, "text": format!("Meaning of {c}:{v}<sup foot_note=1>1</sup>") }]
    })
}

fn verses_response(verses: Vec<serde_json::Value>) -> String {
    let n = verses.len();
    serde_json::json!({
        "verses": verses,
        "pagination": { "per_page": 50, "current_page": 1, "next_page": null, "total_pages": 1, "total_records": n }
    })
    .to_string()
}

/// Serve the fake API on a random local port; returns its base URL.
fn fake_api() -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                match stream.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => buf.extend_from_slice(&chunk[..n]),
                }
            }
            let req = String::from_utf8_lossy(&buf);
            let path = req.split_whitespace().nth(1).unwrap_or("").split('?').next().unwrap_or("").to_string();
            let body = match path.as_str() {
                // 114:1-3 (header + Bismillah before 114:1), lines 11-13.
                "/juzs" => serde_json::json!({ "juzs": [
                    { "id": 29, "juz_number": 29, "verse_mapping": { "67": "1-1" } },
                    { "id": 30, "juz_number": 30, "verse_mapping": { "114": "1-3" } },
                ]})
                .to_string(),
                "/verses/by_juz/29" => verses_response(vec![verse_json(67, 1, 2, 562, 3)]),
                "/verses/by_page/604" | "/verses/by_chapter/114" | "/verses/by_juz/30" => verses_response(vec![
                    verse_json(114, 1, 3, 604, 11),
                    verse_json(114, 2, 2, 604, 12),
                    verse_json(114, 3, 2, 604, 13),
                ]),
                // 2:5's second word has no line data → flowing block + banner.
                "/verses/by_page/2" | "/verses/by_chapter/2" => {
                    let mut broken = verse_json(2, 5, 3, 2, 14);
                    broken["words"][1]["line_number"] = serde_json::Value::Null;
                    verses_response(vec![verse_json(2, 4, 2, 2, 13), broken])
                }
                "/verses/by_key/1:1" => serde_json::json!({ "verse": {
                    "id": 1, "verse_number": 1, "verse_key": "1:1",
                    "words": [word_json(1, 1, 2, false, "In (the) name"), word_json(2, 1, 2, false, "(of) Allah"),
                              word_json(3, 1, 2, false, "the Most Gracious"), word_json(4, 1, 2, false, "the Most Merciful"),
                              word_json(5, 1, 2, true, "(1)")],
                    "translations": [{ "resource_id": 131, "text": "In the Name of Allah—the Most Compassionate, Most Merciful." }]
                }})
                .to_string(),
                _ => {
                    let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    continue;
                }
            };
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(body.as_bytes());
        }
    });
    base
}

fn count(html: &str, needle: &str) -> usize {
    html.matches(needle).count()
}

#[tokio::test(flavor = "current_thread")]
async fn mushaf_page_renders_lines_words_headers_and_bismillah() {
    let api = fake_api();
    let html = render_settled_with("/page/604", &api, 60).await;
    assert!(html.contains("class=\"mushaf-page\""), "{html}");
    assert!(html.contains("data-page=\"604\""), "{html}");
    // Surah banner (name, place and verse count from the seeded chapter list) and
    // Bismillah before 114:1.
    assert!(html.contains("class=\"surah-banner"), "{html}");
    assert!(html.contains("Surah An-Nas · Meccan · 6 verses"), "{html}");
    assert!(html.contains("مكية") && html.contains("آياتها ٦"), "{html}");
    assert!(html.contains("class=\"bismillah "), "{html}");
    assert!(html.contains("In the Name of Allah—the Most Compassionate, Most Merciful."), "{html}");
    let header = html.find("class=\"surah-banner").unwrap();
    let first_line = html.find("class=\"mushaf-line align").unwrap();
    assert!(header < first_line, "banner must come before the first line");
    // Three lines, 7 words, 3 ayah markers, 3 verse anchors with word counts.
    assert_eq!(count(&html, "class=\"mushaf-line align-justify\""), 3, "{html}");
    for line in ["11", "12", "13"] {
        assert!(html.contains(&format!("data-line=\"{line}\"")), "{html}");
    }
    assert_eq!(count(&html, "data-location=\"114:"), 10, "{html}");
    assert_eq!(count(&html, "class=\"ayah-marker qpc\""), 3, "{html}");
    assert!(html.contains("data-word-count=\"3\""), "{html}");
    assert_eq!(count(&html, "data-word-count="), 3, "one anchor per verse:\n{html}");
    assert!(html.contains("id=\"verse-114:1\""), "{html}");
    // Fonts can't load here (no JS): every word falls back to its QPC Unicode text,
    // taken from the verse's text, and every ayah marker to its number.
    assert!(!html.contains("data-mode=\"glyph\""), "{html}");
    assert!(html.contains("class=\"word qpc\"") && html.contains(">ق1<"), "{html}");
    assert!(html.contains(">١<"), "{html}");
    assert!(!html.contains("class=\"word missing"), "{html}");
    // Word-by-word meanings (tooltip or inline, per the default settings).
    assert!(html.contains("m114-1-1"), "{html}");
    // The fake API serves only 114:1-3 for page 604; the page is known to begin at
    // 112:1 and end at 114:6, so the verses that did not arrive are reported.
    assert!(html.contains("Some verses were missing from the server response: 112:1, 112:2"), "{html}");
    assert!(html.contains("114:4, 114:5, 114:6") && !html.contains("looked incomplete"), "{html}");
    assert!(html.contains(">604</a>"), "page number link:\n{html}");
}

#[tokio::test(flavor = "current_thread")]
async fn mushaf_missing_line_data_shows_flowing_block_and_banner() {
    let api = fake_api();
    let html = render_settled_with("/page/2", &api, 60).await;
    assert!(html.contains("mushaf-flow"), "{html}");
    assert!(html.contains("data-fallback=\"layout\""), "{html}");
    assert!(html.contains("integrity-banner"), "{html}");
    assert!(html.contains("Mushaf line data was missing"), "{html}");
    // No word lost: 2:4 (2 words) + 2:5 (3 words), plus 2 markers.
    assert_eq!(count(&html, "data-location=\"2:"), 7, "{html}");
    assert!(html.contains("data-word-count=\"3\""), "{html}");
}

#[tokio::test(flavor = "current_thread")]
async fn verse_range_trims_partial_lines() {
    let api = fake_api();
    let html = render_settled_with("/114/2-3", &api, 60).await;
    assert!(html.contains("An-Nas · Verses 2–3 of 6"), "{html}");
    assert!(!html.contains("surah-banner"), "no banner for a range not starting at 1:\n{html}");
    assert_eq!(count(&html, "data-word-count="), 2, "{html}");
    assert!(html.contains("align-end"), "first line of a range is packed, not justified:\n{html}");
    assert!(html.contains("align-start"), "{html}");
}

#[tokio::test(flavor = "current_thread")]
async fn surah_slugs_redirect_to_numeric_urls() {
    let api = fake_api();
    let html = render_settled_with("/an-nas", &api, 60).await;
    assert!(html.contains(r#"data-location="114:1:1""#), "{html}");
    assert!(html.contains("Surah An-Nas"), "{html}");
    let html = render_settled_with("/an-nas/2-3", &api, 60).await;
    assert!(html.contains("Verses 2–3"), "{html}");
    let html = render_settled_with("/no-such-surah", &api, 20).await;
    assert!(html.contains("No surah called"), "{html}");
}

async fn settle(dom: &mut VirtualDom) {
    for _ in 0..40 {
        if tokio::time::timeout(std::time::Duration::from_millis(50), dom.wait_for_work()).await.is_err() {
            continue;
        }
        dom.render_immediate(&mut dioxus::dioxus_core::NoOpMutations);
    }
}

/// Mount `from`, let it load, then navigate client-side to `to` (same app, same
/// components, like following a link) and let that load too.
async fn navigate_settled(from: &str, to: &str, api: &str) -> String {
    let mut dom = VirtualDom::new_with_props(Harness, HarnessProps { path: from.to_string(), api: api.into() });
    dom.rebuild_in_place();
    settle(&mut dom).await;
    dom.in_runtime(|| *TEST_NAV.write() = Some(to.to_string()));
    settle(&mut dom).await;
    dioxus_ssr::render(&dom)
}

/// Regression: navigating between two views of the same kind (page→page,
/// surah→surah, juz→juz) must load the NEW content, not keep the old verses.
#[tokio::test(flavor = "current_thread")]
async fn client_side_navigation_loads_the_new_page() {
    let api = fake_api();
    let html = navigate_settled("/page/2", "/page/604", &api).await;
    assert!(html.contains("Page 604"), "{html}");
    assert!(html.contains(r#"data-location="114:1:1""#), "page 604's verses not shown after navigation:\n{html}");
    assert!(!html.contains(r#"data-location="2:4:1""#), "page 2's verses still shown on page 604:\n{html}");
}

#[tokio::test(flavor = "current_thread")]
async fn client_side_navigation_between_surahs() {
    let api = fake_api();
    let html = navigate_settled("/2", "/114", &api).await;
    assert!(html.contains(r#"data-location="114:1:1""#), "surah 114 not shown after navigation:\n{html}");
    assert!(!html.contains(r#"data-location="2:4:1""#), "surah 2 still shown:\n{html}");
}

/// Juz → juz: the juz's expected verse ranges must follow the new juz (stale
/// ranges would raise a false "verses missing" warning for the old juz).
#[tokio::test(flavor = "current_thread")]
async fn client_side_navigation_between_juz() {
    let api = fake_api();
    let html = navigate_settled("/juz/29", "/juz/30", &api).await;
    assert!(html.contains("Juz 30"), "{html}");
    assert!(html.contains(r#"data-location="114:1:1""#), "juz 30 not shown:\n{html}");
    assert!(!html.contains(r#"data-location="67:1:1""#), "juz 29 still shown:\n{html}");
    assert!(!html.contains("missing from the server response"), "false missing-verse warning:\n{html}");
}

/// Surah banner: a plain framed text line until the QUL banner fonts are verified;
/// then the ornamental frame with the name fitted in its cartouche and the place
/// and verse count in its roundels.
#[tokio::test(flavor = "current_thread")]
async fn surah_banner_text_then_glyph() {
    use crate::reader::fonts::{BANNER, BannerFonts, BannerMetrics, Ink};
    let api = fake_api();
    let mut dom = VirtualDom::new_with_props(Harness, HarnessProps { path: "/page/604".into(), api });
    dom.rebuild_in_place();
    settle(&mut dom).await;
    let html = dioxus_ssr::render(&dom);
    assert!(html.contains(r#"data-banner="text""#), "{html}");
    assert!(html.contains("سورة"), "banner text missing:\n{html}");
    assert!(!html.contains("surah114 surah-icon"), "ligature codes must never show without the fonts:\n{html}");

    let metrics = BannerMetrics { frame: Ink(0.0, 8047.0, 813.0, 188.0), names: vec![Ink(0.0, 2546.0, 688.0, 78.0); 114] };
    dom.in_runtime(|| *BANNER.write() = Some(BannerFonts::Ready(Rc::new(metrics))));
    settle(&mut dom).await;
    let html = dioxus_ssr::render(&dom);
    assert!(html.contains(r#"data-banner="glyph""#), "{html}");
    assert!(html.contains(r#"viewBox="0 0 8047.0 1001.0""#), "{html}");
    assert!(html.contains(">surah114 surah-icon<") && html.contains(">header<"), "{html}");
    assert!(html.contains(">مكية<") && html.contains(">٦<"), "roundel labels:\n{html}");
}
