//! Rendering of the 15-line mushaf: pages → lines → words (QCF glyphs or
//! Unicode fallback) with word-by-word meanings, surah headers, Bismillah lines
//! and optional per-page translations.
//!
//! DOM contract (used by the browser word checker):
//! - word: `span.word[data-location="c:v:p"][data-page][data-line][data-mode="glyph"|"text"]`
//!   containing `span.word-text` (+ `span.wbw-translation` / `span.wbw-transliteration`);
//!   words of flowing fallback blocks have no `data-page`/`data-line`.
//! - ayah end: `span.ayah-marker[data-location][data-verse-key][data-mode]` (not `.word`).
//! - verse: `span.verse-anchor#verse-{key}[data-verse-key][data-word-count]` before its first word.

use std::rc::Rc;

use dioxus::prelude::*;
use quran_api::{Chapter, Verse, VerseQuery};

use super::fonts::{self, FONTS, use_mushaf_fonts, use_page_font_ready};
use super::glyph::{self, MISSING_WORD, WordMode};
use super::mushaf_layout::{FlowReason, Item, PageBlock, Trim, WordCell, build_layout};
use super::render_audit;
use super::verse::{CopyVerseButton, strip_html};
use super::{BISMILLAH, CHAPTERS, chapter_route};
use crate::settings::wbw::TranslationText;
use crate::settings::{MushafFont, WbwDisplay, use_settings};

/// English meaning of the Bismillah when no translation is selected.
pub const BISMILLAH_MEANING: &str = "In the Name of Allah—the Most Compassionate, Most Merciful";

/// Verse 1:1 (whose four words draw the Bismillah line), cached per query.
static BISMILLAH_VERSE: GlobalSignal<Option<(VerseQuery, Verse)>> = Signal::global(|| None);

/// Word-by-word display options.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WbwOpts {
    pub translation: bool,
    pub transliteration: bool,
    pub tooltip: bool,
}

impl WbwOpts {
    fn any(self) -> bool {
        self.translation || self.transliteration
    }
    fn inline(self) -> bool {
        self.any() && !self.tooltip
    }
}

/// All mushaf pages for the loaded `verses` (sorted). Mount inside `VerseList`.
#[component]
pub(crate) fn MushafView(verses: ReadSignal<Vec<Verse>>, #[props(default)] trim: Trim) -> Element {
    let settings = use_settings();
    let api = crate::use_api();
    let trim = use_memo(use_reactive!(|trim| trim));
    let layout = use_memo(move || build_layout(&verses.read(), trim()));
    let query = use_memo(move || settings.read().verse_query());
    let need_bismillah = use_memo(move || layout.read().has_bismillah());

    let bismillah = use_resource(move || {
        let client = api.clone();
        let q = query();
        let need = need_bismillah();
        async move {
            if !need {
                return None;
            }
            if let Some((cached_q, v)) = BISMILLAH_VERSE.peek().as_ref()
                && *cached_q == q
            {
                return Some(v.clone());
            }
            match client.verse_by_key("1:1", &q).await {
                Ok(v) => {
                    *BISMILLAH_VERSE.write() = Some((q, v.clone()));
                    Some(v)
                }
                Err(e) => {
                    dioxus::logger::tracing::warn!("could not load verse 1:1 for the Bismillah line: {e}");
                    None
                }
            }
        }
    });
    let bismillah_verse = use_memo(move || bismillah.read().clone().flatten());

    // Fonts for every page shown (+ page 1 for the Bismillah glyphs).
    let glyph_pages = use_memo(move || {
        let mut pages = layout.read().glyph_pages();
        if let Some(v) = bismillah_verse.read().as_ref() {
            let glyphs: String =
                v.words.iter().filter(|w| !w.is_end_marker()).filter_map(glyph::glyph_code).collect();
            if !glyphs.is_empty() {
                match pages.iter_mut().find(|(p, _)| *p == 1) {
                    Some((_, all)) => all.push_str(&glyphs),
                    None => pages.push((1, glyphs)),
                }
            }
        }
        pages
    });
    use_mushaf_fonts(glyph_pages);

    let font = use_memo(move || settings.read().mushaf_font);
    let wbw = use_memo(move || {
        let s = settings.read();
        WbwOpts {
            translation: s.word_by_word.show_translation,
            transliteration: s.word_by_word.show_transliteration,
            tooltip: s.word_by_word.display == WbwDisplay::Tooltip,
        }
    });
    let show_translations = use_memo(move || settings.read().show_page_translation);
    let failed = use_memo(move || {
        let pages: Vec<u16> = glyph_pages.read().iter().map(|(p, _)| *p).collect();
        FONTS.read().failed_pages(font(), &pages)
    });
    let palette = use_memo(move || fonts::palette_css(font(), &FONTS.read().loaded_pages(font())));

    // Borrow the cached chapter list instead of cloning all 114 chapters per render.
    let chapters_guard = CHAPTERS.read();
    let chapters: &[Chapter] = chapters_guard.as_ref().map(|(_, list)| list.as_slice()).unwrap_or(&[]);
    let layout = layout.read();
    let verses = verses.read();
    let failed = failed();

    rsx! {
        if !palette.read().is_empty() {
            style { {palette()} }
        }
        if !failed.is_empty() {
            div { class: "font-banner", role: "status",
                p {
                    "The mushaf font could not be loaded for page{plural(failed.len())} {page_list(&failed)}; "
                    "showing the standard Uthmani text there instead. "
                    button {
                        class: "link-button",
                        r#type: "button",
                        onclick: move |_| fonts::retry_failed(),
                        "Retry"
                    }
                }
            }
        }
        div { class: if wbw().inline() { "mushaf-pages wbw-inline" } else { "mushaf-pages" },
            for (i, block) in layout.pages.iter().enumerate() {
                PageView {
                    key: "{block_key(&layout.pages, i)}",
                    block_id: block_key(&layout.pages, i),
                    block: Rc::clone(block),
                    headers: headers_for(block, chapters),
                    bismillah: if block_has_bismillah(block) { bismillah_verse() } else { None },
                    translations: if show_translations() { verses_for(block, &verses) } else { Vec::new() },
                    wbw: wbw(),
                    font: font(),
                }
            }
        }
    }
}

