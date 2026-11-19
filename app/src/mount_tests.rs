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
        *crate::reader::CHAPTERS.write() = Some(("en".into(), list.chapters));
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

// ---------------------------------------------------------------------------
// Mushaf rendering against a local fake API.

/// A word JSON value on `(page, line)`; `end` makes it the ayah-end marker.
fn word_json(pos: u16, page: u16, line: u16, end: bool, meaning: &str) -> serde_json::Value {
    serde_json::json!({
        "id": pos, "position": pos, "char_type_name": if end { "end" } else { "word" },
        "qpc_uthmani_hafs": format!("ق{pos}"), "text_uthmani": format!("ع{pos}"),
        "code_v2": format!("\u{fb50}{pos}"), "v2_page": page, "page_number": page, "line_number": line,
        "location": "", "translation": { "text": meaning }
    })
}

/// Verse `c:v` with `n` words on `line` (+ end marker) of `page`.
fn verse_json(c: u16, v: u16, n: u16, page: u16, line: u16) -> serde_json::Value {
    let mut words: Vec<serde_json::Value> =
        (1..=n).map(|p| word_json(p, page, line, false, &format!("m{c}-{v}-{p}"))).collect();
    words.push(word_json(n + 1, page, line, true, &format!("({v})")));
    serde_json::json!({
        "id": v, "verse_number": v, "verse_key": format!("{c}:{v}"), "page_number": page,
        "text_uthmani": "نص", "words": words,
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
                "/verses/by_page/604" | "/verses/by_chapter/114" => verses_response(vec![
                    verse_json(114, 1, 3, 604, 11),
                    verse_json(114, 2, 2, 604, 12),
                    verse_json(114, 3, 2, 604, 13),
                ]),
                // 2:5's second word has no line data → flowing block + banner.
                "/verses/by_page/2" => {
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
    // Surah header (name from the seeded chapter list) and Bismillah before 114:1.
    assert!(html.contains("surah-header-line"), "{html}");
    assert!(html.contains("Surah An-Nas"), "{html}");
    assert!(html.contains("bismillah-line"), "{html}");
    assert!(html.contains("In the Name of Allah—the Most Compassionate, Most Merciful."), "{html}");
    let header = html.find("surah-header-line").unwrap();
    let first_line = html.find("class=\"mushaf-line align").unwrap();
    assert!(header < first_line, "header must come before the first line");
    // Three lines, 7 words, 3 ayah markers, 3 verse anchors with word counts.
    assert_eq!(count(&html, "class=\"mushaf-line align-justify\""), 3, "{html}");
    for line in ["11", "12", "13"] {
        assert!(html.contains(&format!("data-line=\"{line}\"")), "{html}");
    }
    assert_eq!(count(&html, "data-location=\"114:"), 10, "{html}");
    assert_eq!(count(&html, "class=\"ayah-marker\""), 3, "{html}");
    assert!(html.contains("data-word-count=\"3\""), "{html}");
    assert_eq!(count(&html, "class=\"verse-anchor\""), 3, "{html}");
    assert!(html.contains("id=\"verse-114:1\""), "{html}");
    // Fonts can't load here (no JS): every word falls back to its Unicode text.
    assert!(!html.contains("data-mode=\"glyph\""), "{html}");
    assert!(html.contains("ق1"), "{html}");
    assert!(html.contains("\u{fd3f}١\u{fd3e}"), "{html}");
    assert!(html.contains("font-banner"), "{html}");
    assert!(!html.contains("word-missing"), "{html}");
    // Word-by-word meanings (tooltip or inline, per the default settings).
    assert!(html.contains("m114-1-1"), "{html}");
    assert!(!html.contains("integrity-banner"), "{html}");
    assert!(html.contains(">604<"), "page number footer:\n{html}");
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
    assert!(html.contains("chapter-view"), "{html}");
    assert!(!html.contains("surah-header-line"), "no header for a range not starting at 1:\n{html}");
    assert_eq!(count(&html, "class=\"verse-anchor\""), 2, "{html}");
    assert!(html.contains("align-end"), "first line of a range is packed, not justified:\n{html}");
    assert!(html.contains("align-start"), "{html}");
}

#[tokio::test(flavor = "current_thread")]
async fn surah_slugs_redirect_to_numeric_urls() {
    let api = fake_api();
    let html = render_settled_with("/an-nas", &api, 60).await;
    assert!(html.contains("chapter-view"), "{html}");
    assert!(html.contains("114. An-Nas"), "{html}");
    let html = render_settled_with("/an-nas/2-3", &api, 60).await;
    assert!(html.contains("Verses 2–3"), "{html}");
    let html = render_settled_with("/no-such-surah", &api, 20).await;
    assert!(html.contains("No surah called"), "{html}");
}
