//! Rendering of the 15-line mushaf: pages → lines → words (QCF glyphs or
//! Unicode fallback) with word-by-word meanings, surah banners, Bismillah lines
//! and optional per-page translations.
//!
//! DOM contract (used by the render audit and the browser word checker):
//! - page: `section.mushaf-page[data-page][data-block]` > `div.mushaf-lines`, whose
//!   children are the lines, banners and Bismillah lines.
//! - line: `div.mushaf-line[data-line]`; a verse without line data is a
//!   `div.mushaf-flow[data-verse-key]` instead.
//! - word: `span.word[data-location="c:v:p"][data-page][data-line][data-mode="glyph"|"text"]`.
//!   Its Arabic text is its own first text node; meanings are child `span.wbw`.
//!   Words of flowing blocks have no `data-page`/`data-line`.
//! - ayah end: `span.ayah-marker[data-location][data-verse-key][data-mode]` (not `.word`).
//! - verse: its first word (or marker) has `id="verse-{key}"` and `data-word-count`.
//! - `div.surah-banner[data-chapter]`, `p.bismillah[data-chapter]`.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::rc::Rc;

use dioxus::prelude::*;
use quran_api::{Chapter, Verse};

use super::fonts::{self, BANNER_UNITS, BannerFonts, BannerMetrics, FONTS, FontSet, use_mushaf_fonts, use_page_font_ready};
use super::glyph::{self, WordMode};
use super::mushaf_layout::{FlowReason, Item, PageBlock, Trim, WordCell, build_layout};
use super::render_audit::Audit;
use super::verse::{CopyVerseButton, strip_html};
use super::{parse, render_audit, use_chapters};
use crate::Route;
use crate::settings::translation::TranslationText;
use crate::settings::{WbwDisplay, use_settings};
use crate::util::short_list;

/// English meaning of the Bismillah when no translation is selected.
pub const BISMILLAH_MEANING: &str = "In the Name of Allah—the Most Compassionate, Most Merciful";

/// Unicode Bismillah, the last-resort text of the Bismillah line.
const BISMILLAH: &str = "بِسْمِ ٱللَّهِ ٱلرَّحْمَٰنِ ٱلرَّحِيمِ";

/// How often, and how long apart, the audit of a page that isn't rendered yet is retried.
const AUDIT_TRIES: u32 = 20;
const AUDIT_RETRY_MS: u32 = 150;

