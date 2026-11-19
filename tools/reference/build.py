#!/usr/bin/env python3
"""Rebuild the reference tables the app checks loaded verses against.

Fetches every verse of the Quran with its words from the Quran.com API (about
150 requests) and writes, next to the Rust code that embeds them:

  app/src/reader/reference/words_per_verse.bin   one byte per verse, in mushaf
      order: its number of words (the ayah-number marker not counted)
  app/src/reader/reference/page_starts.txt       604 lines, "chapter:verse" of
      the first verse that begins on each mushaf page

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


def verses(chapter, mushaf, cache):
    page = 1
    while page:
        data = fetch(f"{API}/verses/by_chapter/{chapter}?words=true&per_page=50&page={page}&mushaf={mushaf}&word_fields=v2_page", cache)
        yield from data["verses"]
        page = data["pagination"]["next_page"]


def main():
    cache = pathlib.Path(sys.argv[sys.argv.index("--cache") + 1]) if "--cache" in sys.argv else ROOT / "target/reference-cache"
    cache.mkdir(parents=True, exist_ok=True)
    chapters = fetch(f"{API}/chapters?language=en", cache)["chapters"]
    assert [c["id"] for c in chapters] == list(range(1, 115)), "expected 114 chapters in order"
    counts, starts = bytearray(), {}
    for c in chapters:
        expected = 1
        for v in verses(c["id"], MUSHAF, cache):
            assert v["verse_key"] == f"{c['id']}:{expected}", f"{v['verse_key']} out of order"
            expected += 1
            words = v["words"]
            assert [w["position"] for w in words] == list(range(1, len(words) + 1)), v["verse_key"]
            assert [w["char_type_name"] == "end" for w in words] == [False] * (len(words) - 1) + [True], v["verse_key"]
            n = len(words) - 1
            assert 0 < n < 256, v["verse_key"]
            counts.append(n)
            starts.setdefault(words[0]["page_number"], v["verse_key"])
            # `/verses/by_page` lists a verse on the page it begins on.
            assert v.get("page_number") in (None, words[0]["page_number"]), v["verse_key"]
        assert expected - 1 == c["verses_count"], f"surah {c['id']} has {expected - 1} verses, not {c['verses_count']}"
    assert len(counts) == 6236, len(counts)
    assert sorted(starts) == list(range(1, 605)), "every page 1..604 must start a verse"
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "words_per_verse.bin").write_bytes(bytes(counts))
    (OUT / "page_starts.txt").write_text("".join(f"{starts[p]}\n" for p in range(1, 605)))
    (OUT / "verses_per_chapter.txt").write_text("".join(f"{c['verses_count']}\n" for c in chapters))
    print(f"{len(counts)} verses, {sum(counts)} words, 604 pages -> {OUT}")


if __name__ == "__main__":
    main()
