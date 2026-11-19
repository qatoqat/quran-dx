//! What to draw for one mushaf word: its QCF glyph (when the page font is
//! loaded) or Unicode text, never nothing. Pure, so the fallback chain is tested.

use quran_api::Word;

/// Placeholder for a word without any glyph or text (never render nothing).
pub const MISSING_WORD: &str = "⟨?⟩";

/// How a word is drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WordMode {
    /// QCF glyph code(s) in the page font `family`.
    Glyph { glyph: String, family: String },
    /// Unicode text in the fallback font stack.
    Text(String),
    /// Nothing to draw: show [`MISSING_WORD`].
    Missing,
}

impl WordMode {
    /// `data-mode` attribute value.
    pub fn attr(&self) -> &'static str {
        match self {
            WordMode::Glyph { .. } => "glyph",
            WordMode::Text(_) | WordMode::Missing => "text",
        }
    }
}

fn non_empty(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|t| !t.is_empty())
}

/// Decode numeric character references (`&#xFB51;`, `&#64337;`) that some API
/// glyph codes use, and drop any other markup-ish text. The result is plain text
/// (rendered as a text node, never as HTML).
pub fn decode_glyph(code: &str) -> String {
    let mut out = String::with_capacity(code.len());
    let mut rest = code;
    while let Some(amp) = rest.find("&#") {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 2..];
        let Some(semi) = after.find(';').filter(|i| *i <= 8) else {
            out.push_str(&rest[amp..amp + 2]);
            rest = after;
            continue;
        };
        let num = &after[..semi];
        let parsed = match num.strip_prefix(['x', 'X']) {
            Some(hex) => u32::from_str_radix(hex, 16).ok(),
            None => num.parse::<u32>().ok(),
        };
        match parsed.and_then(char::from_u32) {
            Some(c) => out.push(c),
            None => out.push_str(&rest[amp..amp + 2 + semi + 1]),
        }
        rest = &after[semi + 1..];
    }
    out.push_str(rest);
    out.trim().to_string()
}

/// The word's Unicode text: `qpc_uthmani_hafs` → `text_uthmani` → `text`.
pub fn unicode_text(word: &Word) -> Option<String> {
    [word.qpc_uthmani_hafs.as_deref(), word.text_uthmani.as_deref(), word.text.as_deref()]
        .into_iter()
        .find_map(non_empty)
        .map(str::to_string)
}

/// The word's glyph code, if it has one that belongs to its layout page
/// (`v2_page` absent or equal to `page_number`).
pub fn glyph_code(word: &Word) -> Option<String> {
    if !super::integrity::glyph_page_ok(word) {
        return None;
    }
    non_empty(word.code_v2.as_deref()).map(decode_glyph).filter(|g| !g.is_empty())
}

/// Choose how to draw a word. `glyph` is the word's glyph code (see
/// [`glyph_code`]), `text` its Unicode text, `font_ready` whether the QCF font of
/// the page is loaded, and `family` that font's CSS family.
pub fn choose_mode(glyph: Option<&str>, text: Option<&str>, font_ready: bool, family: &str) -> WordMode {
    match (glyph, font_ready) {
        (Some(g), true) if !g.trim().is_empty() => {
            WordMode::Glyph { glyph: g.to_string(), family: family.to_string() }
        }
        _ => match non_empty(text) {
            Some(t) => WordMode::Text(t.to_string()),
            None => WordMode::Missing,
        },
    }
}

/// `255` → `٢٥٥`
pub fn arabic_number(n: u16) -> String {
    n.to_string().chars().map(|c| c.to_digit(10).and_then(|d| char::from_u32(0x0660 + d)).unwrap_or(c)).collect()
}

/// Unicode ayah-end marker, e.g. `﴿٢٥٥﴾` (ornate parentheses around the number).
pub fn unicode_ayah_marker(verse_number: u16) -> String {
    format!("\u{fd3f}{}\u{fd3e}", arabic_number(verse_number))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word() -> Word {
        Word { id: 1, position: 1, char_type_name: "word".into(), ..Default::default() }
    }

    #[test]
    fn glyph_only_when_font_loaded_and_code_present() {
        let fam = "p2-v4";
        assert_eq!(
            choose_mode(Some("\u{fb51}"), Some("بِسْمِ"), true, fam),
            WordMode::Glyph { glyph: "\u{fb51}".into(), family: fam.into() }
        );
        // Font not loaded (yet / failed) → Unicode text.
        assert_eq!(choose_mode(Some("\u{fb51}"), Some("بِسْمِ"), false, fam), WordMode::Text("بِسْمِ".into()));
        // No glyph code → text even if the font is loaded.
        assert_eq!(choose_mode(None, Some("t"), true, fam), WordMode::Text("t".into()));
        assert_eq!(choose_mode(Some("  "), Some("t"), true, fam), WordMode::Text("t".into()));
        // Nothing at all → placeholder.
        assert_eq!(choose_mode(None, Some(" "), true, fam), WordMode::Missing);
        assert_eq!(choose_mode(Some("g"), None, false, fam), WordMode::Missing);
        assert_eq!(WordMode::Missing.attr(), "text");
        assert_eq!(choose_mode(Some("g"), None, true, fam).attr(), "glyph");
    }

    #[test]
    fn unicode_fallback_chain() {
        let mut w = word();
        assert_eq!(unicode_text(&w), None);
        w.text = Some("t".into());
        assert_eq!(unicode_text(&w).as_deref(), Some("t"));
        w.text_uthmani = Some("u".into());
        assert_eq!(unicode_text(&w).as_deref(), Some("u"));
        w.qpc_uthmani_hafs = Some(" ".into());
        assert_eq!(unicode_text(&w).as_deref(), Some("u"));
        w.qpc_uthmani_hafs = Some("q".into());
        assert_eq!(unicode_text(&w).as_deref(), Some("q"));
    }

    #[test]
    fn glyph_code_requires_matching_page() {
        let mut w = word();
        assert_eq!(glyph_code(&w), None);
        w.code_v2 = Some("\u{fb51}".into());
        assert_eq!(glyph_code(&w).as_deref(), Some("\u{fb51}"));
        w.page_number = Some(5);
        w.v2_page = Some(5);
        assert!(glyph_code(&w).is_some());
        w.v2_page = Some(6);
        assert_eq!(glyph_code(&w), None);
    }

    #[test]
    fn decodes_entities_as_text() {
        assert_eq!(decode_glyph("&#xFB51;"), "\u{fb51}");
        assert_eq!(decode_glyph("&#64337; &#xfb52;"), "\u{fb51} \u{fb52}");
        assert_eq!(decode_glyph("\u{fb51}"), "\u{fb51}");
        assert_eq!(decode_glyph("&#xZZ;"), "&#xZZ;");
        assert_eq!(decode_glyph("a&#b"), "a&#b");
        assert_eq!(decode_glyph("<b>x</b>"), "<b>x</b>"); // stays inert text
    }

    #[test]
    fn markers() {
        assert_eq!(arabic_number(255), "٢٥٥");
        assert_eq!(unicode_ayah_marker(7), "\u{fd3f}٧\u{fd3e}");
    }
}
