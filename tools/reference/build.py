#!/usr/bin/env python3
"""Rebuild the reference tables the app checks loaded verses against.

Fetches every verse of the Quran with its words from the Quran.com API (about
380 requests: each surah once per layout, see below) and writes, next to the
Rust code that embeds them, in app/src/reader/reference/:

  verses_per_chapter.txt  114 lines: verses in each surah
  words_per_verse.bin     one byte per verse, in mushaf order: its number of
                          words (the ayah-number marker not counted)
  words_per_line.bin      604 x 15 bytes: words (markers included) on each line
                          of each mushaf page; 0 = no text on that line (surah
                          banner, Bismillah, or a line the page doesn't have)
  page_verses.txt         604 lines "c:v c:v": the first and the last verse
                          with a word on that page
  listing_starts.txt      604 lines "c:v": the first verse `/verses/by_page/N`
                          returns for page N

The last two differ because the API knows two layouts. Its page listings follow
an older one; the page and line numbers it returns when `code_v2` glyphs are
requested, as the app does, are the layout the QCF V2/V4 fonts are drawn for,
and at some page turns a verse sits on the other side (83:34 is listed under
page 588 but printed on line 1 of page 589).

Run it only to pick up a deliberate change in the API's data; the tables are
what protects the app from an accidental one.

    python3 tools/reference/build.py [--cache DIR]
"""
import json, pathlib, subprocess, sys

API = "https://api.quran.com/api/v4"
ROOT = pathlib.Path(__file__).resolve().parents[2]
OUT = ROOT / "app/src/reader/reference"
MUSHAF = 19  # QCF V4 tajweed; QCF V2 (1) has the same words and pages


def fetch(url, cache):
    name = cache / (url.split("/v4/")[1].replace("/", "_").replace("?", "_").replace("&", "_") + ".json")
    if not name.exists():
        for attempt in range(4):
            done = subprocess.run(["curl", "-sf", "--max-time", "60", "-A", "quran-reference/1.0", url, "-o", str(name)])
            if done.returncode == 0:
                break
            name.unlink(missing_ok=True)
        else:
            sys.exit(f"could not fetch {url}")
    return json.loads(name.read_text())


def verses(chapter, mushaf, cache, fields):
    page = 1
    while page:
        data = fetch(f"{API}/verses/by_chapter/{chapter}?words=true&per_page=50&page={page}&mushaf={mushaf}&word_fields={fields}", cache)
        yield from data["verses"]
        page = data["pagination"]["next_page"]


# The page and line numbers the API returns follow the glyph code that is asked
# for: with `code_v2` (what the app requests) they are the layout of the QCF
# V2/V4 fonts; without it they are the older layout, which the page listings
# (`/verses/by_page/N`) follow whatever is asked.
LISTING_FIELDS = "v2_page"
LAYOUT_FIELDS = "code_v2,v2_page,page_number,line_number"


def key(verse_key):
    c, v = verse_key.split(":")
    return int(c), int(v)


def main():
    cache = pathlib.Path(sys.argv[sys.argv.index("--cache") + 1]) if "--cache" in sys.argv else ROOT / "target/reference-cache"
    cache.mkdir(parents=True, exist_ok=True)
    chapters = fetch(f"{API}/chapters?language=en", cache)["chapters"]
    assert [c["id"] for c in chapters] == list(range(1, 115)), "expected 114 chapters in order"
    counts, listing = bytearray(), {}
    lines = [[0] * 15 for _ in range(604)]
    first, last = {}, {}
    for c in chapters:
        for v in verses(c["id"], MUSHAF, cache, LISTING_FIELDS):
            listing.setdefault(v["words"][0]["page_number"], v["verse_key"])
        expected = 1
        for v in verses(c["id"], MUSHAF, cache, LAYOUT_FIELDS):
            assert v["verse_key"] == f"{c['id']}:{expected}", f"{v['verse_key']} out of order"
            expected += 1
            words = v["words"]
            assert [w["position"] for w in words] == list(range(1, len(words) + 1)), v["verse_key"]
            assert [w["char_type_name"] == "end" for w in words] == [False] * (len(words) - 1) + [True], v["verse_key"]
            n = len(words) - 1
            assert 0 < n < 256, v["verse_key"]
            counts.append(n)
            prev = None
            for w in words:
                slot = (w["page_number"], w["line_number"])
                assert 1 <= slot[0] <= 604 and 1 <= slot[1] <= 15, (v["verse_key"], slot)
                # This is the fonts' layout: a word's glyph is in the font of the page it is on.
                assert w["v2_page"] == slot[0], f"{v['verse_key']} word {w['position']}: page {slot[0]}, font page {w['v2_page']}"
                # The app's rule for a marker the API places before the word it follows (84:21).
                if w["char_type_name"] == "end" and prev and slot < prev:
                    print(f"  marker of {v['verse_key']} is at {slot}, before its verse's last word at {prev}: kept with the word")
                    slot = prev
                assert prev is None or slot >= prev, f"{v['verse_key']} goes backwards at word {w['position']}"
                prev = slot
                lines[slot[0] - 1][slot[1] - 1] += 1
                first.setdefault(slot[0], v["verse_key"])
                last[slot[0]] = v["verse_key"]
        assert expected - 1 == c["verses_count"], f"surah {c['id']} has {expected - 1} verses, not {c['verses_count']}"
    assert len(counts) == 6236, len(counts)
    assert sorted(listing) == sorted(first) == list(range(1, 605)), "every page 1..604 must have verses"
    assert all(n < 256 for page in lines for n in page)
    starts = [key(listing[p]) for p in range(1, 605)]
    assert starts == sorted(starts), "listings in order"
    # The app fetches a page's listing in one request of 50 verses.
    ends = starts[1:] + [(115, 1)]
    per_chapter = {c["id"]: c["verses_count"] for c in chapters}
    def verses_between(a, b):
        return sum((b[1] if ch == b[0] else per_chapter[ch] + 1) - (a[1] if ch == a[0] else 1) for ch in range(a[0], min(b[0], 114) + 1))
    longest = max(verses_between(a, b) for a, b in zip(starts, ends))
    assert longest <= 50, f"a page listing has {longest} verses"
    moved = [p for p in range(1, 605) if listing[p] != first[p]]
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "words_per_verse.bin").write_bytes(bytes(counts))
    (OUT / "words_per_line.bin").write_bytes(bytes(n for page in lines for n in page))
    (OUT / "page_verses.txt").write_text("".join(f"{first[p]} {last[p]}\n" for p in range(1, 605)))
    (OUT / "listing_starts.txt").write_text("".join(f"{listing[p]}\n" for p in range(1, 605)))
    (OUT / "verses_per_chapter.txt").write_text("".join(f"{c['verses_count']}\n" for c in chapters))
    stale = OUT / "page_starts.txt"
    if stale.exists():
        stale.unlink()
    text_lines = sum(1 for page in lines for n in page if n)
    print(f"{len(counts)} verses, {sum(counts)} words, {text_lines} text lines on 604 pages -> {OUT}")
    print(f"longest page listing: {longest} verses; pages whose first verse differs from their listing: {len(moved)} {moved[:40]}")


if __name__ == "__main__":
    main()
