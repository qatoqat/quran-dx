//! Fitting and auditing the REAL page (the DOM the browser/webview shows).
//!
//! Fit: a mushaf line is never wrapped or cut. If the widest line on any page
//! is wider than its page, everything on all pages (Arabic, word meanings,
//! banners) is scaled down through the `--fit` CSS variable until it fits, and
//! every page is made as wide as that widest line, so text size and line width
//! stay the same from page to page.
//!
//! Audit: when a page scrolls into view, every word and ayah marker the layout
//! placed on it is looked up in the DOM and must:
//! - exist exactly once, inside that page,
//! - show exactly the text we rendered (glyph code or Unicode text),
//! - not be hidden (display/visibility/opacity), transparent, or tiny,
//! - have a non-zero on-screen size,
//! - not be cut off: the whole word (with its meaning) must lie inside the window
//!   and inside every ancestor that clips,
//! - use a QCF page font that the browser reports as loaded (glyph words).
//!
//! Any failure is reported back to Rust. The word is then switched to plain
//! Unicode text and gets a visible red error marker, and the page shows an
//! alert listing the affected words, so the problem is never silent. If the
//! audit itself can't run (e.g. no JS runtime, as in tests), nothing is changed.

use serde::{Deserialize, Serialize};

use super::glyph::WordMode;
use super::mushaf_layout::{Item, PageBlock, WordCell};
use crate::util::js_string;

/// What the DOM must show for one word or marker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Expect {
    /// `data-location`, e.g. `"114:1:2"`.
    pub loc: String,
    /// Exact text of the element's own text node.
    pub text: String,
    /// QCF font family for glyph words (must be loaded), else `None`.
    pub family: Option<String>,
}

/// One audit failure: `(location, reason)`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Failure(pub String, pub String);

/// Everything the page must show, given how each cell is drawn.
pub fn expectations(block: &PageBlock, mode_of: impl Fn(&WordCell) -> WordMode) -> Vec<Expect> {
    let mut out = Vec::new();
    for item in &block.items {
        let (Item::Line { cells, .. } | Item::Flow { cells, .. }) = item else { continue };
        for c in cells {
            let mode = mode_of(c);
            let family = match &mode {
                WordMode::Glyph { family, .. } => Some(family.clone()),
                _ => None,
            };
            out.push(Expect { loc: c.location.clone(), text: mode.text().to_string(), family });
        }
    }
    out
}

/// Defines `window.__mushafFit` once: `fit(lines)` re-measures one
/// `.mushaf-lines` box, `all()` every box. Each box needs some scale for its
/// widest line to fit its page; the smallest of them is set as `--fit` on the
/// document, and the widest line at that scale as `--mushaf-width`, so all pages
/// on screen share one text size and one line width. Window resizes and late
/// web-font loads re-measure everything.
const FIT_JS: &str = r#"
if (!window.__mushafFit) {
  const html = document.documentElement;
  // Per box: the page width available to it, and its widest line at scale 1.
  const sizes = new WeakMap();
  const scale = () => parseFloat(html.style.getPropertyValue("--fit")) || 1;
  const boxes = () => document.querySelectorAll(".mushaf-lines");
  // A line's own width (its words and gaps), however the line is stretched.
  const lineWidth = (line) => {
    if (!line.classList.contains("mushaf-line")) return line.getBoundingClientRect().width;
    let w = (parseFloat(getComputedStyle(line).columnGap) || 0) * Math.max(0, line.children.length - 1);
    for (const word of line.children) w += word.getBoundingClientRect().width;
    return w;
  };
  const measure = (el) => {
    let widest = 0;
    for (const line of el.querySelectorAll(":scope > .mushaf-line, :scope > .bismillah")) widest = Math.max(widest, lineWidth(line));
    sizes.set(el, { room: el.parentElement.clientWidth, natural: widest / scale() });
  };
  const apply = () => {
    let fit = 1, natural = 0;
    for (const el of boxes()) {
      const size = sizes.get(el);
      if (!size || !(size.room > 0) || !(size.natural > 0)) continue;
      // One pixel to spare: engines round glyph and font sizes.
      fit = Math.min(fit, Math.floor((size.room - 1) / size.natural * 1000) / 1000);
      natural = Math.max(natural, size.natural);
    }
    fit = Math.max(0.2, fit);
    html.style.setProperty("--fit", String(fit));
    html.style.setProperty("--mushaf-width", `${Math.floor(natural * fit)}px`);
  };
  // Lines don't scale exactly with --fit (meanings wrap differently, engines round
  // font sizes), so after a change of scale the boxes are measured again at it.
  const settle = (els) => {
    for (let round = 0; round < 3; round++) {
      const before = scale();
      els.forEach(measure);
      apply();
      if (scale() === before) break;
    }
  };
  const fit = (el) => { if (el) settle([el]); };
  const all = () => settle([...boxes()]);
  let queued = false;
  const later = () => {
    if (queued) return;
    queued = true;
    requestAnimationFrame(() => { queued = false; all(); });
  };
  window.__mushafFit = { fit, all };
  addEventListener("resize", later);
  document.fonts.addEventListener("loadingdone", later);
}
"#;

