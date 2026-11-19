//! The lists: all 114 surahs in a filterable grid (`/`) and the 30 ajzāʾ
//! (`/juz`). The header's mode toggle switches between them.

use dioxus::prelude::*;
use quran_api::Chapter;

use super::parse::{self, VerseRef};
use super::{chapter_route, find_chapter, use_chapters, use_juzs};
use crate::Route;
use crate::layout::{ErrorBox, Loading};

#[component]
pub fn Home() -> Element {
    let mut filter = use_signal(String::new);

    // A typed reference like "2:255" or "18" offers a direct jump.
    let jump: Option<(Route, String)> =
        parse::parse_chapter_segment(&filter.read()).ok().and_then(|r: VerseRef| {
            let span = r.span?;
            Some((chapter_route(r.chapter, Some(span)), format!("verse {}:{}", r.chapter, span.segment())))
        });

    rsx! {
        document::Title { "The Noble Quran" }
        input {
            class: "surah-filter",
            r#type: "search",
            placeholder: "Filter by number or name (e.g. 18, kahf, 2:255)",
            "aria-label": "Filter surahs",
            value: "{filter}",
            oninput: move |e| filter.set(e.value()),
        }
        if let Some((to, label)) = jump {
            Link { class: "jump-link", to, "Go to {label} →" }
        }
        SurahGrid { filter: filter() }
    }
}

#[component]
fn SurahGrid(filter: String) -> Element {
    let chapters = use_chapters();
    match &*chapters.read() {
        None => rsx! { Loading {} },
        Some(Err(e)) => rsx! { ErrorBox { message: "Could not load the list of surahs: {e}" } },
        Some(Ok(list)) => {
            let shown: Vec<Chapter> = list.iter().filter(|c| chapter_matches(c, &filter)).cloned().collect();
            rsx! {
                if shown.is_empty() {
                    p { class: "feed-empty", "No surah matches “{filter}”." }
                }
                ol { class: "surah-grid",
                    for c in shown {
                        li { key: "{c.id}", SurahCard { chapter: c } }
                    }
                }
            }
        }
    }
}

#[component]
fn SurahCard(chapter: Chapter) -> Element {
    rsx! {
        Link { class: "surah-card", to: chapter_route(chapter.id, None),
            span { class: "surah-number", "{chapter.id}" }
            span { class: "surah-name", "{chapter.name_simple}" }
            span { class: "surah-arabic", dir: "rtl", lang: "ar", "{chapter.name_arabic}" }
            span { class: "surah-meta", "{chapter.translated_name.name} · {chapter.verses_count} verses" }
        }
    }
}

#[component]
pub fn JuzIndex() -> Element {
    let juzs = use_juzs();
    let chapters = use_chapters();
    let chapter_list = chapters.read().as_ref().and_then(|r| r.clone().ok()).unwrap_or_default();
    match &*juzs.read() {
        None => rsx! { Loading {} },
        Some(Err(e)) => rsx! { ErrorBox { message: "Could not load the list of juz: {e}" } },
        Some(Ok(list)) => rsx! {
            document::Title { "Juz – The Noble Quran" }
            ol { class: "juz-grid",
                for juz in list.iter() {
                    li { key: "{juz.juz_number}", class: "juz-card",
                        Link { class: "juz-title", to: Route::juz(juz.juz_number), "Juz {juz.juz_number}" }
                        for (chapter , span) in parse::juz_mapping(&juz.verse_mapping) {
                            JuzRangeLink {
                                key: "{chapter}",
                                chapter,
                                span,
                                info: find_chapter(&chapter_list, chapter).cloned(),
                            }
                        }
                    }
                }
            }
        },
    }
}

#[component]
fn JuzRangeLink(chapter: u16, span: parse::VerseSpan, info: Option<Chapter>) -> Element {
    let whole = info.as_ref().is_some_and(|c| span.from == 1 && span.to >= c.verses_count);
    let name = info.as_ref().map(|c| c.name_simple.clone()).unwrap_or_else(|| format!("Surah {chapter}"));
    let to = chapter_route(chapter, if whole { None } else { Some(span) });
    rsx! {
        Link { class: "juz-range", to,
            "{chapter}. {name}"
            span { " {span.from}–{span.to}" }
        }
    }
}

/// Lowercase and keep only letters/digits, so "Al-Kahf" matches "al kahf" / "alkahf".
fn normalize(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(char::to_lowercase).collect()
}

/// Filter predicate for the surah grid: number prefix, or name substring
/// (simple, complex, translated or Arabic name).
pub(crate) fn chapter_matches(c: &Chapter, filter: &str) -> bool {
    let f = filter.trim();
    if f.is_empty() {
        return true;
    }
    if f.chars().all(|ch| ch.is_ascii_digit()) {
        return c.id.to_string().starts_with(f);
    }
    // "2:255" style input filters by chapter number.
    if let Ok(r) = parse::parse_chapter_segment(f) {
        return c.id == r.chapter;
    }
    let needle = normalize(f);
    if needle.is_empty() {
        return true;
    }
    [&c.name_simple, &c.name_complex, &c.translated_name.name, &c.name_arabic]
        .iter()
        .any(|name| normalize(name).contains(&needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chapter(id: u16, simple: &str, translated: &str, arabic: &str) -> Chapter {
        serde_json::from_value(serde_json::json!({
            "id": id, "revelation_place": "makkah", "revelation_order": 1, "bismillah_pre": true,
            "name_simple": simple, "name_complex": simple, "name_arabic": arabic,
            "verses_count": 7, "translated_name": { "name": translated }
        }))
        .unwrap()
    }

    #[test]
    fn filter_by_number_and_name() {
        let kahf = chapter(18, "Al-Kahf", "The Cave", "الكهف");
        assert!(chapter_matches(&kahf, ""));
        assert!(chapter_matches(&kahf, "18"));
        assert!(chapter_matches(&kahf, "1"));
        assert!(!chapter_matches(&kahf, "2"));
        assert!(chapter_matches(&kahf, "kahf"));
        assert!(chapter_matches(&kahf, "Al Kahf"));
        assert!(chapter_matches(&kahf, "alkahf"));
        assert!(chapter_matches(&kahf, "cave"));
        assert!(chapter_matches(&kahf, "الكهف"));
        assert!(chapter_matches(&kahf, "18:10"));
        assert!(!chapter_matches(&kahf, "2:255"));
        assert!(!chapter_matches(&kahf, "baqarah"));
        assert!(chapter_matches(&kahf, " - "));
    }
}
