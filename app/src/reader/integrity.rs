//! Pure integrity checks over loaded verses, so no word or verse of the Quran
//! silently goes missing from the view. Nothing here panics; every problem is
//! reported as an [`IntegrityIssue`].
//!
//! - (a) [`sort_dedup`]: verses sorted by (chapter, verse) and deduplicated by key.
//! - (b) [`check_gaps`] / [`check_expected`]: verse numbers contiguous.
//! - (c) + (d) [`word_issues`]: per-verse word structure and per-word text.

use std::fmt;

use quran_api::{Verse, Word};

use super::parse::VerseSpan;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IntegrityIssue {
    /// The same verse arrived more than once (dropped).
    Duplicate { verse_key: String },
    /// Verses arrived out of mushaf order (re-sorted).
    OutOfOrder { verse_key: String },
    /// A verse that should be shown is not in the list.
    MissingVerse { verse_key: String },
    /// A verse came without any words.
    NoWords { verse_key: String },
    /// Word positions are not 1, 2, 3, …: `position` is the expected one.
    WordPositionGap { verse_key: String, position: u16 },
    /// No `end` (ayah number) marker.
    MissingEndMarker { verse_key: String },
    /// More than one `end` marker; `position` is the extra one.
    ExtraEndMarker { verse_key: String, position: u16 },
    /// The `end` marker is not the last word.
    EndMarkerNotLast { verse_key: String, position: u16 },
    /// A word has no text for the selected script nor any fallback.
    EmptyWordText { verse_key: String, position: u16 },
}

impl IntegrityIssue {
    pub fn verse_key(&self) -> &str {
        match self {
            IntegrityIssue::Duplicate { verse_key }
            | IntegrityIssue::OutOfOrder { verse_key }
            | IntegrityIssue::MissingVerse { verse_key }
            | IntegrityIssue::NoWords { verse_key }
            | IntegrityIssue::WordPositionGap { verse_key, .. }
            | IntegrityIssue::MissingEndMarker { verse_key }
            | IntegrityIssue::ExtraEndMarker { verse_key, .. }
            | IntegrityIssue::EndMarkerNotLast { verse_key, .. }
            | IntegrityIssue::EmptyWordText { verse_key, .. } => verse_key,
        }
    }

    /// Word-level problems: the verse is rendered from verse-level text instead.
    pub fn is_word_issue(&self) -> bool {
        matches!(
            self,
            IntegrityIssue::NoWords { .. }
                | IntegrityIssue::WordPositionGap { .. }
                | IntegrityIssue::MissingEndMarker { .. }
                | IntegrityIssue::ExtraEndMarker { .. }
                | IntegrityIssue::EndMarkerNotLast { .. }
                | IntegrityIssue::EmptyWordText { .. }
        )
    }
}

impl fmt::Display for IntegrityIssue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            IntegrityIssue::Duplicate { verse_key } => write!(f, "{verse_key}: duplicate verse"),
            IntegrityIssue::OutOfOrder { verse_key } => write!(f, "{verse_key}: out of order"),
            IntegrityIssue::MissingVerse { verse_key } => write!(f, "{verse_key}: verse missing"),
            IntegrityIssue::NoWords { verse_key } => write!(f, "{verse_key}: no words"),
            IntegrityIssue::WordPositionGap { verse_key, position } => {
                write!(f, "{verse_key}: word position {position} missing")
            }
            IntegrityIssue::MissingEndMarker { verse_key } => {
                write!(f, "{verse_key}: no end-of-verse marker")
            }
            IntegrityIssue::ExtraEndMarker { verse_key, position } => {
                write!(f, "{verse_key}: extra end marker at word {position}")
            }
            IntegrityIssue::EndMarkerNotLast { verse_key, position } => {
                write!(f, "{verse_key}: end marker at word {position} is not last")
            }
            IntegrityIssue::EmptyWordText { verse_key, position } => {
                write!(f, "{verse_key}: word {position} has no text")
            }
        }
    }
}

/// `(chapter, verse)` sort key; malformed keys sort last.
fn sort_key(v: &Verse) -> (u16, u16) {
    (v.chapter_number().unwrap_or(u16::MAX), v.verse_number)
}

