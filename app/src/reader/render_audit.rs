//! Built-in render audit: checks the REAL page (the DOM the browser/webview
//! actually shows), not just the data, so a word can't silently go missing
//! because of rendering.
//!
//! When a mushaf page scrolls into view, every word and ayah marker the layout
//! placed on it is looked up in the DOM and must:
//! - exist exactly once, inside that page,
//! - contain exactly the text we rendered (glyph code or Unicode text),
//! - not be hidden (display/visibility/opacity), transparent, or tiny,
//! - have a non-zero on-screen size,
//! - use a QCF page font that the browser reports as loaded (glyph words).
//!
//! Any failure is reported back to Rust. The word is then switched to plain
//! Unicode text and gets a visible red error marker, and the page shows an
//! alert listing the affected words, so the problem is never silent. If the
//! audit itself can't run (e.g. no JS runtime, as in tests), nothing is changed.

use serde::{Deserialize, Serialize};

use super::glyph::{self, MISSING_WORD, WordMode};
use super::mushaf_layout::{Item, PageBlock, WordCell};

/// What the DOM must show for one word or marker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Expect {
    /// `data-location`, e.g. `"114:1:2"`.
    pub loc: String,
    /// Exact text of its `.word-text` element.
    pub text: String,
    /// QCF font family for glyph words (must be loaded), else `None`.
    pub family: Option<String>,
}

/// One audit failure: `(location, reason)`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Failure(pub String, pub String);

/// The text a cell's `.word-text` contains for `mode` (mirrors `mushaf_word`).
pub fn expected_text(cell: &WordCell, mode: &WordMode) -> String {
    match mode {
        WordMode::Glyph { glyph, .. } => glyph.clone(),
        _ if cell.is_end => glyph::unicode_ayah_marker(cell.verse_number),
        WordMode::Text(t) => t.clone(),
        WordMode::Missing => MISSING_WORD.to_string(),
    }
}

/// Everything the page must show, given how each cell is drawn.
pub fn expectations(block: &PageBlock, mode_of: impl Fn(&WordCell) -> WordMode) -> Vec<Expect> {
    let mut out = Vec::new();
    for item in &block.items {
        let cells = match item {
            Item::Line { cells, .. } | Item::Flow { cells, .. } => cells,
            _ => continue,
        };
        for c in cells {
            let mode = mode_of(c);
            let family = match &mode {
                WordMode::Glyph { family, .. } => Some(family.clone()),
                _ => None,
            };
            out.push(Expect { loc: c.location.clone(), text: expected_text(c, &mode), family });
        }
    }
    out
}

/// JS that audits the page section `section.mushaf-page[data-block=…]` and
/// returns `[[location, reason], …]` (empty = all good), or `null` if the
/// section isn't in the document.
pub fn audit_js(block_id: &str, expected: &[Expect]) -> String {
    let id = serde_json::to_string(block_id).unwrap_or_else(|_| "\"\"".into());
    let exp = serde_json::to_string(expected).unwrap_or_else(|_| "[]".into());
    format!(
        r##"const blockId = {id};
const expected = {exp};
const root = [...document.querySelectorAll("section.mushaf-page[data-block]")].find((s) => s.dataset.block === blockId);
if (!root) return null;
const out = [];
const els = new Map();
for (const el of root.querySelectorAll(".word[data-location], .ayah-marker[data-location]")) {{
  const l = el.dataset.location;
  if (els.has(l)) out.push([l, "shown twice"]); else els.set(l, el);
}}
const transparent = (color) => {{
  const m = /rgba?\(([^)]*)\)/.exec(color || "");
  if (!m) return false;
  const parts = m[1].split(/[\s,\/]+/).filter(Boolean);
  return parts.length === 4 && parseFloat(parts[3]) === 0;
}};
const hiddenBy = (el) => {{
  for (let n = el; n && n !== document.body; n = n.parentElement) {{
    const cs = getComputedStyle(n);
    if (cs.display === "none") return "hidden (display: none)";
    if (parseFloat(cs.opacity) === 0) return "hidden (opacity 0)";
  }}
  return null;
}};
for (const e of expected) {{
  const el = els.get(e.loc);
  if (!el) {{ out.push([e.loc, "not on the page"]); continue; }}
  els.delete(e.loc);
  const t = el.querySelector(".word-text");
  if (!t) {{ out.push([e.loc, "no text element"]); continue; }}
  const text = (t.textContent || "").trim();
  if (!text) {{ out.push([e.loc, "empty"]); continue; }}
  if (text !== e.text.trim()) {{ out.push([e.loc, "wrong text"]); continue; }}
  const cs = getComputedStyle(t);
  if (cs.visibility !== "visible") {{ out.push([e.loc, "hidden (visibility)"]); continue; }}
  const hidden = hiddenBy(t);
  if (hidden) {{ out.push([e.loc, hidden]); continue; }}
  if (transparent(cs.color)) {{ out.push([e.loc, "transparent"]); continue; }}
  if (parseFloat(cs.fontSize) < 4) {{ out.push([e.loc, "text too small"]); continue; }}
  const r = t.getBoundingClientRect();
  if (r.width < 1 || r.height < 1) {{ out.push([e.loc, "zero size"]); continue; }}
  if (e.family && !document.fonts.check(`16px "${{e.family}}"`, text)) {{ out.push([e.loc, "font not loaded"]); continue; }}
}}
for (const l of els.keys()) out.push([l, "unexpected word"]);
return out;"##
    )
}