/// Verse 1:1 (whose four words draw the Bismillah line), cached per query.
static BISMILLAH_VERSE: GlobalSignal<Option<(quran_api::VerseQuery, Verse)>> = Signal::global(|| None);

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
    let chapters = use_chapters();
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
    let set = use_memo(move || {
        let s = settings.read();
        fonts::font_set(s.mushaf_font, s.dark())
    });
    use_mushaf_fonts(glyph_pages, set);

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
        set().map(|s| FONTS.read().failed_pages(s, &pages)).unwrap_or_default()
    });
    let palette =
        use_memo(move || {
            let dark = fonts::is_dark(settings.read().dark()).unwrap_or(false);
            set().map(|s| fonts::palette_css(s, &FONTS.read().loaded_pages(s), dark)).unwrap_or_default()
        });

    // Text size, spacing and the content width change the room for a line without re-rendering: refit.
    let sizes = use_memo(move || {
        let s = settings.read();
        (s.arabic_font_scale, s.translation_font_scale, s.word_spacing, s.line_spacing, s.content_width)
    });
    use_effect(move || {
        sizes();
        render_audit::fit_all();
    });

    let chapters = chapters.read();
    let chapters: &[Chapter] = chapters.as_ref().and_then(|r| r.as_ref().ok()).map_or(&[], |list| list.as_slice());
    let layout = layout.read();
    let verses = verses.read();
    let failed = failed();

    rsx! {
        if !palette.read().is_empty() {
            style { {palette()} }
        }
        if !failed.is_empty() {
            p { class: "font-banner", role: "status",
                "The mushaf font could not be loaded for page{plural(failed.len())} {short_list(&failed, 10)}; "
                "showing the standard Uthmani text there instead. "
                button { class: "link-button", r#type: "button", onclick: move |_| fonts::retry_failed(), "Retry" }
            }
        }
        for (i , block) in layout.pages.iter().enumerate() {
            PageView {
                key: "{block_key(&layout.pages, i)}",
                block_id: block_key(&layout.pages, i),
                block: Rc::clone(block),
                headers: headers_for(block, chapters),
                bismillah: if block_has_bismillah(block) { bismillah_verse() } else { None },
                translations: if show_translations() { verses_for(block, &verses) } else { Vec::new() },
                wbw: wbw(),
                set: set(),
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

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
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
    set: Option<FontSet>,
) -> Element {
    let ready = use_page_font_ready(set, block.page);
    let bismillah_ready = use_page_font_ready(set, Some(1));
    let family = set.zip(block.page).map(|(s, p)| s.family(p)).unwrap_or_default();
    let page_attr = block.page.map(|p| p.to_string());

    // Words that failed the render audit are forced to plain Unicode text and
    // get a visible error marker.
    let mut visible = use_signal(|| false);
    let mut render_errors = use_signal(BTreeMap::<String, String>::new);
    let errors_now = render_errors.read().clone();
    let mode_of = |c: &WordCell| {
        let failed = errors_now.contains_key(&c.location);
        glyph::choose_mode(c, ready && !failed && c.page.is_some() && c.page == block.page, &family)
    };
    let expected = render_audit::expectations(&block, mode_of);
    // Bumped whenever the page changes, so an audit of an older state is dropped.
    let audit_run = use_hook(|| Rc::new(Cell::new(0u32)));
    // After every change to what the page shows: fit its lines, and audit it while on screen.
    use_effect(use_reactive!(|expected, wbw, block_id| {
        let _ = wbw;
        let run = audit_run.get().wrapping_add(1);
        audit_run.set(run);
        if !visible() {
            document::eval(&render_audit::fit_js(&block_id));
            return;
        }
        let audit_run = audit_run.clone();
        spawn(async move {
            let mut failures = None;
            for _ in 0..AUDIT_TRIES {
                match render_audit::run(&block_id, &expected).await {
                    Audit::Done(found) => failures = Some(found),
                    Audit::Later => crate::util::sleep_ms(AUDIT_RETRY_MS).await,
                    Audit::Unavailable => return,
                }
                // The page changed meanwhile: its own, newer audit is under way.
                if audit_run.get() != run {
                    return;
                }
                if failures.is_some() {
                    break;
                }
            }
            let Some(failures) = failures else { return };
            let mut map = render_errors.peek().clone();
            let mut changed = false;
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

    let error_list: Vec<String> = errors_now.iter().map(|(loc, why)| format!("{loc} ({why})")).collect();
    let (s, verb) = if error_list.len() == 1 { ("", "is") } else { ("s", "are") };

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
            if !error_list.is_empty() {
                p { class: "render-error-banner", role: "alert",
                    strong { "⚠ {error_list.len()} word{s} on this page did not display correctly" }
                    " and {verb} now shown as plain text, marked ⚠: {short_list(&error_list, 12)}"
                }
            }
            div {
                class: if wbw.inline() { "mushaf-lines wbw-inline" } else { "mushaf-lines" },
                dir: "rtl",
                lang: "ar",
                for (i , item) in block.items.iter().enumerate() {
                    match item {
                        Item::SurahHeader { chapter } => rsx! {
                            SurahBanner {
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
                                family: set.filter(|_| bismillah_ready).map(|s| s.family(1)),
                            }
                        },
                        Item::Line { line, cells, align } => rsx! {
                            div {
                                key: "l{i}",
                                class: "mushaf-line {align.class()}",
                                "data-line": "{line}",
                                for c in cells.iter() {
                                    {mushaf_word(c, mode_of(c), wbw, errors_now.get(&c.location))}
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
                                p { class: "mushaf-flow-note", dir: "ltr", lang: "en", {flow_note(*reason, verse_key)} }
                                for c in cells.iter() {
                                    {mushaf_word(c, mode_of(c), wbw, errors_now.get(&c.location))}
                                }
                            }
                        },
                    }
                }
            }
            if let Some(p) = block.page {
                Link { class: "mushaf-page-number", to: Route::page(p), title: "Open page {p}", "{p}" }
            }
            for v in translations {
                VerseTranslations { key: "{v.verse_key}", verse: v }
            }
        }
    }
}

/// One word or ayah-end marker: a single element whose first child is the
/// Arabic text node, followed by its meanings (or tooltip) and, if the word
/// failed the render audit, a visible marker.
///
/// A plain function, not a component: a juz has thousands of words, and a scope
/// plus a signal per word cost memory for no benefit. The tooltip (hover mode) is
/// CSS-only: shown on hover and on focus (tap or Tab focuses the word).
fn mushaf_word(cell: &WordCell, mode: WordMode, wbw: WbwOpts, render_error: Option<&String>) -> Element {
    let style = match &mode {
        WordMode::Glyph { family, .. } => Some(format!("font-family: '{family}'")),
        _ => None,
    };
    let page = cell.page.map(|p| p.to_string());
    let line = cell.line.map(|l| l.to_string());
    // A verse's first cell is its anchor (link target, and the checker's word count).
    let id = cell.anchor.map(|_| format!("verse-{}", cell.verse_key));
    let word_count = cell.anchor.map(|n| n.to_string());
    // In a plain system font, so it can't be hidden by the problem it reports.
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

    if cell.is_end {
        let n = cell.verse_number;
        return rsx! {
            span {
                key: "{cell.location}",
                class: "ayah-marker {mode.class()}",
                style,
                id,
                "data-word-count": word_count,
                "data-location": "{cell.location}",
                "data-verse-key": "{cell.verse_key}",
                "data-page": page,
                "data-line": line,
                "data-mode": mode.attr(),
                role: "img",
                "aria-label": "end of verse {n}",
                "{mode.text()}"
                if wbw.inline() && wbw.translation {
                    span { class: "wbw", lang: "en", dir: "ltr", "({n})" }
                }
                {error_badge}
            }
        };
    }

    let translation = cell.translation.as_deref().filter(|_| wbw.translation);
    let transliteration = cell.transliteration.as_deref().filter(|_| wbw.transliteration);
    let tooltip = wbw.tooltip && (translation.is_some() || transliteration.is_some());
    let tip_id = tooltip.then(|| format!("wbw-tip-{}", cell.location.replace(':', "-")));
    let extras = rsx! {
        if let Some(tr) = transliteration {
            span { class: "wbw translit", lang: "en", dir: "ltr", "{tr}" }
        }
        if let Some(tr) = translation {
            span { class: "wbw", lang: "en", dir: "ltr", "{tr}" }
        }
    };

    rsx! {
        span {
            key: "{cell.location}",
            class: if tooltip { "word {mode.class()} has-tooltip" } else { "word {mode.class()}" },
            style,
            id,
            "data-word-count": word_count,
            "data-location": "{cell.location}",
            "data-page": page,
            "data-line": line,
            "data-mode": mode.attr(),
            tabindex: tooltip.then_some("0"),
            "aria-describedby": tip_id.clone(),
            "{mode.text()}"
            if let Some(tip_id) = &tip_id {
                span { class: "word-tooltip", id: "{tip_id}", role: "tooltip", {extras} }
            } else if wbw.inline() {
                {extras}
            }
            {error_badge}
        }
    }
}

/// Where things sit inside the banner frame, as fractions of the frame's ink
/// box (measured from the QUL `header` glyph): the name cartouche in the middle
/// and the two roundels beside it.
mod frame {
    /// Usable inside of the cartouche: `(x0, y0, x1, y1)`.
    pub const CARTOUCHE: (f64, f64, f64, f64) = (0.285, 0.17, 0.715, 0.83);
    /// Roundel centres (left, right) and their common centre line.
    pub const ROUNDEL_X: (f64, f64) = (0.206, 0.793);
    pub const ROUNDEL_Y: f64 = 0.496;
    /// Inner radius of a roundel, as a fraction of the frame HEIGHT.
    pub const ROUNDEL_R: f64 = 0.283;
}

/// The banner as SVG geometry, in the frame font's units.
#[derive(Debug, Clone, PartialEq)]
struct BannerArt {
    width: f64,
    height: f64,
    /// Text origin of the frame glyph.
    frame: (f64, f64),
    /// Text origin and font size of the surah name, fitted into the cartouche.
    name: (f64, f64, f64),
    /// Roundel centres (left, right), centre line and radius.
    roundels: (f64, f64, f64, f64),
}

fn banner_art(metrics: &BannerMetrics, chapter: u16) -> Option<BannerArt> {
    let name = *metrics.names.get(usize::from(chapter).checked_sub(1)?)?;
    let (width, height) = (metrics.frame.width(), metrics.frame.height());
    let (x0, y0, x1, y1) = frame::CARTOUCHE;
    let scale = ((x1 - x0) * width / name.width()).min((y1 - y0) * height / name.height());
    // Centre the name's ink box in the cartouche.
    let left = (x0 + x1) / 2.0 * width - name.width() * scale / 2.0;
    let top = (y0 + y1) / 2.0 * height - name.height() * scale / 2.0;
    Some(BannerArt {
        width,
        height,
        frame: (metrics.frame.0, metrics.frame.2),
        name: (left + name.0 * scale, top + name.2 * scale, BANNER_UNITS * scale),
        roundels: (
            frame::ROUNDEL_X.0 * width,
            frame::ROUNDEL_X.1 * width,
            frame::ROUNDEL_Y * height,
            frame::ROUNDEL_R * height,
        ),
    })
}

/// "مكية" / "مدنية" for the API's `revelation_place`.
fn place_arabic(place: &str) -> Option<&'static str> {
    match parse::revelation_label(place) {
        "Meccan" => Some("مكية"),
        "Medinan" => Some("مدنية"),
        _ => None,
    }
}

/// The surah banner, one mushaf line tall: the printed-mushaf frame with the
/// calligraphic name in its cartouche, the place of revelation in the right
/// roundel and the number of verses in the left one. Until QUL's banner fonts
/// are loaded and verified, the same content as a plain framed line.
#[component]
fn SurahBanner(chapter: u16, info: Option<Chapter>) -> Element {
    let art = match fonts::use_banner_fonts() {
        Some(BannerFonts::Ready(metrics)) => banner_art(&metrics, chapter),
        _ => None,
    };
    let place = info.as_ref().and_then(|c| place_arabic(&c.revelation_place));
    let count = info.as_ref().map(|c| glyph::arabic_number(c.verses_count));
    let label = match &info {
        Some(c) => format!(
            "Surah {} · {} · {} verses",
            c.name_simple,
            parse::revelation_label(&c.revelation_place),
            c.verses_count
        ),
        None => format!("Surah {chapter}"),
    };

    let Some(art) = art else {
        return rsx! {
            div {
                class: "surah-banner text",
                role: "img",
                "aria-label": "{label}",
                "data-chapter": "{chapter}",
                "data-banner": "text",
                span { class: "side", {place} }
                if let Some(c) = &info {
                    span { class: "name", "سورة {c.name_arabic}" }
                } else {
                    span { class: "name", lang: "en", dir: "ltr", "{label}" }
                }
                span { class: "side",
                    if let Some(count) = &count {
                        "آياتها {count}"
                    }
                }
            }
        };
    };
    let (left, right, mid, r) = art.roundels;
    let n = |v: f64| format!("{v:.1}");
    rsx! {
        div {
            class: "surah-banner",
            role: "img",
            "aria-label": "{label}",
            "data-chapter": "{chapter}",
            "data-banner": "glyph",
            svg { "viewBox": "0 0 {n(art.width)} {n(art.height)}", "aria-hidden": "true",
                text { class: "frame", x: n(art.frame.0), y: n(art.frame.1), "font-size": n(BANNER_UNITS),
                    {fonts::BANNER_FRAME_TEXT}
                }
                text { class: "name", x: n(art.name.0), y: n(art.name.1), "font-size": n(art.name.2),
                    {fonts::banner_name_text(chapter)}
                }
                if let Some(place) = place {
                    text { class: "label", x: n(right), y: n(mid), "font-size": n(r * 0.66), {place} }
                }
                if let Some(count) = count {
                    text { class: "label", x: n(left), y: n(mid - r * 0.38), "font-size": n(r * 0.44), "آياتها" }
                    text { class: "label", x: n(left), y: n(mid + r * 0.34), "font-size": n(r * 0.66), {count} }
                }
            }
        }
    }
}

/// The Bismillah line: verse 1:1's four glyphs in the page-1 font (`family`,
/// when that font is loaded), else its Unicode text, with the English meaning
/// beneath.
#[component]
fn BismillahLine(chapter: u16, verse: Option<Verse>, family: Option<String>) -> Element {
    let words = || verse.iter().flat_map(|v| &v.words).filter(|w| !w.is_end_marker());
    let glyphs: Vec<String> = words().filter_map(glyph::glyph_code).collect();
    let family = family.filter(|_| !glyphs.is_empty() && glyphs.len() == words().count());
    // Verse 1:1's QPC text without its trailing ayah number.
    let qpc = verse.as_ref().and_then(|v| v.qpc_uthmani_hafs.as_deref()).and_then(|t| {
        let t = t.trim_end().trim_end_matches(|c: char| c.is_numeric()).trim_end();
        (!t.is_empty()).then(|| t.to_string())
    });
    let (class, text) = match (&family, qpc) {
        (Some(_), _) => ("glyph", glyphs.join(" ")),
        (None, Some(qpc)) => ("qpc", qpc),
        (None, None) => ("uthmani", BISMILLAH.to_string()),
    };
    let meaning = verse
        .as_ref()
        .and_then(|v| v.translations.first())
        .map(|t| strip_html(&t.text))
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| BISMILLAH_MEANING.to_string());
    rsx! {
        p {
            class: "bismillah {class}",
            style: family.as_ref().map(|f| format!("font-family: '{f}'")),
            "data-chapter": "{chapter}",
            "data-mode": if family.is_some() { "glyph" } else { "text" },
            "{text}"
        }
        p { class: "bismillah-meaning", lang: "en", dir: "ltr", "{meaning}" }
    }
}

/// A verse's translations under its page, with a link to the verse and a copy action.
#[component]
fn VerseTranslations(verse: Verse) -> Element {
    let to = Route::verses(verse.chapter_number().unwrap_or(0), verse.verse_number);
    rsx! {
        article { class: "page-translation", "data-verse-key": "{verse.verse_key}",
            header {
                Link { class: "verse-key", to, "{verse.verse_key}" }
                CopyVerseButton { verse: verse.clone() }
            }
            if verse.translations.is_empty() {
                p { class: "translation-missing", "No translation selected." }
            }
            for t in verse.translations.iter().cloned() {
                TranslationText { key: "{t.resource_id}", translation: t }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::fonts::Ink;

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
            ..WordCell::default()
        }
    }

    fn html(cell: &WordCell, mode: WordMode, tooltip: bool, error: Option<&String>) -> String {
        let wbw = WbwOpts { translation: true, transliteration: false, tooltip };
        dioxus_ssr::render_element(mushaf_word(cell, mode, wbw, error))
    }

    fn text_mode() -> WordMode {
        WordMode::Text { text: "بِرَبِّ".into(), qpc: true }
    }

    #[test]
    fn word_is_one_element_starting_with_its_text() {
        let out = html(&cell(false), text_mode(), false, None);
        assert!(out.starts_with(r#"<span class="word qpc""#), "{out}");
        assert!(out.contains(r#"data-mode="text">بِرَبِّ<span class="wbw""#), "text node comes first: {out}");
        assert!(!out.contains("word-render-error") && !out.contains("id="), "{out}");
        let glyph = WordMode::Glyph { glyph: "\u{FB51}".into(), family: "p604-v4".into() };
        let out = html(&cell(false), glyph, true, None);
        assert!(out.contains("class=\"word glyph has-tooltip\"") && out.contains("font-family: ") && out.contains("p604-v4"), "{out}");
        assert!(out.contains(r#"role="tooltip""#) && out.contains("tabindex"), "{out}");
        // The verse's first word is its anchor.
        let first = WordCell { anchor: Some(3), ..cell(false) };
        let out = html(&first, text_mode(), false, None);
        assert!(out.contains(r#"id="verse-114:1""#) && out.contains(r#"data-word-count="3""#), "{out}");
    }

    #[test]
    fn audit_failure_shows_a_visible_marker_and_keeps_the_text() {
        let reason = "zero size".to_string();
        for tooltip in [false, true] {
            let out = html(&cell(false), text_mode(), tooltip, Some(&reason));
            assert!(out.contains("word-render-error"), "{out}");
            assert!(out.contains("⚠") && out.contains("zero size"), "{out}");
            assert!(out.contains(">بِرَبِّ<"), "{out}");
        }
        let marker = html(&cell(true), text_mode(), false, Some(&reason));
        assert!(marker.contains("ayah-marker") && marker.contains("word-render-error"), "{marker}");
    }

    #[test]
    fn name_is_fitted_inside_the_cartouche() {
        let frame = Ink(0.0, 8047.0, 813.0, 188.0);
        // A wide, low name and a narrow, tall one.
        let names = vec![Ink(0.0, 6000.0, 300.0, 50.0), Ink(10.0, 1500.0, 844.0, 94.0)];
        let metrics = BannerMetrics { frame, names: names.clone() };
        let (x0, y0, x1, y1) = frame::CARTOUCHE;
        for (i, name) in names.iter().enumerate() {
            let art = banner_art(&metrics, i as u16 + 1).unwrap();
            let scale = art.name.2 / BANNER_UNITS;
            let (left, top) = (art.name.0 - name.0 * scale, art.name.1 - name.2 * scale);
            let (right, bottom) = (left + name.width() * scale, top + name.height() * scale);
            assert!(left >= x0 * art.width - 0.5 && right <= x1 * art.width + 0.5, "{art:?}");
            assert!(top >= y0 * art.height - 0.5 && bottom <= y1 * art.height + 0.5, "{art:?}");
            // Centred, and as large as the cartouche allows in one direction.
            assert!(((left + right) / 2.0 - art.width / 2.0).abs() < 1.0, "{art:?}");
            let fills_width = (right - left - (x1 - x0) * art.width).abs() < 1.0;
            let fills_height = (bottom - top - (y1 - y0) * art.height).abs() < 1.0;
            assert!(fills_width || fills_height, "{art:?}");
        }
        assert_eq!(banner_art(&metrics, 3), None);
        assert_eq!(banner_art(&metrics, 0), None);
        assert_eq!((place_arabic("makkah"), place_arabic("madinah"), place_arabic("")), (Some("مكية"), Some("مدنية"), None));
    }
}
