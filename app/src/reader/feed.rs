//! Paged verse loading shared by the chapter, mushaf-page and juz views.
//!
//! A feed fetches 50 verses per API page, appends them on "Load more" (or
//! automatically when the bottom sentinel scrolls into view), and resets
//! whenever the verse-related settings change. Loaded verses are drawn as
//! 15-line mushaf pages by `mushaf_view::MushafView`.
//!
//! A feed opened at a reading position keeps loading until that verse has
//! arrived, then scrolls to it.

use dioxus::prelude::*;
use quran_api::{Client, Verse, VerseQuery, VersesResponse};

use super::integrity::{self, IntegrityIssue};
use super::mushaf_layout::Trim;
use super::mushaf_view::MushafView;
use super::parse::{PER_PAGE, VerseSpan, api_page_of, api_pages_for, next_page_within};
use super::{position, reference};
use crate::layout::{ErrorBox, Loading};
use crate::route::Anchor;
use crate::settings::use_settings;
use crate::util::short_list;

/// What to load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VerseSource {
    Chapter { chapter: u16, span: Option<VerseSpan> },
    Page(u16),
    Juz(u8),
}

impl VerseSource {
    fn first_api_page(self) -> u32 {
        match self {
            VerseSource::Chapter { span: Some(s), .. } => api_page_of(s.from, PER_PAGE),
            _ => 1,
        }
    }

    /// Last request worth making, when known up front. For a mushaf page the
    /// requests are its listings (see `reference::listings_for_page`), one each.
    fn last_api_page(self) -> Option<u32> {
        match self {
            VerseSource::Chapter { span: Some(s), .. } => Some(*api_pages_for(s, PER_PAGE).end()),
            VerseSource::Page(p) => Some(reference::listings_for_page(p).len().max(1) as u32),
            _ => None,
        }
    }

    /// The request after `page`, given what the API says comes next.
    fn next_api_page(self, page: u32, api_next: Option<u32>) -> Option<u32> {
        match self {
            VerseSource::Page(_) => Some(page + 1).filter(|n| Some(*n) <= self.last_api_page()),
            _ => next_page_within(api_next, self.last_api_page()).filter(|n| *n > page),
        }
    }

    fn includes(self, verse: &Verse) -> bool {
        match self {
            VerseSource::Chapter { span: Some(s), .. } => s.contains(verse.verse_number),
            // A neighbouring listing is fetched for the verses printed on this page only.
            VerseSource::Page(p) => verse.chapter_number().is_some_and(|c| {
                reference::verses_on_page(p).iter().any(|(chapter, span)| *chapter == c && span.contains(verse.verse_number))
            }),
            _ => true,
        }
    }

    async fn fetch(self, client: &Client, q: &VerseQuery) -> Result<VersesResponse, quran_api::Error> {
        match self {
            VerseSource::Chapter { chapter, .. } => client.verses_by_chapter(chapter, q).await,
            VerseSource::Page(p) => {
                // Request `q.page` of a mushaf page is its `q.page`-th listing, which fits one API page.
                let listings = reference::listings_for_page(p);
                let listing = listings.get(q.page as usize - 1).copied().unwrap_or(p);
                let response = client.verses_by_page(listing, &q.with_page(1)).await?;
                if response.pagination.next_page.is_some() {
                    return Err(quran_api::Error::Status {
                        status: 0,
                        body: format!("the listing of page {listing} has more verses than one request returns"),
                    });
                }
                Ok(response)
            }
            VerseSource::Juz(j) => client.verses_by_juz(j, q).await,
        }
    }
}

/// Reactive state of a verse feed. `Copy`, so it can move into handlers freely.
#[derive(Clone, Copy)]
struct VerseFeed {
    /// Reactive: when the view navigates to another page/surah/juz the component
    /// is reused with a new `source` prop, and the feed must reload for it.
    source: Memo<VerseSource>,
    /// The source the current `verses` belong to (set on reset). Until it matches
    /// `source`, the list shows "loading" and reports no integrity issues.
    loaded_for: Signal<Option<VerseSource>>,
    client: Signal<Client>,
    query: Memo<VerseQuery>,
    verses: Signal<Vec<Verse>>,
    /// Next API page to fetch; `None` once everything is loaded.
    next_page: Signal<Option<u32>>,
    loading: Signal<bool>,
    error: Signal<Option<String>>,
    total: Signal<Option<u32>>,
    /// Problems found while merging pages (duplicates, ordering).
    merge_issues: Signal<Vec<IntegrityIssue>>,
    /// Bumped on reset so responses from stale requests are dropped.
    generation: Signal<u64>,
}