/// JS binding `root` to the page section `section.mushaf-page[data-block=…]`.
fn root_js(block_id: &str) -> String {
    format!(
        "const root = [...document.querySelectorAll(\"section.mushaf-page[data-block]\")].find((s) => s.dataset.block === {});",
        js_string(block_id)
    )
}

/// JS that fits one page's lines.
pub fn fit_js(block_id: &str) -> String {
    format!("{FIT_JS}\n{}\nwindow.__mushafFit.fit(root && root.querySelector(\".mushaf-lines\"));", root_js(block_id))
}

/// Refit every page (text size or spacing settings changed).
pub fn fit_all() {
    dioxus::prelude::document::eval(&format!("{FIT_JS}\nwindow.__mushafFit.all();"));
}

/// JS that fits and then audits one page and returns `[[location, reason], …]`
/// (empty = all good), `"later"` if the page isn't rendered yet, or `null` if
/// the section isn't in the document. It runs synchronously, so the DOM it sees
/// is the one `expected` was computed for.
pub fn audit_js(block_id: &str, expected: &[Expect]) -> String {
    let fit = fit_js(block_id);
    let exp = serde_json::to_string(expected).unwrap_or_else(|_| "[]".into());
    format!(
        r##"{fit}
if (!root) return null;
// A page that has only just scrolled into view may still be an unrendered
// placeholder (`content-visibility: auto`) whose box does not match its words yet.
const lines = root.querySelector(".mushaf-lines");
if (lines && lines.checkVisibility && !lines.checkVisibility({{ contentVisibilityAuto: true }})) return "later";
// A line that sticks out of its page right now may only be waiting for a font or
// for the fit to settle: fit everything again and look once more, a few times,
// before any word is reported as cut off.
if (lines) {{
  const box = root.getBoundingClientRect();
  const sticksOut = [...lines.querySelectorAll(":scope > .mushaf-line")].some((l) => {{
    const first = l.firstElementChild, last = l.lastElementChild;
    return first && (first.getBoundingClientRect().right > box.right + 1 || last.getBoundingClientRect().left < box.left - 1);
  }});
  const tries = Number(root.dataset.refits || 0);
  if (sticksOut && tries < 6) {{
    root.dataset.refits = String(tries + 1);
    window.__mushafFit.all();
    return "later";
  }}
}}
const expected = {exp};
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
const cutOff = (node) => {{
  const r = node.getBoundingClientRect();
  const vw = document.documentElement.clientWidth;
  if (r.left < -1 || r.right > vw + 1) return "cut off by the window edge";
  for (let n = node.parentElement; n && n !== document.documentElement; n = n.parentElement) {{
    const cs = getComputedStyle(n);
    const clips = /hidden|clip/.test(cs.overflowX + " " + cs.overflowY)
      || /paint|strict|content/.test(cs.contain || "")
      || cs.contentVisibility === "auto" || cs.contentVisibility === "hidden";
    if (!clips) continue;
    const a = n.getBoundingClientRect();
    if (r.left < a.left - 1 || r.right > a.right + 1 || r.top < a.top - 1 || r.bottom > a.bottom + 1) {{
      return "cut off (partly outside its page)";
    }}
  }}
  return null;
}};
const hiddenBy = (el) => {{
  for (let n = el; n && n !== document.body; n = n.parentElement) {{
    const cs = getComputedStyle(n);
    if (cs.display === "none") return "hidden (display: none)";
    if (parseFloat(cs.opacity) === 0) return "hidden (opacity 0)";
  }}
  return null;
}};
const range = document.createRange();
for (const e of expected) {{
  const el = els.get(e.loc);
  if (!el) {{ out.push([e.loc, "not on the page"]); continue; }}
  els.delete(e.loc);
  // The word's text is its own first text node; meanings are child elements.
  const node = el.firstChild;
  if (!node || node.nodeType !== Node.TEXT_NODE) {{ out.push([e.loc, "no text"]); continue; }}
  const text = node.data.trim();
  if (!text) {{ out.push([e.loc, "empty"]); continue; }}
  if (text !== e.text.trim()) {{ out.push([e.loc, "wrong text"]); continue; }}
  const cs = getComputedStyle(el);
  if (cs.visibility !== "visible") {{ out.push([e.loc, "hidden (visibility)"]); continue; }}
  const hidden = hiddenBy(el);
  if (hidden) {{ out.push([e.loc, hidden]); continue; }}
  if (transparent(cs.color)) {{ out.push([e.loc, "transparent"]); continue; }}
  if (parseFloat(cs.fontSize) < 4) {{ out.push([e.loc, "text too small"]); continue; }}
  range.selectNodeContents(node);
  const r = range.getBoundingClientRect();
  // A glyph may take no room and still be drawn: a pause mark the font places on
  // the word before it (the API gives 40:77:5 only such a mark). It must have ink.
  const mark = () => {{
    if (!e.family) return false;
    const ctx = (window.__auditInk ??= document.createElement("canvas").getContext("2d"));
    ctx.font = `32px "${{e.family}}"`;
    const m = ctx.measureText(text);
    return m.actualBoundingBoxLeft + m.actualBoundingBoxRight > 0.5 && m.actualBoundingBoxAscent + m.actualBoundingBoxDescent > 0.5;
  }};
  if (r.height < 1 || (r.width < 1 && !mark())) {{ out.push([e.loc, "zero size"]); continue; }}
  const cut = cutOff(el);
  if (cut) {{ out.push([e.loc, cut]); continue; }}
  if (e.family && !document.fonts.check(`16px "${{e.family}}"`, text)) {{ out.push([e.loc, "font not loaded"]); continue; }}
}}
for (const l of els.keys()) out.push([l, "unexpected word"]);
return out;"##
    )
}