fn flow_note(reason: FlowReason, verse_key: &str) -> String {
    match reason {
        FlowReason::NoLayout => format!("Mushaf line layout unavailable for {verse_key}; shown as flowing text."),
        FlowReason::VerseText => format!("Word data incomplete for {verse_key}; showing the full verse text."),
    }
}

fn render_error_list(errors: &std::collections::BTreeMap<String, String>) -> String {
    const SHOWN: usize = 12;
    let mut s =
        errors.iter().take(SHOWN).map(|(loc, why)| format!("{loc} ({why})")).collect::<Vec<_>>().join(", ");
    if errors.len() > SHOWN {
        s.push_str(&format!(" (+{} more)", errors.len() - SHOWN));
    }
    s
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

fn page_list(pages: &[u16]) -> String {
    const SHOWN: usize = 10;
    let mut s = pages.iter().take(SHOWN).map(u16::to_string).collect::<Vec<_>>().join(", ");
    if pages.len() > SHOWN {
        s.push_str(&format!(" (+{} more)", pages.len() - SHOWN));
    }
    s
}

/// Stable key: page number plus how many earlier blocks had the same page.
fn block_key(pages: &[Rc<PageBlock>], i: usize) -> String {
    let page = pages[i].page;
    let seen = pages[..i].iter().filter(|b| b.page == page).count();
    match page {
        Some(p) => format!("p{p}-{seen}"),
        None => format!("x-{seen}"),
    }
}

fn block_has_bismillah(block: &PageBlock) -> bool {
    block.items.iter().any(|i| matches!(i, Item::Bismillah { .. }))
}

fn headers_for(block: &PageBlock, chapters: &[Chapter]) -> Vec<Chapter> {
    block
        .items
        .iter()
        .filter_map(|i| match i {
            Item::SurahHeader { chapter } => chapters.iter().find(|c| c.id == *chapter).cloned(),
            _ => None,
        })
        .collect()
}

fn verses_for(block: &PageBlock, verses: &[Verse]) -> Vec<Verse> {
    block.verse_keys.iter().filter_map(|k| verses.iter().find(|v| &v.verse_key == k).cloned()).collect()
}

#[component]
fn PageView(
    block_id: String,
    block: Rc<PageBlock>,
    headers: Vec<Chapter>,
    bismillah: Option<Verse>,
    translations: Vec<Verse>,
    wbw: WbwOpts,
    font: MushafFont,
) -> Element {
    let ready = use_page_font_ready(font, block.page);
    let bismillah_ready = use_page_font_ready(font, Some(1));
    let family = block.page.map(|p| font.family(p)).unwrap_or_default();
    let page_attr = block.page.map(|p| p.to_string());

    // Built-in render audit (see render_audit.rs): words that failed it are
    // forced to plain Unicode text and get a visible error marker.
    let mut visible = use_signal(|| false);
    let mut render_errors = use_signal(std::collections::BTreeMap::<String, String>::new);
    let errors_now = render_errors.read().clone();
    let mode_of = |c: &WordCell| {
        let failed = errors_now.contains_key(&c.location);
        let ready = ready && !failed && c.page.is_some() && c.page == block.page;
        glyph::choose_mode(c.glyph.as_deref(), c.text.as_deref(), ready, &family)
    };
    let expected = render_audit::expectations(&block, mode_of);
    {
        let block_id = block_id.clone();
        use_effect(use_reactive!(|expected| {
            if !visible() {
                return;
            }
            let block_id = block_id.clone();
            spawn(async move {
                let Some(failures) = render_audit::run(&block_id, &expected).await else { return };
                let mut changed = false;
                let mut map = render_errors.peek().clone();
                for render_audit::Failure(loc, reason) in failures {
                    if map.get(&loc) != Some(&reason) {
                        if !map.contains_key(&loc) {
                            dioxus::logger::tracing::error!("word {loc} did not render correctly: {reason}");
                        }
                        map.insert(loc, reason);
                        changed = true;
                    }
                }
                if changed {
                    render_errors.set(map);
                }
            });
        }));
    }

    rsx! {
        section {
            class: "mushaf-page",
            "data-page": page_attr.clone(),
            "data-block": "{block_id}",
            onvisible: move |e: VisibleEvent| {
                let now = e.is_intersecting().unwrap_or(false);
                if *visible.peek() != now {
                    visible.set(now);
                }
            },
            if !errors_now.is_empty() {
                div { class: "render-error-banner", role: "alert", dir: "ltr", lang: "en",
                    strong { "⚠ {errors_now.len()} word{plural(errors_now.len())} on this page did not display correctly" }
                    {if errors_now.len() == 1 { " and is now shown as plain text, marked ⚠: " } else { " and are now shown as plain text, marked ⚠: " }}
                    {render_error_list(&errors_now)}
                }
            }
            div { class: "mushaf-page-scroll",
                div { class: "mushaf-lines", dir: "rtl", lang: "ar",
                    for (i, item) in block.items.iter().enumerate() {
                        match item {
                            Item::SurahHeader { chapter } => rsx! {
                                SurahHeaderLine {
                                    key: "h{i}",
                                    chapter: *chapter,
                                    info: headers.iter().find(|c| c.id == *chapter).cloned(),
                                }
                            },
                            Item::Bismillah { chapter } => rsx! {
                                BismillahLine {
                                    key: "b{i}",
                                    chapter: *chapter,
                                    verse: bismillah.clone(),
                                    ready: bismillah_ready,
                                    font,
                                }
                            },
                            Item::Line { line, cells, align } => rsx! {
                                div {
                                    key: "l{i}",
                                    class: "mushaf-line {align.class()}",
                                    "data-page": page_attr.clone(),
                                    "data-line": "{line}",
                                    for c in cells.iter() {
                                        Fragment { key: "{c.location}", {mushaf_word(c, mode_of(c), wbw, errors_now.get(&c.location))} }
                                    }
                                }
                            },
                            Item::Flow { verse_key, reason, cells } => rsx! {
                                div {
                                    key: "f{i}",
                                    class: "mushaf-flow",
                                    "data-verse-key": "{verse_key}",
                                    "data-fallback": match reason {
                                        FlowReason::NoLayout => "layout",
                                        FlowReason::VerseText => "verse-text",
                                    },
                                    p { class: "mushaf-flow-note", dir: "ltr", lang: "en",
                                        {flow_note(*reason, verse_key)}
                                    }
                                    div { class: "mushaf-line flowing",
                                        for c in cells.iter() {
                                            Fragment { key: "{c.location}", {mushaf_word(c, mode_of(c), wbw, errors_now.get(&c.location))} }
                                        }
                                    }
                                }
                            },
                        }
                    }
                }
            }
            if let Some(p) = block.page {
                footer { class: "mushaf-page-number",
                    Link { to: crate::Route::MushafPage { page: p }, title: "Open page {p}", "{p}" }
                }
            }
            if !translations.is_empty() {
                PageTranslations { verses: translations }
            }
        }
    }
}

/// One word or ayah-end marker, plus the verse anchor before a verse's first word.
///
/// A plain function, not a component: a juz has thousands of words, and a scope
/// plus a signal per word cost memory for no benefit. The tooltip (hover mode) is
/// CSS-only: shown on hover and on focus (tap or Tab focuses the word).
fn mushaf_word(cell: &WordCell, mode: WordMode, wbw: WbwOpts, render_error: Option<&String>) -> Element {
    // Visible marker for a word that failed the render audit (outside `.word-text`,
    // in a plain system font, so it can't be hidden by the problem it reports).
    let error_badge = render_error.map(|reason| {
        rsx! {
            span {
                class: "word-render-error",
                role: "img",
                lang: "en",
                dir: "ltr",
                title: "This word did not display correctly ({reason}); shown as plain text.",
                "aria-label": "display problem: {reason}",
                "⚠"
            }
        }
    });
    let page = cell.page.map(|p| p.to_string());
    let line = cell.line.map(|l| l.to_string());
    let anchor = cell.anchor.map(|count| {
        rsx! {
            span {
                class: "verse-anchor",
                id: "verse-{cell.verse_key}",
                "data-verse-key": "{cell.verse_key}",
                "data-word-count": "{count}",
                "aria-hidden": "true",
            }
        }
    });
    let text = match &mode {
        WordMode::Glyph { glyph, family } => rsx! {
            span { class: "word-text glyph-text", style: "font-family: '{family}'", "{glyph}" }
        },
        // Ayah end without its medallion glyph: a styled Unicode marker ﴿٢٥٥﴾.
        _ if cell.is_end => rsx! {
            span { class: "word-text ayah-text", "{glyph::unicode_ayah_marker(cell.verse_number)}" }
        },
        WordMode::Text(t) => rsx! {
            span { class: "word-text unicode-text", "{t}" }
        },
        WordMode::Missing => rsx! {
            span { class: "word-text", title: "word text unavailable", "{MISSING_WORD}" }
        },
    };

    if cell.is_end {
        let n = cell.verse_number;
        return rsx! {
            {anchor}
            span {
                class: "ayah-marker",
                "data-location": "{cell.location}",
                "data-verse-key": "{cell.verse_key}",
                "data-page": page,
                "data-line": line,
                "data-mode": mode.attr(),
                role: "img",
                "aria-label": "end of verse {n}",
                {error_badge}
                {text}
                if wbw.inline() && wbw.translation {
                    span { class: "ayah-number", lang: "en", dir: "ltr", "({n})" }
                }
            }
        };
    }

    let translation = cell.translation.clone().filter(|_| wbw.translation);
    let transliteration = cell.transliteration.clone().filter(|_| wbw.transliteration);
    let extras = rsx! {
        if let Some(tr) = transliteration {
            span { class: "wbw-transliteration", lang: "en", dir: "ltr", "{tr}" }
        }
        if let Some(tr) = translation {
            span { class: "wbw-translation", lang: "en", dir: "ltr", "{tr}" }
        }
    };
    let missing = matches!(mode, WordMode::Missing);
    let has_extras = wbw.any() && (cell.translation.is_some() || cell.transliteration.is_some());

    if wbw.tooltip && has_extras {
        let tip_id = format!("wbw-tip-{}", cell.location.replace(':', "-"));
        let class = if missing { "word word-missing has-tooltip" } else { "word has-tooltip" };
        return rsx! {
            {anchor}
            span {
                class,
                "data-location": "{cell.location}",
                "data-page": page,
                "data-line": line,
                "data-mode": mode.attr(),
                tabindex: 0,
                "aria-describedby": "{tip_id}",
                {error_badge}
                {text}
                span { class: "word-tooltip", id: "{tip_id}", role: "tooltip", {extras} }
            }
        };
    }

    rsx! {
        {anchor}
        span {
            class: if missing { "word word-missing" } else { "word" },
            "data-location": "{cell.location}",
            "data-page": page,
            "data-line": line,
            "data-mode": mode.attr(),
            {error_badge}
            {text}
            if wbw.inline() {
                {extras}
            }
        }
    }
}

/// Ornamental surah title line (CSS frame) shown before a surah's first line.
#[component]
fn SurahHeaderLine(chapter: u16, info: Option<Chapter>) -> Element {
    let to = chapter_route(chapter, None);
    let (arabic, label) = match &info {
        Some(c) => (format!("سورة {}", c.name_arabic), format!("Surah {}", c.name_simple)),
        None => (String::new(), format!("Surah {chapter}")),
    };
    rsx! {
        div { class: "surah-header-line", "data-chapter": "{chapter}",
            Link { class: "surah-header-frame", to, title: "{label}", "aria-label": "{label}",
                if arabic.is_empty() {
                    span { class: "surah-header-fallback", lang: "en", dir: "ltr", "{label}" }
                } else {
                    span { class: "surah-header-arabic", lang: "ar", dir: "rtl", "{arabic}" }
                }
            }
        }
    }
}

/// The Bismillah line: verse 1:1's four glyphs in the page-1 font when available,
/// else its Unicode text, with the English meaning beneath.
#[component]
fn BismillahLine(chapter: u16, verse: Option<Verse>, ready: bool, font: MushafFont) -> Element {
    let words: Vec<&quran_api::Word> =
        verse.as_ref().map(|v| v.words.iter().filter(|w| !w.is_end_marker()).collect()).unwrap_or_default();
    let glyphs: Vec<String> = words.iter().filter_map(|w| glyph::glyph_code(w)).collect();
    let use_glyphs = ready && !glyphs.is_empty() && glyphs.len() == words.len();
    let unicode: String = {
        let joined = words.iter().filter_map(|w| glyph::unicode_text(w)).collect::<Vec<_>>().join(" ");
        if joined.trim().is_empty() { BISMILLAH.to_string() } else { joined }
    };
    let meaning = verse
        .as_ref()
        .and_then(|v| v.translations.first())
        .map(|t| strip_html(&t.text))
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| BISMILLAH_MEANING.to_string());
    let family = font.family(1);
    rsx! {
        div { class: "bismillah-line", "data-chapter": "{chapter}",
            div {
                class: "bismillah-text",
                dir: "rtl",
                lang: "ar",
                "data-mode": if use_glyphs { "glyph" } else { "text" },
                if use_glyphs {
                    for (i, g) in glyphs.into_iter().enumerate() {
                        span { key: "{i}", class: "bismillah-word glyph-text", style: "font-family: '{family}'", "{g}" }
                    }
                } else {
                    span { class: "bismillah-word unicode-text", "{unicode}" }
                }
            }
            p { class: "bismillah-meaning", lang: "en", "{meaning}" }
        }
    }
}