impl VerseFeed {
    fn reset(mut self) {
        *self.generation.write() += 1;
        self.verses.set(Vec::new());
        let source = *self.source.peek();
        self.loaded_for.set(Some(source));
        self.next_page.set(Some(source.first_api_page()));
        self.loading.set(false);
        self.error.set(None);
        self.total.set(None);
        self.merge_issues.set(Vec::new());
        self.load_more();
    }

    fn load_more(mut self) {
        let Some(page) = *self.next_page.peek() else { return };
        if *self.loading.peek() {
            return;
        }
        let generation = *self.generation.peek();
        self.loading.set(true);
        self.error.set(None);
        let client = self.client.peek().clone();
        let mut q = self.query.peek().with_page(page);
        q.per_page = PER_PAGE;
        let source = *self.source.peek();
        spawn(async move {
            let result = source.fetch(&client, &q).await;
            if *self.generation.peek() != generation {
                return; // settings changed meanwhile; a fresh request is in flight
            }
            self.loading.set(false);
            match result {
                Ok(resp) => {
                    // Guard against an API that keeps returning the same page.
                    let next = source.next_api_page(page, resp.pagination.next_page).filter(|_| !resp.verses.is_empty());
                    if source.last_api_page().is_none() {
                        self.total.set(Some(resp.pagination.total_records));
                    }
                    let issues = {
                        let mut verses = self.verses.write();
                        verses.extend(resp.verses.into_iter().filter(|v| source.includes(v)));
                        integrity::sort_dedup(&mut verses)
                    };
                    if !issues.is_empty() {
                        self.merge_issues.write().extend(issues);
                    }
                    self.next_page.set(next);
                    // A mushaf page is one unit: fetch its other listing right away
                    // rather than when the reader scrolls to the end.
                    if next.is_some() && matches!(source, VerseSource::Page(_)) {
                        self.load_more();
                    }
                }
                Err(e) => self.error.set(Some(e.to_string())),
            }
        });
    }
}

fn use_verse_feed(source: Memo<VerseSource>) -> VerseFeed {
    let settings = use_settings();
    let query = use_memo(move || settings.read().verse_query());
    // Read the context *before* creating the signal: calling a hook (use_context)
    // inside another hook's initializer panics in Dioxus ("hook list already borrowed").
    let api = crate::use_api();
    let client = use_signal(move || api);
    let feed = VerseFeed {
        source,
        loaded_for: use_signal(|| None),
        client,
        query,
        verses: use_signal(Vec::new),
        // Start "loading" so nothing claims the list is complete (or empty) before
        // the mount effect below has started the first request.
        next_page: use_signal(move || Some(source.peek().first_api_page())),
        loading: use_signal(|| true),
        error: use_signal(|| None),
        total: use_signal(|| None),
        merge_issues: use_signal(Vec::new),
        generation: use_signal(|| 0),
    };
    // Runs on mount and again whenever the verse query (translations, font,
    // layout) or the source (navigation to another page/surah/juz) changes.
    use_effect(move || {
        let _ = query.read();
        let _ = source.read();
        feed.reset();
    });
    feed
}

/// Open the feed at `start`: load until its verse is there, then scroll to it.
/// Otherwise every change of the verses is reported, and the position is
/// forgotten when the list goes away.
fn use_start_position(feed: VerseFeed, start: Anchor) {
    let mut pending = use_signal(|| None);
    // On mount, and when only the hash of the URL changes (a link, or one typed in):
    // unless the reader is already there, which is all a re-render with the
    // tracked position in the URL amounts to.
    use_effect(use_reactive!(|start| {
        if let Some(target) = start.0.filter(|t| (*position::POSITION.peek()).map(|p| p.at) != Some(*t)) {
            position::hold();
            pending.set(Some(target));
        }
    }));
    use_effect(move || {
        let loaded = feed.verses.read().len();
        let Some(target) = pending() else {
            if loaded > 0 {
                position::changed();
            }
            return;
        };
        if (feed.loading)() || *feed.loaded_for.read() != Some(*feed.source.peek()) {
            return;
        }
        let key = target.verse_key();
        if feed.verses.read().iter().any(|v| v.verse_key == key) {
            pending.set(None);
            spawn(async move {
                position::restore(target).await;
            });
        } else if (feed.next_page)().is_some() && (feed.error)().is_none() {
            feed.load_more();
        } else {
            pending.set(None);
            position::release();
        }
    });
    use_drop(|| *position::POSITION.write() = None);
}