/// (a) Sort verses into mushaf order and drop duplicate `verse_key`s (keeping
/// the first copy). Reports one `OutOfOrder` (first offender) and every duplicate.
pub fn sort_dedup(verses: &mut Vec<Verse>) -> Vec<IntegrityIssue> {
    let mut issues = Vec::new();
    if let Some(w) = verses.windows(2).find(|w| sort_key(&w[0]) > sort_key(&w[1])) {
        issues.push(IntegrityIssue::OutOfOrder { verse_key: w[1].verse_key.clone() });
    }
    // Stable sort keeps the first-arrived copy of a duplicate first.
    verses.sort_by_key(sort_key);
    let mut seen = std::collections::HashSet::new();
    verses.retain(|v| {
        let fresh = seen.insert(v.verse_key.clone());
        if !fresh {
            issues.push(IntegrityIssue::Duplicate { verse_key: v.verse_key.clone() });
        }
        fresh
    });
    issues
}

/// (b) Gaps between consecutive verses of the same chapter (list must be sorted).
pub fn check_gaps(verses: &[Verse]) -> Vec<IntegrityIssue> {
    let mut issues = Vec::new();
    for w in verses.windows(2) {
        let (a, b) = (sort_key(&w[0]), sort_key(&w[1]));
        if a.0 == b.0 && b.1 > a.1.saturating_add(1) {
            for n in a.1 + 1..b.1 {
                issues.push(IntegrityIssue::MissingVerse { verse_key: format!("{}:{n}", a.0) });
            }
        }
    }
    issues
}

/// (b) Every verse of `expected` in `chapter` must be present. Call this only
/// once all pages are loaded (otherwise the tail is merely not loaded yet).
pub fn check_expected(verses: &[Verse], chapter: u16, expected: VerseSpan) -> Vec<IntegrityIssue> {
    let present: std::collections::HashSet<u16> =
        verses.iter().filter(|v| v.chapter_number() == Some(chapter)).map(|v| v.verse_number).collect();
    (expected.from..=expected.to)
        .filter(|n| !present.contains(n))
        .map(|n| IntegrityIssue::MissingVerse { verse_key: format!("{chapter}:{n}") })
        .collect()
}

/// Text of `word` for the script whose API field is `script_field`
/// (`QuranScript::field()`), falling back to `text_uthmani`, then `text`.
pub fn word_text<'a>(word: &'a Word, script_field: &str) -> Option<&'a str> {
    let selected = match script_field {
        "text_indopak" => word.text_indopak.as_deref(),
        "text_imlaei" => word.text_imlaei.as_deref(),
        _ => word.text_uthmani.as_deref(),
    };
    [selected, word.text_uthmani.as_deref(), word.text.as_deref()]
        .into_iter()
        .flatten()
        .find(|t| !t.trim().is_empty())
}

/// Verse-level text for the selected script, falling back to the other scripts.
pub fn verse_text<'a>(verse: &'a Verse, script_field: &str) -> Option<&'a str> {
    let selected = match script_field {
        "text_indopak" => verse.text_indopak.as_deref(),
        "text_imlaei" => verse.text_imlaei.as_deref(),
        _ => verse.text_uthmani.as_deref(),
    };
    [selected, verse.text_uthmani.as_deref(), verse.text_indopak.as_deref(), verse.text_imlaei.as_deref()]
        .into_iter()
        .flatten()
        .find(|t| !t.trim().is_empty())
}

/// (c) + (d) Word structure and text of one verse.
pub fn word_issues(verse: &Verse, script_field: &str) -> Vec<IntegrityIssue> {
    let key = || verse.verse_key.clone();
    let words = &verse.words;
    if words.is_empty() {
        return vec![IntegrityIssue::NoWords { verse_key: key() }];
    }
    let mut issues = Vec::new();
    // Positions: the API sends words in order, 1-based and contiguous.
    let mut expected: u16 = 1;
    for w in words {
        if w.position != expected {
            issues.push(IntegrityIssue::WordPositionGap { verse_key: key(), position: expected });
            break;
        }
        expected = expected.saturating_add(1);
    }
    let ends: Vec<&Word> = words.iter().filter(|w| w.is_end_marker()).collect();
    match ends.as_slice() {
        [] => issues.push(IntegrityIssue::MissingEndMarker { verse_key: key() }),
        [first, rest @ ..] => {
            for extra in rest {
                issues.push(IntegrityIssue::ExtraEndMarker { verse_key: key(), position: extra.position });
            }
            if rest.is_empty() && !words.last().is_some_and(|w| w.is_end_marker()) {
                issues.push(IntegrityIssue::EndMarkerNotLast { verse_key: key(), position: first.position });
            }
        }
    }
    for w in words.iter().filter(|w| !w.is_end_marker()) {
        if word_text(w, script_field).is_none() {
            issues.push(IntegrityIssue::EmptyWordText { verse_key: key(), position: w.position });
        }
    }
    issues
}