/// Outcome of one audit run.
#[derive(Debug, PartialEq)]
pub enum Audit {
    Done(Vec<Failure>),
    /// The page isn't rendered yet: try again shortly.
    Later,
    /// The audit can't run (no JS runtime, section gone).
    Unavailable,
}

impl Audit {
    fn from_json(value: serde_json::Value) -> Self {
        match value {
            serde_json::Value::String(_) => Audit::Later,
            list @ serde_json::Value::Array(_) => serde_json::from_value(list).map_or(Audit::Unavailable, Audit::Done),
            _ => Audit::Unavailable,
        }
    }
}

/// Fit and audit a page.
pub async fn run(block_id: &str, expected: &[Expect]) -> Audit {
    crate::util::eval(&audit_js(block_id, expected)).await.map_or(Audit::Unavailable, Audit::from_json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::glyph::{self, MISSING_WORD};
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
            text: Some(if is_end { glyph::arabic_number(3) } else { "T".into() }),
            ..WordCell::default()
        }
    }

    #[test]
    fn expectations_mirror_rendered_text() {
        let block = PageBlock {
            page: Some(604),
            items: vec![Item::Line {
                line: 2,
                cells: vec![cell("114:3:1", false), cell("114:3:2", true)],
                align: LineAlign::Justify,
            }],
            verse_keys: vec!["114:3".into()],
        };
        let glyph = |c: &WordCell| glyph::choose_mode(c, true, "p604-v4");
        assert_eq!(
            expectations(&block, glyph),
            vec![
                Expect { loc: "114:3:1".into(), text: "G".into(), family: Some("p604-v4".into()) },
                Expect { loc: "114:3:2".into(), text: "G".into(), family: Some("p604-v4".into()) },
            ]
        );
        // Text mode: words show their Unicode text; ayah markers their number.
        let e = expectations(&block, |c| glyph::choose_mode(c, false, ""));
        assert_eq!(e[0], Expect { loc: "114:3:1".into(), text: "T".into(), family: None });
        assert_eq!(e[1].text, "٣");
        assert_eq!(expectations(&block, |_| WordMode::Missing)[0].text, MISSING_WORD);
    }

    #[test]
    fn audit_js_embeds_values_safely_and_fits_first() {
        let js = audit_js("p1-0\"</script>", &[Expect { loc: "1:1:1".into(), text: "\"x".into(), family: None }]);
        assert!(js.contains(r#"s.dataset.block === "p1-0\"</script>""#), "{js}");
        assert!(js.contains(r#""text":"\"x""#));
        assert!(js.contains("not on the page") && js.contains("zero size") && js.contains("font not loaded"));
        assert!(js.contains("cut off"));
        let fit = js.find("window.__mushafFit.fit(").unwrap();
        assert!(fit < js.find("const expected").unwrap(), "lines are fitted before words are checked for cut-off");
    }

    #[test]
    fn audit_results_parse() {
        let parse = |json: &str| Audit::from_json(serde_json::from_str(json).unwrap());
        assert_eq!(parse(r#"[["1:1:2","zero size"]]"#), Audit::Done(vec![Failure("1:1:2".into(), "zero size".into())]));
        assert_eq!(parse("[]"), Audit::Done(Vec::new()));
        assert_eq!(parse(r#""later""#), Audit::Later);
        assert_eq!(parse("null"), Audit::Unavailable);
        assert_eq!(parse("[1]"), Audit::Unavailable);
    }
}