/// Mushaf pages of a verse source with "Load more", auto-load near the bottom,
/// surah headers where a surah starts, and integrity warning banners.
///
/// `expected`: for a chapter source, every verse that must be present once all
/// pages have loaded (`1..=verses_count` or the requested range).
///
/// Mount it with a `key` derived from `source` so a new source gets a fresh feed.
#[component]
pub(crate) fn VerseList(
    source: VerseSource,
    /// `(chapter, span)` pairs that must all be present once everything has loaded.
    #[props(default)] expected: Vec<(u16, VerseSpan)>,
    /// Whether the first/last mushaf line may be cut (verse-range views).
    #[props(default)] trim: Trim,
    /// Reading position to open at.
    #[props(default)] start: Anchor,
) -> Element {
    let source = use_memo(use_reactive!(|source| source));
    let feed = use_verse_feed(source);
    use_start_position(feed, start);
    let expected = use_memo(use_reactive!(|expected| expected));

    // Integrity of what is loaded so far (completeness only once fully loaded).
    let issues = use_memo(move || {
        // The mushaf always shows Uthmani (QCF glyphs / KFGQPC Hafs text).
        let script_field = "text_uthmani";
        // Verses still from the previous source (navigation just happened): nothing to check yet.
        if *feed.loaded_for.read() != Some(source()) {
            return Vec::new();
        }
        let complete = (feed.next_page)().is_none() && !(feed.loading)() && (feed.error)().is_none();
        let expected_spans = expected.read();
        let expected: &[(u16, VerseSpan)] = if complete { &expected_spans } else { &[] };
        let mut issues = feed.merge_issues.read().clone();
        let verses = feed.verses.read();
        issues.extend(integrity::check_all(&verses, script_field, expected, reference::verses_in));
        // Lines: a mushaf page must be whole once it has loaded; elsewhere every
        // line between the first and the last one loaded.
        match source() {
            VerseSource::Page(p) if complete => issues.extend(integrity::line_issues(&verses, Some(p))),
            VerseSource::Page(_) => {}
            _ => issues.extend(integrity::line_issues(&verses, None)),
        }
        issues
    });
    use_effect(move || {
        let issues = issues.read();
        if !issues.is_empty() {
            let list: Vec<String> = issues.iter().map(ToString::to_string).collect();
            dioxus::logger::tracing::warn!("verse integrity issues ({:?}): {}", *source.peek(), list.join("; "));
        }
    });

    // Right after navigation (before the reset effect runs) the old verses are
    // still in the feed: render as loading rather than flash the previous content.
    let stale = *feed.loaded_for.read() != Some(source());
    let verses = feed.verses.read();
    let loading = (feed.loading)() || stale;
    let error = (feed.error)();
    let has_more = (feed.next_page)().is_some();
    let total = (feed.total)();
    // Pages/juz have no chapter span up front: once loading finishes, the number of
    // verses shown must match the API's own total for the page/juz.
    let complete = !has_more && !loading && error.is_none();
    let short_by = match total {
        Some(t) if complete && (verses.len() as u32) < t => Some((verses.len(), t)),
        _ => None,
    };

    let issues_now = issues.read();
    let fallback_keys = integrity::keys_where(&issues_now, IntegrityIssue::is_structural_word_issue);
    let empty_word_keys =
        integrity::keys_where(&issues_now, |i| matches!(i, IntegrityIssue::EmptyWordText { .. }));
    let layout_keys = integrity::keys_where(&issues_now, IntegrityIssue::is_layout_issue);
    let shape_keys = integrity::keys_where(&issues_now, IntegrityIssue::is_page_shape_issue);
    let glyph_keys =
        integrity::keys_where(&issues_now, |i| matches!(i, IntegrityIssue::GlyphPageMismatch { .. }));
    let missing_keys = integrity::keys_where(&issues_now, |i| matches!(i, IntegrityIssue::MissingVerse { .. }));
    let unknown_keys = integrity::keys_where(&issues_now, |i| matches!(i, IntegrityIssue::UnknownVerse { .. }));
    let short_lines: Vec<String> = issues_now
        .iter()
        .filter(|i| matches!(i, IntegrityIssue::LineWordCount { .. }))
        .map(ToString::to_string)
        .collect();
    drop(issues_now);
    let any_issue =
        [&fallback_keys, &empty_word_keys, &layout_keys, &shape_keys, &glyph_keys, &missing_keys, &unknown_keys, &short_lines]
        .iter()
        .any(|k| !k.is_empty());

    rsx! {
        if let Some((shown, total)) = short_by {
            div { class: "integrity-banner", role: "status",
                p { "Only {shown} of {total} verses were loaded; some verses may be missing." }
                button { class: "load-more", r#type: "button", onclick: move |_| feed.reset(), "Reload" }
            }
        }
        if any_issue {
            div { class: "integrity-banner", role: "status",
                if !fallback_keys.is_empty() {
                    p {
                        "Some verse data looked incomplete (words missing or out of place); showing full verse text instead for: "
                        {key_list(&fallback_keys)}
                    }
                }
                if !empty_word_keys.is_empty() {
                    p {
                        "Some words came without Unicode text (marked ⟨?⟩ unless the mushaf font draws them) in: "
                        {key_list(&empty_word_keys)}
                    }
                }
                if !layout_keys.is_empty() {
                    p {
                        "Mushaf line data was missing for these verses; they are shown as flowing text: "
                        {key_list(&layout_keys)}
                    }
                }
                if !shape_keys.is_empty() {
                    p {
                        "The mushaf page layout looked inconsistent near (all words are still shown): "
                        {key_list(&shape_keys)}
                    }
                }
                if !glyph_keys.is_empty() {
                    p {
                        "Some words are shown as Unicode text because their glyph font page did not match: "
                        {key_list(&glyph_keys)}
                    }
                }
                if !short_lines.is_empty() {
                    p {
                        "Some mushaf lines do not have all their words: "
                        {key_list(&short_lines)}
                        " "
                        button {
                            class: "link-button",
                            r#type: "button",
                            onclick: move |_| feed.reset(),
                            "Reload"
                        }
                    }
                }
                if !unknown_keys.is_empty() {
                    p {
                        "The server returned verse numbers that do not exist: "
                        {key_list(&unknown_keys)}
                    }
                }
                if !missing_keys.is_empty() {
                    p {
                        "Some verses were missing from the server response: "
                        {key_list(&missing_keys)}
                        " "
                        button {
                            class: "link-button",
                            r#type: "button",
                            onclick: move |_| feed.reset(),
                            "Reload"
                        }
                    }
                }
            }
        }
        if !stale {
            MushafView { verses: feed.verses, trim }
        }
        if let Some(message) = error.filter(|_| !stale) {
            div { class: "feed-error",
                ErrorBox { message: "Could not load verses: {message}" }
                button {
                    class: "load-more",
                    r#type: "button",
                    onclick: move |_| feed.load_more(),
                    "Try again"
                }
            }
        } else if loading {
            Loading {}
        } else if has_more {
            div {
                class: "load-more-wrap",
                // Auto-load when this comes into view; the button is the fallback.
                onvisible: move |e: VisibleEvent| {
                    if e.is_intersecting().unwrap_or(false) {
                        feed.load_more();
                    }
                },
                if let Some(total) = total {
                    p { class: "feed-progress", "{verses.len()} of {total} verses" }
                }
                button {
                    class: "load-more",
                    r#type: "button",
                    onclick: move |_| feed.load_more(),
                    "Load more"
                }
            }
        } else if verses.is_empty() {
            p { class: "feed-empty", "No verses found." }
        }
    }
}

/// "2:5, 2:7, … (+N more)" for the banner.
fn key_list(keys: &[String]) -> String {
    short_list(keys, 12)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn verse(key: &str) -> Verse {
        let n: u16 = key.split(':').nth(1).unwrap().parse().unwrap();
        serde_json::from_value(serde_json::json!({
            "id": 1, "verse_number": n, "verse_key": key
        }))
        .unwrap()
    }

    #[test]
    fn source_pages() {
        let s = VerseSource::Chapter { chapter: 2, span: Some(VerseSpan { from: 120, to: 160 }) };
        assert_eq!(s.first_api_page(), 3);
        assert_eq!(s.last_api_page(), Some(4));
        assert!(s.includes(&verse("2:120")) && s.includes(&verse("2:160")));
        assert!(!s.includes(&verse("2:119")) && !s.includes(&verse("2:161")));
        let whole = VerseSource::Chapter { chapter: 2, span: None };
        assert_eq!((whole.first_api_page(), whole.last_api_page()), (1, None));
        assert_eq!((VerseSource::Juz(30).first_api_page(), VerseSource::Juz(30).last_api_page()), (1, None));
        assert_eq!(VerseSource::Juz(30).next_api_page(1, Some(2)), Some(2));
        assert_eq!(VerseSource::Juz(30).next_api_page(2, Some(2)), None, "an API that repeats a page is not followed");
    }

    #[test]
    fn a_mushaf_page_is_loaded_from_its_listings() {
        // An ordinary page: its own listing, one request.
        let page = VerseSource::Page(3);
        assert_eq!((page.first_api_page(), page.last_api_page()), (1, Some(1)));
        assert_eq!(page.next_api_page(1, None), None);
        // 83:34 is printed on page 589 but listed under 588: two requests, and of
        // the first listing only that verse belongs to the page.
        let turn = VerseSource::Page(589);
        assert_eq!(turn.last_api_page(), Some(2));
        assert_eq!((turn.next_api_page(1, None), turn.next_api_page(2, None)), (Some(2), None));
        assert!(turn.includes(&verse("83:34")) && turn.includes(&verse("84:24")));
        // 83:33 is on page 588; 84:25 is in this page's listing but printed on page 590.
        assert!(!turn.includes(&verse("83:33")) && !turn.includes(&verse("84:25")));
    }
}