/// Whether a verse's words can be rendered as-is.
pub fn words_ok(verse: &Verse, script_field: &str) -> bool {
    word_issues(verse, script_field).is_empty()
}

/// Number of non-end-marker words.
pub fn word_count(verse: &Verse) -> usize {
    verse.words.iter().filter(|w| !w.is_end_marker()).count()
}

/// (b) Gaps across chapter boundaries (list must be sorted). When consecutive
/// verses belong to different chapters, the earlier chapter must end at its last
/// verse, every chapter in between must be fully present, and the later chapter
/// must start at verse 1. `verses_count` gives a chapter's length when known;
/// without it only the "starts at 1" part can be checked.
pub fn check_boundaries(verses: &[Verse], verses_count: impl Fn(u16) -> Option<u16>) -> Vec<IntegrityIssue> {
    let mut issues = Vec::new();
    let missing = |issues: &mut Vec<IntegrityIssue>, c: u16, from: u16, to: u16| {
        for n in from..=to {
            issues.push(IntegrityIssue::MissingVerse { verse_key: format!("{c}:{n}") });
        }
    };
    for w in verses.windows(2) {
        let (a, b) = (sort_key(&w[0]), sort_key(&w[1]));
        if a.0 >= b.0 || b.0 == u16::MAX {
            continue;
        }
        if let Some(last) = verses_count(a.0) {
            missing(&mut issues, a.0, a.1 + 1, last);
        }
        for c in a.0 + 1..b.0 {
            if let Some(last) = verses_count(c) {
                missing(&mut issues, c, 1, last);
            } else {
                issues.push(IntegrityIssue::MissingVerse { verse_key: format!("{c}:1") });
            }
        }
        missing(&mut issues, b.0, 1, b.1.saturating_sub(1));
    }
    issues
}

/// All issues for a loaded list (already sorted/deduped).
/// `expected` lists `(chapter, span)` that must be fully present; pass it only once
/// everything requested has been loaded.
pub fn check_all(
    verses: &[Verse],
    script_field: &str,
    expected: &[(u16, VerseSpan)],
    verses_count: impl Fn(u16) -> Option<u16>,
) -> Vec<IntegrityIssue> {
    let mut issues = check_gaps(verses);
    let add = |found: Vec<IntegrityIssue>, issues: &mut Vec<IntegrityIssue>| {
        for i in found {
            if !issues.contains(&i) {
                issues.push(i);
            }
        }
    };
    add(check_boundaries(verses, &verses_count), &mut issues);
    for &(chapter, span) in expected {
        add(check_expected(verses, chapter, span), &mut issues);
    }
    for v in verses {
        issues.extend(word_issues(v, script_field));
    }
    issues
}

