//! Paged verse loading shared by the chapter, mushaf-page and juz views.
//!
//! A feed fetches 50 verses per API page, appends them on "Load more" (or
//! automatically when the bottom sentinel scrolls into view), and resets
//! whenever the verse-related settings change. Loaded verses are drawn as
//! 15-line mushaf pages by `mushaf_view::MushafView`.

use dioxus::prelude::*;
use quran_api::{Client, Verse, VerseQuery, VersesResponse};

use super::integrity::{self, IntegrityIssue};
use super::mushaf_layout::Trim;
use super::mushaf_view::MushafView;
use super::parse::{PER_PAGE, VerseSpan, api_page_of, api_pages_for, next_page_within};
use super::use_chapters;
use crate::layout::{ErrorBox, Loading};
use crate::settings::use_settings;

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

    /// Last API page worth fetching, when known up front.
    fn last_api_page(self) -> Option<u32> {
        match self {
            VerseSource::Chapter { span: Some(s), .. } => Some(*api_pages_for(s, PER_PAGE).end()),
            _ => None,
        }
    }

    fn includes(self, verse: &Verse) -> bool {
        match self {
            VerseSource::Chapter { span: Some(s), .. } => s.contains(verse.verse_number),
            _ => true,
        }
    }

    async fn fetch(self, client: &Client, q: &VerseQuery) -> Result<VersesResponse, quran_api::Error> {
        match self {
            VerseSource::Chapter { chapter, .. } => client.verses_by_chapter(chapter, q).await,
            VerseSource::Page(p) => client.verses_by_page(p, q).await,
            VerseSource::Juz(j) => client.verses_by_juz(j, q).await,
        }
    }
}

/// Reactive state of a verse feed. `Copy`, so it can move into handlers freely.
#[derive(Clone, Copy)]
struct VerseFeed {
    source: VerseSource,
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
        self.next_page.set(Some(self.source.first_api_page()));
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
        let source = self.source;
        spawn(async move {
            let result = source.fetch(&client, &q).await;
            if *self.generation.peek() != generation {
                return; // settings changed meanwhile; a fresh request is in flight
            }
            self.loading.set(false);
            match result {
                Ok(resp) => {
                    let next = next_page_within(resp.pagination.next_page, source.last_api_page())
                        // Guard against an API that keeps returning the same page.
                        .filter(|n| *n > page && !resp.verses.is_empty());
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
                }
                Err(e) => self.error.set(Some(e.to_string())),
            }
        });
    }
}

fn use_verse_feed(source: VerseSource) -> VerseFeed {
    let settings = use_settings();
    let query = use_memo(move || settings.read().verse_query());
    // Read the context *before* creating the signal: calling a hook (use_context)
    // inside another hook's initializer panics in Dioxus ("hook list already borrowed").
    let api = crate::use_api();
    let client = use_signal(move || api);
    let feed = VerseFeed {
        source,
        client,
        query,
        verses: use_signal(Vec::new),
        // Start "loading" so nothing claims the list is complete (or empty) before
        // the mount effect below has started the first request.
        next_page: use_signal(move || Some(source.first_api_page())),
        loading: use_signal(|| true),
        error: use_signal(|| None),
        total: use_signal(|| None),
        merge_issues: use_signal(Vec::new),
        generation: use_signal(|| 0),
    };
    // Runs on mount and again whenever the verse query (translations, script,
    // language) changes.
    use_effect(move || {
        let _ = query.read();
        feed.reset();
    });
    feed
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
) -> Element {
    let feed = use_verse_feed(source);
    let chapters = use_chapters();
    let expected = use_memo(use_reactive!(|expected| expected));

    // Integrity of what is loaded so far (completeness only once fully loaded).
    let issues = use_memo(move || {
        // The mushaf always shows Uthmani (QCF glyphs / KFGQPC Hafs text).
        let script_field = "text_uthmani";
        let complete = (feed.next_page)().is_none() && !(feed.loading)() && (feed.error)().is_none();
        let expected_spans = expected.read();
        let expected: &[(u16, VerseSpan)] = if complete { &expected_spans } else { &[] };
        let counts: Vec<(u16, u16)> = match &*chapters.read() {
            Some(Ok(list)) => list.iter().map(|c| (c.id, c.verses_count)).collect(),
            _ => Vec::new(),
        };
        let verses_count = |c: u16| counts.iter().find(|(id, _)| *id == c).map(|(_, n)| *n);
        let mut issues = feed.merge_issues.read().clone();
        issues.extend(integrity::check_all(&feed.verses.read(), script_field, expected, verses_count));
        issues
    });
    use_effect(move || {
        let issues = issues.read();
        if !issues.is_empty() {
            let list: Vec<String> = issues.iter().map(ToString::to_string).collect();
            dioxus::logger::tracing::warn!("verse integrity issues ({:?}): {}", source, list.join("; "));
        }
    });

    let verses = feed.verses.read();
    let loading = (feed.loading)();
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
    drop(issues_now);
    let any_issue = [&fallback_keys, &empty_word_keys, &layout_keys, &shape_keys, &glyph_keys, &missing_keys]
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
                        "Some verse data looked incomplete; showing full verse text instead for: "
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
        MushafView { verses: feed.verses, trim }
        if let Some(message) = error {
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
    const SHOWN: usize = 12;
    let mut s = keys.iter().take(SHOWN).cloned().collect::<Vec<_>>().join(", ");
    if keys.len() > SHOWN {
        s.push_str(&format!(" (+{} more)", keys.len() - SHOWN));
    }
    s
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
    fn key_list_truncates() {
        let keys: Vec<String> = (1..=14).map(|n| format!("2:{n}")).collect();
        assert_eq!(key_list(&keys[..2]), "2:1, 2:2");
        assert!(key_list(&keys).ends_with("2:12 (+2 more)"));
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
        assert_eq!((VerseSource::Juz(30).first_api_page(), VerseSource::Page(3).last_api_page()), (1, None));
    }
}
