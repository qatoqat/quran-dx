//! Rendering of a single verse (Arabic words + translations + actions).

use dioxus::prelude::*;
use quran_api::Verse;

use super::integrity;
use crate::settings::wbw::{TranslationText, WordView};

#[component]
pub fn VerseView(verse: Verse) -> Element {
    let settings = crate::settings::use_settings();
    // Memoized so unrelated settings (e.g. font size) don't re-render every verse.
    let script_field = use_memo(move || settings.read().script.field())();
    let chapter = verse.chapter_number().unwrap_or(0);
    let link =
        crate::Route::ChapterRange { chapter: chapter.to_string(), range: verse.verse_number.to_string() };
    let key = verse.verse_key.clone();

    // Render word by word only when the words are complete; otherwise fall back
    // to the verse-level text so no part of the verse silently disappears.
    let use_words = integrity::words_ok(&verse, script_field);
    let fallback_tokens: Vec<String> = if use_words {
        Vec::new()
    } else {
        integrity::verse_text(&verse, script_field)
            .map(|t| t.split_whitespace().map(str::to_string).collect())
            .unwrap_or_default()
    };
    let marker = format!("\u{fd3f}{}\u{fd3e}", arabic_number(verse.verse_number));
    let word_count = if use_words { integrity::word_count(&verse) } else { fallback_tokens.len() };

    rsx! {
        article {
            class: if use_words { "verse" } else { "verse verse-fallback" },
            id: "verse-{verse.verse_key}",
            "data-verse-key": "{verse.verse_key}",
            "data-word-count": "{word_count}",
            "data-fallback": if !use_words { "true" },
            div { class: "verse-meta",
                Link { class: "verse-key", to: link, title: "Open verse {key}", "{key}" }
                VerseActions { verse: verse.clone() }
            }
            div { class: "verse-arabic", dir: "rtl", lang: "ar",
                if use_words {
                    for w in verse.words.iter().cloned() {
                        WordView { key: "{w.id}", word: w }
                    }
                } else if fallback_tokens.is_empty() {
                    span { class: "verse-text-missing", dir: "ltr",
                        "The Arabic text of this verse could not be loaded."
                    }
                } else {
                    for (i, t) in fallback_tokens.into_iter().enumerate() {
                        span { key: "{i}", class: "word fallback-word", "{t} " }
                    }
                    span { class: "verse-number-marker", "{marker}" }
                }
            }
            div { class: "verse-translations",
                for t in verse.translations.iter().cloned() {
                    TranslationText { key: "{t.resource_id}", translation: t }
                }
            }
        }
    }
}

/// Small row of per-verse actions. Copy uses the browser/webview clipboard via
/// `document::eval`, which works on web and desktop alike.
#[component]
fn VerseActions(verse: Verse) -> Element {
    let mut copied = use_signal(|| false);
    let onclick = move |_| {
        let script = format!("const text = {};\n{}", js_string(&copy_text(&verse)), COPY_JS);
        document::eval(&script);
        copied.set(true);
        spawn(async move {
            crate::util::sleep_ms(1500).await;
            copied.set(false);
        });
    };
    rsx! {
        div { class: "verse-actions",
            button {
                class: "verse-action",
                r#type: "button",
                title: "Copy verse text and translations",
                onclick,
                if copied() { "Copied ✓" } else { "Copy" }
            }
        }
    }
}

/// Clipboard write with a `<textarea>` + `execCommand` fallback for contexts
/// where `navigator.clipboard` is unavailable (e.g. some desktop webviews).
const COPY_JS: &str = r#"
const fallback = () => {
  const area = document.createElement("textarea");
  area.value = text;
  area.setAttribute("readonly", "");
  area.style.position = "fixed";
  area.style.opacity = "0";
  document.body.appendChild(area);
  area.select();
  try { document.execCommand("copy"); } catch (_) {}
  area.remove();
};
if (navigator.clipboard && window.isSecureContext) {
  navigator.clipboard.writeText(text).catch(fallback);
} else {
  fallback();
}
"#;

/// Arabic-Indic digits for an ayah number marker.
fn arabic_number(n: u16) -> String {
    n.to_string()
        .chars()
        .map(|c| c.to_digit(10).and_then(|d| char::from_u32(0x0660 + d)).unwrap_or(c))
        .collect()
}

