# The Noble Quran — Dioxus port

A Rust/[Dioxus 0.7](https://dioxuslabs.com) port of the
[quran.com-frontend-v2](https://github.com/quran/quran.com-frontend-v2) reader.
One codebase builds both a **web app** (WebAssembly) and a **desktop app**.

Data comes from the public [Quran.com API v4](https://api-docs.quran.com)
(`https://api.quran.com/api/v4`) instead of the original's Postgres + Elasticsearch.

## Features

- **15-line Madani mushaf everywhere** (Uthman Taha's calligraphy): surah (`/53` or
  `/an-najm`), verse range (`/2/1-5`, `/al-baqarah/255`), juz (`/juz/30`) and page
  (`/page/1`…`/page/604`) views all show real mushaf pages, every word on its printed
  line. Words are drawn with the King Fahd (KFGQPC) QCF per-page fonts, with tajweed
  colours (QCF V4) by default or plain (QCF V2); ayah medallions come from the fonts.
  Surah headers and the Bismillah are inserted where a surah starts.
- **English word-by-word** meaning under each word (or on hover), optional
  transliteration, and optional English verse translations under each page, with footnotes.
- **Settings:** mushaf font, word-by-word options, English translations, text sizes, and a
  custom font source. Saved to localStorage (web) or `~/.config/quran-app/settings.json` (desktop).
- **Search:** header search with suggestions and shortcuts (`2:255`, `juz 30`, `page 3`,
  surah names) plus a full results page with highlighted matches.

Out of scope: audio recitation, tafsir, surah info, bookmarks, static pages, PWA.

### Fonts

Page fonts load at runtime from quran.com's font servers (`static.qurancdn.com`, then
`quran.com`). Until a page's font has loaded — or if it can't load — its words are shown
as Unicode text in the KFGQPC Hafs font instead, so no word is ever blank. On desktop the
fonts are downloaded natively first (the `native-fonts` feature, on with `desktop`), which
avoids browser cross-origin limits. If the web build can't load them (cross-origin
blocking), self-host a copy of quran.com's `fonts/quran` folder and set
**Settings → Advanced → Font source URL**.

## Requirements

- Rust **1.100** (pinned in `rust-toolchain.toml`; rustup installs it and the
  `wasm32-unknown-unknown` target automatically)
- Dioxus CLI: `cargo install dioxus-cli --version 0.7.10 --locked`
- Desktop on Linux needs WebKitGTK:
  `sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libxdo-dev libssl-dev`
  (macOS and Windows need nothing extra)

## Run

```bash
cd app
dx serve --platform web       # opens http://localhost:8080
dx serve --platform desktop   # native window
```

Release builds:

```bash
cd app
dx bundle --platform web --release
dx bundle --platform desktop --release
```

If `dx` complains about both features being enabled, add
`--no-default-features --features desktop` to the desktop command.

## Tests

```bash
cargo test --workspace                          # offline tests, using recorded fixtures
cargo test -p quran-api -- --ignored            # live tests against api.quran.com
cargo clippy --workspace --all-targets -- -D warnings
```

## Checking that no word is missing

The app checks itself as it loads: each verse must have contiguous word positions ending
in its ayah marker, every word must have text, and no verse in a range may be skipped. If a
verse's word data is incomplete, the full verse text is shown instead and a warning banner
names the verse. Words never render empty; a visible `⟨?⟩` marks text the API didn't send.

These checks need the real API, so they run on your machine:

```bash
# 1. Data: all 6,236 verses / every word from the API (~190 requests)
cargo test -p quran-api --test live_integrity -- --ignored --nocapture

# 2. Mushaf layout data: all 604 pages — every word has a glyph, its page, a line 1-15
cargo test -p quran-api --test live_mushaf -- --ignored --nocapture

# 3. Screen: what the web app actually renders vs. the API
cd app && dx serve --platform web          # in one terminal
cd tools/word-check && npm install && npx playwright install chromium
node check-words.mjs                        # surahs 1-114, sample ranges, juz 1-30
node check-words.mjs --views pages          # all 604 mushaf pages
node check-words.mjs --font plain --wbw tooltip   # other render modes
```

The browser check compares, word by word, what is on screen with the API: every word
present in order at its `chapter:verse:position`, drawn with the right glyph (or the right
Unicode text where the font didn't load), on the right page and line. Missing, duplicated
or misplaced verses and any warning banner are problems; words shown as Unicode fallback
are reported separately (`--allow-fallback` to accept them). It writes `report.json` and
exits non-zero on any problem.

`cargo test` also mounts every route in a virtual DOM against a small fake API
(`app/src/mount_tests.rs`), so a component that panics or renders nothing fails the suite.

## Layout

```
crates/quran-api/   typed async API client (reqwest; fetch on wasm) + fixtures
app/src/route.rs    URL routes (mirror quran.com URLs)
app/src/reader/     surah list, views, mushaf layout (pages/lines), QCF glyphs, font loading
app/src/settings/   settings state, persistence, settings page, translations, footnotes
app/src/search/     search box, shortcut parser, results page
app/assets/         CSS
tools/word-check/   browser test: rendered words vs. API
```

## Notes

- The API base URL is set in `quran_api::DEFAULT_BASE_URL`. Quran.com has been moving
  API access to the Quran Foundation API, so if the public v4 endpoints start requiring
  auth, change it there and add the token header in `crates/quran-api/src/client.rs`.
- Translation/footnote/search HTML from the API is never injected as raw HTML; it is
  tokenized and rendered as text with only footnote and highlight markup kept.

## License

GPL-3.0-or-later, same as the original project.