/// The page's verse translations (verses starting on this page), with copy actions.
#[component]
fn PageTranslations(verses: Vec<Verse>) -> Element {
    rsx! {
        div { class: "page-translations",
            for v in verses.into_iter() {
                div { key: "{v.verse_key}", class: "page-translation", "data-verse-key": "{v.verse_key}",
                    div { class: "page-translation-head",
                        Link {
                            class: "verse-key",
                            to: crate::Route::ChapterRange {
                                chapter: v.chapter_number().unwrap_or(0).to_string(),
                                range: v.verse_number.to_string(),
                            },
                            "{v.verse_key}"
                        }
                        CopyVerseButton { verse: v.clone() }
                    }
                    if v.translations.is_empty() {
                        p { class: "translation-missing", "No translation selected." }
                    }
                    for t in v.translations.iter().cloned() {
                        TranslationText { key: "{t.resource_id}", translation: t }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(is_end: bool) -> WordCell {
        WordCell {
            location: "114:1:2".into(),
            verse_key: "114:1".into(),
            verse_number: 1,
            page: Some(604),
            line: Some(2),
            is_end,
            glyph: Some("\u{FB51}".into()),
            text: Some("بِرَبِّ".into()),
            translation: Some("in (the) Lord".into()),
            transliteration: None,
            anchor: None,
        }
    }

    fn html(cell: &WordCell, mode: WordMode, tooltip: bool, error: Option<&String>) -> String {
        let wbw = WbwOpts { translation: true, transliteration: false, tooltip };
        dioxus_ssr::render_element(mushaf_word(cell, mode, wbw, error))
    }

    #[test]
    fn audit_failure_shows_a_visible_marker_and_keeps_the_text() {
        let reason = "zero size".to_string();
        for tooltip in [false, true] {
            let out = html(&cell(false), WordMode::Text("بِرَبِّ".into()), tooltip, Some(&reason));
            assert!(out.contains("word-render-error"), "{out}");
            assert!(out.contains("⚠") && out.contains("zero size"), "{out}");
            // The word's own text is still there, in its own element.
            assert!(out.contains(r#"<span class="word-text unicode-text">بِرَبِّ</span>"#), "{out}");
        }
        let marker = html(&cell(true), WordMode::Text("x".into()), false, Some(&reason));
        assert!(marker.contains("ayah-marker") && marker.contains("word-render-error"), "{marker}");
        // No failure → no marker.
        let ok = html(&cell(false), WordMode::Text("بِرَبِّ".into()), false, None);
        assert!(!ok.contains("word-render-error"), "{ok}");
    }
}