/// Encode `s` as a JavaScript string literal (JSON is a subset of JS).
fn js_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// Plain-text form of a verse for the clipboard.
pub(crate) fn copy_text(verse: &Verse) -> String {
    let arabic = verse
        .text_uthmani
        .clone()
        .or_else(|| verse.text_indopak.clone())
        .or_else(|| verse.text_imlaei.clone())
        .unwrap_or_else(|| {
            verse
                .words
                .iter()
                .filter(|w| !w.is_end_marker())
                .filter_map(|w| {
                    w.text_uthmani.clone().or_else(|| w.text_indopak.clone()).or_else(|| w.text.clone())
                })
                .collect::<Vec<_>>()
                .join(" ")
        });
    let mut out = arabic.trim().to_string();
    for t in &verse.translations {
        out.push_str("\n\n");
        out.push_str(&strip_html(&t.text));
        if let Some(name) = t.resource_name.as_deref().filter(|n| !n.is_empty()) {
            out.push_str(" — ");
            out.push_str(name);
        }
    }
    out.push_str(&format!("\n\n({})", verse.verse_key));
    out
}

/// Remove HTML tags from translation text. Footnote markers
/// (`<sup foot_note=1>1</sup>`) are dropped together with their content.
pub(crate) fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    let mut sup_depth = 0usize;
    while let Some(lt) = rest.find('<') {
        if sup_depth == 0 {
            out.push_str(&rest[..lt]);
        }
        let Some(gt) = rest[lt..].find('>') else {
            // Unterminated tag: keep the text as-is.
            if sup_depth == 0 {
                out.push_str(&rest[lt..]);
            }
            rest = "";
            break;
        };
        let tag = rest[lt + 1..lt + gt].trim().to_ascii_lowercase();
        if tag == "sup" || tag.starts_with("sup ") {
            sup_depth += 1;
        } else if tag == "/sup" {
            sup_depth = sup_depth.saturating_sub(1);
        }
        rest = &rest[lt + gt + 1..];
    }
    if sup_depth == 0 {
        out.push_str(rest);
    }
    decode_entities(&out).split_whitespace().collect::<Vec<_>>().join(" ")
}

fn decode_entities(s: &str) -> String {
    s.replace("&nbsp;", " ")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_tags_and_footnotes() {
        assert_eq!(
            strip_html("Allah<sup foot_note=123>1</sup> — there is no deity <i>except</i> Him"),
            "Allah — there is no deity except Him"
        );
        assert_eq!(strip_html("a &amp; b&nbsp;c"), "a & b c");
        assert_eq!(strip_html("plain"), "plain");
        assert_eq!(strip_html("x < y"), "x < y");
        assert_eq!(strip_html("<p>one</p>\n<p>two</p>"), "one two");
    }

    #[test]
    fn copy_text_format() {
        let verse: Verse = serde_json::from_str(
            r#"{"id":1,"verse_number":1,"verse_key":"1:1","text_uthmani":"بِسْمِ",
                "translations":[{"resource_id":131,"text":"In the Name<sup foot_note=1>1</sup>",
                "resource_name":"Dr. Mustafa Khattab"}]}"#,
        )
        .unwrap();
        assert_eq!(copy_text(&verse), "بِسْمِ\n\nIn the Name — Dr. Mustafa Khattab\n\n(1:1)");
    }

    #[test]
    fn copy_text_from_words() {
        let verse: Verse = serde_json::from_str(
            r#"{"id":1,"verse_number":1,"verse_key":"1:1","words":[
                {"id":1,"position":1,"char_type_name":"word","text_uthmani":"a"},
                {"id":2,"position":2,"char_type_name":"word","text_uthmani":"b"},
                {"id":3,"position":3,"char_type_name":"end","text_uthmani":"١"}]}"#,
        )
        .unwrap();
        assert_eq!(copy_text(&verse), "a b\n\n(1:1)");
    }

    #[test]
    fn arabic_digits() {
        assert_eq!(arabic_number(255), "٢٥٥");
    }

    #[test]
    fn js_string_escapes() {
        assert_eq!(js_string("a\"b\n</script>"), r#""a\"b\n</script>""#);
    }
}