/// Distinct verse keys of issues matching `pred`, in order.
pub fn keys_where(issues: &[IntegrityIssue], pred: impl Fn(&IntegrityIssue) -> bool) -> Vec<String> {
    let mut keys: Vec<String> = Vec::new();
    for i in issues.iter().filter(|i| pred(i)) {
        if !keys.iter().any(|k| k == i.verse_key()) {
            keys.push(i.verse_key().to_string());
        }
    }
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const U: &str = "text_uthmani";

    fn word(pos: u16, kind: &str, text: Option<&str>) -> serde_json::Value {
        json!({ "id": pos, "position": pos, "char_type_name": kind, "text_uthmani": text })
    }

    /// A well-formed verse with `n` words plus an end marker.
    fn verse_with(key: &str, words: Vec<serde_json::Value>) -> Verse {
        let n: u16 = key.split(':').nth(1).unwrap().parse().unwrap();
        serde_json::from_value(json!({
            "id": n, "verse_number": n, "verse_key": key, "text_uthmani": "نص الآية", "words": words
        }))
        .unwrap()
    }

    fn good(key: &str) -> Verse {
        verse_with(
            key,
            vec![word(1, "word", Some("ب")), word(2, "word", Some("س")), word(3, "end", Some("١"))],
        )
    }

    fn keys(vs: &[Verse]) -> Vec<&str> {
        vs.iter().map(|v| v.verse_key.as_str()).collect()
    }

    #[test]
    fn sorts_and_dedups_across_pages() {
        let mut vs = vec![good("2:2"), good("2:1"), good("2:3"), good("2:2"), good("1:7")];
        let issues = sort_dedup(&mut vs);
        assert_eq!(keys(&vs), ["1:7", "2:1", "2:2", "2:3"]);
        assert_eq!(
            issues,
            vec![
                IntegrityIssue::OutOfOrder { verse_key: "2:1".into() },
                IntegrityIssue::Duplicate { verse_key: "2:2".into() },
            ]
        );
        let mut sorted = vec![good("2:1"), good("2:2")];
        assert!(sort_dedup(&mut sorted).is_empty());
    }

    #[test]
    fn page_boundary_gap() {
        // API page 1 ends at 50, page 2 starts at 52: verse 51 is missing.
        let vs = vec![good("2:49"), good("2:50"), good("2:52"), good("2:53")];
        assert_eq!(check_gaps(&vs), vec![IntegrityIssue::MissingVerse { verse_key: "2:51".into() }]);
        // A chapter change is not a gap.
        assert!(check_gaps(&[good("2:286"), good("3:1")]).is_empty());
    }

    #[test]
    fn expected_range_gaps() {
        let vs = vec![good("2:2"), good("2:3")];
        let span = VerseSpan { from: 1, to: 4 };
        assert_eq!(
            check_expected(&vs, 2, span),
            vec![
                IntegrityIssue::MissingVerse { verse_key: "2:1".into() },
                IntegrityIssue::MissingVerse { verse_key: "2:4".into() },
            ]
        );
        assert!(check_expected(&vs, 2, VerseSpan { from: 2, to: 3 }).is_empty());
        // check_all doesn't double-report a gap found both ways.
        let vs = vec![good("2:1"), good("2:3")];
        assert_eq!(
            check_all(&vs, U, &[(2, VerseSpan { from: 1, to: 3 })], |_| None),
            vec![IntegrityIssue::MissingVerse { verse_key: "2:2".into() }]
        );
    }

    #[test]
    fn good_verse_has_no_issues() {
        assert!(word_issues(&good("1:1"), U).is_empty());
        assert!(words_ok(&good("1:1"), "text_indopak")); // falls back to uthmani
        assert_eq!(word_count(&good("1:1")), 2);
    }

    #[test]
    fn no_words() {
        let v = verse_with("1:1", vec![]);
        assert_eq!(word_issues(&v, U), vec![IntegrityIssue::NoWords { verse_key: "1:1".into() }]);
        assert!(!words_ok(&v, U));
    }

    #[test]
    fn position_gap() {
        let v = verse_with(
            "1:1",
            vec![word(1, "word", Some("a")), word(3, "word", Some("b")), word(4, "end", Some("١"))],
        );
        assert_eq!(
            word_issues(&v, U),
            vec![IntegrityIssue::WordPositionGap { verse_key: "1:1".into(), position: 2 }]
        );
        let v = verse_with("1:1", vec![word(2, "word", Some("a")), word(3, "end", Some("١"))]);
        assert_eq!(
            word_issues(&v, U),
            vec![IntegrityIssue::WordPositionGap { verse_key: "1:1".into(), position: 1 }]
        );
    }

    #[test]
    fn end_marker_rules() {
        let missing = verse_with("1:1", vec![word(1, "word", Some("a"))]);
        assert_eq!(
            word_issues(&missing, U),
            vec![IntegrityIssue::MissingEndMarker { verse_key: "1:1".into() }]
        );
        let not_last = verse_with("1:1", vec![word(1, "end", Some("١")), word(2, "word", Some("a"))]);
        assert_eq!(
            word_issues(&not_last, U),
            vec![IntegrityIssue::EndMarkerNotLast { verse_key: "1:1".into(), position: 1 }]
        );
        let two = verse_with("1:1", vec![word(1, "end", Some("١")), word(2, "end", Some("١"))]);
        assert_eq!(
            word_issues(&two, U),
            vec![IntegrityIssue::ExtraEndMarker { verse_key: "1:1".into(), position: 2 }]
        );
    }

    #[test]
    fn empty_word_text_and_fallbacks() {
        let v =
            verse_with("1:1", vec![word(1, "word", None), word(2, "word", Some("  ")), word(3, "end", None)]);
        assert_eq!(
            word_issues(&v, U),
            vec![
                IntegrityIssue::EmptyWordText { verse_key: "1:1".into(), position: 1 },
                IntegrityIssue::EmptyWordText { verse_key: "1:1".into(), position: 2 },
            ]
        );
        // `text` is the last fallback; the selected script wins when present.
        let w: Word = serde_json::from_value(json!({
            "id": 1, "position": 1, "char_type_name": "word", "text": "t", "text_indopak": "i"
        }))
        .unwrap();
        assert_eq!(word_text(&w, U), Some("t"));
        assert_eq!(word_text(&w, "text_indopak"), Some("i"));
    }

    #[test]
    fn verse_text_fallback() {
        let v: Verse = serde_json::from_value(json!({
            "id": 1, "verse_number": 1, "verse_key": "1:1", "text_indopak": "indo"
        }))
        .unwrap();
        assert_eq!(verse_text(&v, U), Some("indo"));
        assert_eq!(verse_text(&good("1:1"), "text_indopak"), Some("نص الآية"));
    }

    #[test]
    fn keys_filtering() {
        let issues = vec![
            IntegrityIssue::EmptyWordText { verse_key: "2:5".into(), position: 1 },
            IntegrityIssue::EmptyWordText { verse_key: "2:5".into(), position: 2 },
            IntegrityIssue::MissingVerse { verse_key: "2:7".into() },
            IntegrityIssue::NoWords { verse_key: "2:9".into() },
        ];
        assert_eq!(keys_where(&issues, IntegrityIssue::is_word_issue), ["2:5", "2:9"]);
        assert_eq!(keys_where(&issues, |i| matches!(i, IntegrityIssue::MissingVerse { .. })), ["2:7"]);
        assert_eq!(issues[0].to_string(), "2:5: word 1 has no text");
    }

    #[test]
    fn boundary_gap_at_chapter_start_is_flagged() {
        // Juz 30: 78:46 then 79:2 -> 79:1 missing (no chapter lengths needed).
        let vs = vec![good("78:46"), good("79:2")];
        assert_eq!(
            check_boundaries(&vs, |_| None),
            vec![IntegrityIssue::MissingVerse { verse_key: "79:1".into() }]
        );
    }

    #[test]
    fn boundary_gap_at_chapter_end_and_whole_chapter_flagged() {
        let counts = |c: u16| match c {
            112 => Some(4),
            113 => Some(5),
            _ => None,
        };
        // 112:3 then 114:1 -> 112:4 and all of 113 missing.
        let vs = vec![good("112:3"), good("114:1")];
        let keys: Vec<String> = check_boundaries(&vs, counts).iter().map(|i| i.verse_key().to_string()).collect();
        assert_eq!(keys, vec!["112:4", "113:1", "113:2", "113:3", "113:4", "113:5"]);
        // Clean boundary -> nothing.
        assert!(check_boundaries(&[good("112:4"), good("113:1")], counts).is_empty());
    }

    #[test]
    fn check_all_checks_every_expected_span() {
        let vs = vec![good("78:40"), good("79:1")];
        let spans = [(78, VerseSpan { from: 39, to: 40 }), (79, VerseSpan { from: 1, to: 2 })];
        let keys: Vec<String> =
            check_all(&vs, U, &spans, |_| None).iter().map(|i| i.verse_key().to_string()).collect();
        assert_eq!(keys, vec!["78:39", "79:2"]);
    }
}
