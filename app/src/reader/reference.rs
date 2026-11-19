//! Reference data the loaded verses are checked against, so a response that
//! lacks a verse or a word is noticed even when it is consistent in itself.
//!
//! Three small tables are compiled in (built by `tools/reference/build.py` from
//! a full read of the API): the number of verses in each surah, the number of
//! words in each verse, and the verse each mushaf page begins with. Looking
//! something up is an array index.

use std::sync::OnceLock;

use super::parse::{CHAPTER_COUNT, VerseSpan};

/// One byte per verse in mushaf order: its words, not counting the ayah marker.
const WORDS_PER_VERSE: &[u8] = include_bytes!("reference/words_per_verse.bin");
/// One line per surah: its number of verses.
const VERSES_PER_CHAPTER: &str = include_str!("reference/verses_per_chapter.txt");
/// One line per page: `chapter:verse` of the first verse that begins on it.
const PAGE_STARTS: &str = include_str!("reference/page_starts.txt");

struct Tables {
    /// Verses per surah (index 0 = surah 1).
    verses: Vec<u16>,
    /// Index into `WORDS_PER_VERSE` of each surah's first verse.
    offsets: Vec<usize>,
    /// First verse of each page (index 0 = page 1).
    page_starts: Vec<(u16, u16)>,
}

fn tables() -> &'static Tables {
    static TABLES: OnceLock<Tables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let verses: Vec<u16> = VERSES_PER_CHAPTER.lines().filter_map(|l| l.trim().parse().ok()).collect();
        let offsets = verses
            .iter()
            .scan(0usize, |next, n| {
                let start = *next;
                *next += usize::from(*n);
                Some(start)
            })
            .collect();
        let page_starts = PAGE_STARTS
            .lines()
            .filter_map(|l| {
                let (c, v) = l.trim().split_once(':')?;
                Some((c.parse().ok()?, v.parse().ok()?))
            })
            .collect();
        Tables { verses, offsets, page_starts }
    })
}

/// Number of verses in a surah.
pub fn verses_in(chapter: u16) -> Option<u16> {
    tables().verses.get(usize::from(chapter.checked_sub(1)?)).copied()
}

/// Number of words in a verse (the ayah marker not counted).
pub fn words_in(chapter: u16, verse: u16) -> Option<usize> {
    let t = tables();
    let c = usize::from(chapter.checked_sub(1)?);
    if verse == 0 || verse > *t.verses.get(c)? {
        return None;
    }
    WORDS_PER_VERSE.get(t.offsets[c] + usize::from(verse) - 1).map(|n| usize::from(*n))
}

/// The verses that begin on a mushaf page, as `(chapter, span)` per surah.
pub fn verses_on_page(page: u16) -> Vec<(u16, VerseSpan)> {
    let t = tables();
    let Some(&(first_chapter, first_verse)) = page.checked_sub(1).and_then(|p| t.page_starts.get(usize::from(p))) else {
        return Vec::new();
    };
    // Up to, not including, the next page's first verse (or the end of the Quran).
    let (end_chapter, end_verse) = t.page_starts.get(usize::from(page)).copied().unwrap_or((CHAPTER_COUNT + 1, 1));
    let mut spans = Vec::new();
    for chapter in first_chapter..=end_chapter.min(CHAPTER_COUNT) {
        let from = if chapter == first_chapter { first_verse } else { 1 };
        let to = if chapter == end_chapter { end_verse - 1 } else { verses_in(chapter).unwrap_or(0) };
        if from <= to {
            spans.push((chapter, VerseSpan { from, to }));
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::parse::MUSHAF_PAGES;

    #[test]
    fn tables_are_whole() {
        let t = tables();
        assert_eq!(t.verses.len(), usize::from(CHAPTER_COUNT));
        assert_eq!(t.verses.iter().map(|n| u32::from(*n)).sum::<u32>(), 6236);
        assert_eq!(WORDS_PER_VERSE.len(), 6236);
        assert!(WORDS_PER_VERSE.iter().all(|n| *n > 0));
        assert_eq!(t.page_starts.len(), usize::from(MUSHAF_PAGES));
        assert!(t.page_starts.windows(2).all(|w| w[0] < w[1]), "pages begin in mushaf order");
    }

    #[test]
    fn lookups() {
        assert_eq!((verses_in(1), verses_in(2), verses_in(114)), (Some(7), Some(286), Some(6)));
        assert_eq!((verses_in(0), verses_in(115)), (None, None));
        // 1:1 is the Bismillah (4 words); 112:1 "قل هو الله أحد" has 4, 108:1 has 3.
        assert_eq!((words_in(1, 1), words_in(112, 1), words_in(108, 1)), (Some(4), Some(4), Some(3)));
        assert_eq!((words_in(1, 8), words_in(1, 0), words_in(115, 1)), (None, None, None));
        // The last verse of the table is 114:6.
        assert!(words_in(114, 6).is_some());
    }

    #[test]
    fn pages_cover_every_verse_once() {
        assert_eq!(verses_on_page(1), [(1, VerseSpan { from: 1, to: 7 })]);
        assert_eq!(verses_on_page(2), [(2, VerseSpan { from: 1, to: 5 })]);
        // The last page holds the last three surahs.
        let last = verses_on_page(604);
        assert_eq!(last.iter().map(|(c, _)| *c).collect::<Vec<_>>(), [112, 113, 114]);
        assert_eq!(last[2], (114, VerseSpan { from: 1, to: 6 }));
        assert!(verses_on_page(0).is_empty() && verses_on_page(605).is_empty());
        let total: u32 = (1..=MUSHAF_PAGES)
            .flat_map(verses_on_page)
            .map(|(_, s)| u32::from(s.to - s.from) + 1)
            .sum();
        assert_eq!(total, 6236);
    }
}