/// Run the audit. `None` if it couldn't run (no JS runtime, section gone).
pub async fn run(block_id: &str, expected: &[Expect]) -> Option<Vec<Failure>> {
    let js = audit_js(block_id, expected);
    let value: serde_json::Value = dioxus::prelude::document::eval(&js).join().await.ok()?;
    if value.is_null() {
        return None;
    }
    serde_json::from_value(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::mushaf_layout::LineAlign;

    fn cell(loc: &str, is_end: bool) -> WordCell {
        WordCell {
            location: loc.into(),
            verse_key: loc.rsplit_once(':').unwrap().0.into(),
            verse_number: 3,
            page: Some(604),
            line: Some(2),
            is_end,
            glyph: Some("G".into()),
            text: Some("T".into()),
            translation: None,
            transliteration: None,
            anchor: None,
        }
    }

    #[test]
    fn expectations_mirror_rendered_text() {
        let block = PageBlock {
            page: Some(604),
            items: vec![Item::Line { line: 2, cells: vec![cell("114:3:1", false), cell("114:3:2", true)], align: LineAlign::Justify }],
            verse_keys: vec!["114:3".into()],
        };
        let glyph = |_: &WordCell| WordMode::Glyph { glyph: "G".into(), family: "p604-v4".into() };
        assert_eq!(
            expectations(&block, glyph),
            vec![
                Expect { loc: "114:3:1".into(), text: "G".into(), family: Some("p604-v4".into()) },
                Expect { loc: "114:3:2".into(), text: "G".into(), family: Some("p604-v4".into()) },
            ]
        );
        // Text mode: words show their Unicode text; ayah markers show ﴿٣﴾.
        let text = |c: &WordCell| WordMode::Text(c.text.clone().unwrap());
        let e = expectations(&block, text);
        assert_eq!(e[0], Expect { loc: "114:3:1".into(), text: "T".into(), family: None });
        assert_eq!(e[1].text, glyph::unicode_ayah_marker(3));
        assert_eq!(expected_text(&cell("1:1:1", false), &WordMode::Missing), MISSING_WORD);
    }

    #[test]
    fn audit_js_embeds_values_safely() {
        let js = audit_js("p1-0\"</script>", &[Expect { loc: "1:1:1".into(), text: "\"x".into(), family: None }]);
        assert!(js.contains(r#"const blockId = "p1-0\"</script>";"#));
        assert!(js.contains(r#""text":"\"x""#));
        assert!(js.contains("not on the page") && js.contains("zero size") && js.contains("font not loaded"));
    }

    #[test]
    fn failures_parse() {
        let f: Vec<Failure> = serde_json::from_str(r#"[["1:1:2","zero size"]]"#).unwrap();
        assert_eq!(f, vec![Failure("1:1:2".into(), "zero size".into())]);
    }
}
