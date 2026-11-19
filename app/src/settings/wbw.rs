//! Verse translation text with safe footnotes, used for the per-page translations
//! under the mushaf pages. (Word rendering lives in `reader::glyph`/`reader::mushaf_view`.)

use dioxus::prelude::*;
use quran_api::Translation;

use super::html::{Segment, display_segments};
use super::is_rtl_language;
use crate::layout::{ErrorBox, Loading};

#[component]
pub fn TranslationText(translation: Translation) -> Element {
    let mut open_note: Signal<Option<u32>> = use_signal(|| None);
    let segments = display_segments(&translation.text);
    let language = translation.language_name.clone().unwrap_or_default();
    let dir = if is_rtl_language(&language) { "rtl" } else { "ltr" };
    let uid = translation.id.unwrap_or(translation.resource_id);

    rsx! {
        div { class: "translation", dir, "data-resource": "{translation.resource_id}",
            p { class: "translation-text", {render_segments(&segments, Some((uid, open_note)))} }
            if let Some(name) = translation.resource_name.as_ref().filter(|n| !n.is_empty()) {
                p { class: "translation-source", "— {name}" }
            }
            if let Some(id) = open_note() {
                FootNoteBox {
                    key: "{id}",
                    id,
                    dom_id: format!("footnote-{uid}-{id}"),
                    rtl: dir == "rtl",
                    on_close: move |_| open_note.set(None),
                }
            }
        }
    }
}

/// Render tokenized segments. When `footnotes` is given, markers become toggle buttons.
fn render_segments(segments: &[Segment], footnotes: Option<(u32, Signal<Option<u32>>)>) -> Element {
    rsx! {
        {segments.iter().cloned().enumerate().map(|(i, seg)| render_segment(i, seg, footnotes))}
    }
}

fn render_segment(i: usize, seg: Segment, footnotes: Option<(u32, Signal<Option<u32>>)>) -> Element {
    match seg {
        Segment::Text { text, style } => match (style.italic, style.bold) {
            (true, true) => rsx! { em { key: "{i}", strong { "{text}" } } },
            (true, false) => rsx! { em { key: "{i}", "{text}" } },
            (false, true) => rsx! { strong { key: "{i}", "{text}" } },
            (false, false) => rsx! { span { key: "{i}", "{text}" } },
        },
        Segment::LineBreak => rsx! { br { key: "{i}" } },
        Segment::FootNote { id, label } => {
            let label = if label.is_empty() { "*".to_string() } else { label };
            let Some((uid, mut open)) = footnotes else {
                return rsx! { sup { key: "{i}", "{label}" } };
            };
            let expanded = open() == Some(id);
            rsx! {
                sup { key: "{i}", class: "footnote-ref",
                    button {
                        r#type: "button",
                        class: if expanded { "footnote-btn active" } else { "footnote-btn" },
                        aria_expanded: "{expanded}",
                        aria_controls: "footnote-{uid}-{id}",
                        aria_label: "Footnote {label}",
                        onclick: move |_| open.set(if expanded { None } else { Some(id) }),
                        "{label}"
                    }
                }
            }
        }
    }
}

#[component]
fn FootNoteBox(id: u32, dom_id: String, rtl: bool, on_close: EventHandler<()>) -> Element {
    let client = crate::use_api();
    let note = use_resource(move || {
        let client = client.clone();
        async move { client.footnote(id).await }
    });

    let body = match &*note.read() {
        None => rsx! { Loading {} },
        Some(Err(e)) => rsx! { ErrorBox { message: format!("Could not load footnote: {e}") } },
        Some(Ok(n)) => {
            let segs = display_segments(&n.text);
            let lang_rtl = n.language_name.as_deref().map(is_rtl_language).unwrap_or(rtl);
            rsx! {
                div { class: "footnote-text", dir: if lang_rtl { "rtl" } else { "ltr" },
                    {render_segments(&segs, None)}
                }
            }
        }
    };

    rsx! {
        aside { class: "footnote-box", id: "{dom_id}", role: "note", aria_label: "Footnote",
            div { class: "footnote-head",
                span { class: "footnote-title", "Footnote" }
                button {
                    r#type: "button",
                    class: "footnote-close",
                    aria_label: "Close footnote",
                    onclick: move |_| on_close.call(()),
                    "×"
                }
            }
            {body}
        }
    }
}
