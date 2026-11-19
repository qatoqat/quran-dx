# The Noble Quran — Dioxus port

A Rust/[Dioxus 0.7](https://dioxuslabs.com) port of the
[quran.com-frontend-v2](https://github.com/quran/quran.com-frontend-v2) reader.
One codebase builds both a **web app** (WebAssembly) and a **desktop app**.

Live: <https://qatoqat.com/quran-dx/>

Data comes from the public [Quran.com API v4](https://api-docs.quran.com)
(`https://api.quran.com/api/v4`) instead of the original's Postgres + Elasticsearch.

## Features

- **15-line Madani mushaf everywhere** (Uthman Taha's calligraphy): surah (`/53` or
  `/an-najm`), verse range (`/2/1-5`, `/al-baqarah/255`), juz (`/juz/30`) and page
  (`/page/1`…`/page/604`) views all show real mushaf pages, every word on its printed
  line. Words are drawn with the King Fahd (KFGQPC) QCF per-page fonts, with tajweed
  colours (QCF V4) by default or plain (QCF V2); ayah medallions come from the fonts.
  A surah banner and the Bismillah are inserted where a surah starts. A mushaf line
  is never wrapped or cut: if lines don't fit the window, all pages on screen are
  scaled down together, so text size and line width stay the same from page to page.
- **English word-by-word** meaning under each word (or on hover), optional
  transliteration, and optional English verse translations under each page, with footnotes.
- **Header:** the view's title (`Surah Al-Baqarah`, `Juz 2`, `Page 22`), a search button,
  a Surah | Juz | Page toggle and a settings button, kept to the width of the page
  content. The toggle's current button flips between that mode's list (`/` for surahs,
  `/juz`; pages have none) and reading (back to the last place read); the other two open
  the line being read in their view. Each button's tooltip says where it leads.
- **Reload-proof URLs:** the path is the view and the hash is the reading position — the
  word that starts the top line, e.g. `/2#2:142` or `/juz/2#2:282:45` — updated as you
  scroll, so a reload (or a shared link) comes back to the same line. The desktop app
  has no URL bar; it saves the location in `~/.config/quran-app/location` and starts there.
- **Settings:** a sidebar floating over the right edge (the page behind it is the live
  preview): theme (auto, light, dark, sepia, or custom: pick a background and an accent and the
  other colours are worked out from them), mushaf font, word-by-word options, English
  translations, text sizes, space between words, above word meanings and under each
  line, the width of the content column, a reading pointer, and a custom font source. Saved to localStorage (web) or
  `~/.config/quran-app/settings.json` (desktop).
- **Reading pointer** (optional): a dot or ring replaces the mouse cursor below the
  header, with its own colour, size (in tenths of a line's height), opacity, trail
  (count and distance), blur, shrinking with speed, and glow (stronger colours of the text under it). The cursor comes back over the
  header and the settings.
- **Search:** the header's search button opens a box in the middle of the header, with
  suggestions and shortcuts (`2:255`, `juz 30`, `page 3`, surah names), plus a full
  results page with highlighted matches.

Out of scope: audio recitation, tafsir, surah info, bookmarks, static pages, PWA.

### Fonts

Page fonts are downloaded at runtime from quran.com's font servers
(`verses.quran.foundation`, then `quran.com`) with `fetch` and registered from the
bytes, the same way in the browser and in the desktop webview. Chromium and Firefox
get the COLRv1 tajweed fonts (dark mode uses the font's dark palette); WebKit (the
Linux desktop webview, Safari) can't switch COLRv1 palettes, so it gets quran.com's
OpenType-SVG fonts with light or dark colours baked in.

Until a page's font has loaded — or if it can't load — its words are shown as Unicode
text instead, so no word is ever blank: each word's part of the verse's
`qpc_uthmani_hafs` text in the KFGQPC Hafs font (which expects that encoding), or its
`text_uthmani` in Amiri Quran. If fonts can't be fetched at all, self-host a copy of
quran.com's `fonts/quran` folder and set **Settings → Advanced → Font source URL**.

The surah banner is the printed-mushaf one: an ornamental frame with the calligraphic
"سورة <name>" fitted into its cartouche, the place of revelation and the number of
verses in its two roundels, one mushaf line tall. Frame and names are two ligature
fonts from QUL, the Quranic Universal Library (`quran-common` and `surah-name-v4`),
loaded at runtime from `static-cdn.tarteel.ai`. QUL lists no licence information for
them, so they are not bundled. Until they are loaded and verified to draw the
banner, the same content is shown as a plain framed line.

## Requirements

- Rust **1.100** (pinned in `rust-toolchain.toml`; rustup installs it and the
  `wasm32-unknown-unknown` target automatically)
- Desktop on Linux needs WebKitGTK 4.1 and its usual companions:
  - Arch: `sudo pacman -S webkit2gtk-4.1 xdotool`
  - Debian/Ubuntu: `sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libxdo-dev libssl-dev`

  (macOS and Windows need nothing extra)
- For the web build and bundling, the Dioxus CLI:
  `cargo install dioxus-cli --version 0.7.10 --locked`

## Run

Desktop (the default feature), with plain cargo or with `dx` for hot reload:

```bash
cargo run -p quran-app
cd app && dx serve --platform desktop
```

Web:

```bash
cd app && dx serve --platform web       # http://localhost:8080
```

Release builds:

```bash
cd app
dx bundle --platform desktop --release
dx bundle --platform web --release
```

The web app is deployed to GitHub Pages by `.github/workflows/pages.yml` on every push
to `main`: it runs the tests, bundles with the repository name as base path, and copies
`index.html` to `404.html` so any URL opens the app.

`dx` selects the `desktop` or `web` feature itself. The CSS is compiled into the
binary, so a plain `cargo` build is fully styled too.

On Linux the window uses the desktop's own frame (KDE's title bar and buttons under
KWin) and has no in-window menu bar.

### Install on Arch Linux

`packaging/arch/PKGBUILD` builds the working tree into a `quran-dx` package: the
`quran-dx` command, a menu entry ("The Noble Quran") and an icon.

```bash
paru -Bi --rebuild packaging/arch  # or: cd packaging/arch && makepkg -sif
```

Run it again after changes to rebuild and upgrade in place (`--rebuild` matters: the
version doesn't change, so without it the previous package file is reused); `pacman -R quran-dx`
removes it. Settings stay in `~/.config/quran-app/`.

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

Three small tables are compiled into the app as an independent reference
(`app/src/reader/reference/`, rebuilt with `python3 tools/reference/build.py`): the number
of verses in each surah, the number of words in each of the 6,236 verses, and the verse
each of the 604 pages begins with. A response that has lost a word, a verse at the edge
of a page, or the end of a surah is therefore caught even when it looks consistent in
itself. The lookups are array indexes, so they cost nothing measurable.

The app also checks what is actually on screen. Whenever a mushaf page scrolls into
view (and has really been rendered), every word and ayah marker on it is looked up in
the page and must exist exactly once, show exactly the expected text, be visible (not
hidden, transparent, tiny or zero-sized), not be cut off (the whole word and its
meaning must be inside the window and its page), and, for glyphs, use a page font the
browser reports as loaded. A word that fails is switched to plain Unicode text with a
red ⚠ marker, and the page shows an alert listing the affected words — rendering
problems are never silent.

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
app/src/route.rs    URL routes (mirror quran.com URLs) and the reading position in the hash
app/src/layout.rs   header: title, search, surah / juz / page toggle, settings
app/src/reader/     surah and juz lists, views, mushaf layout (pages/lines), QCF glyphs,
                    font loading, line fitting, the on-screen audit, position tracking
app/src/settings/   settings state, persistence, settings panel, translations, footnotes
app/src/search/     search box, shortcut parser, results page
app/assets/         CSS (compiled into the binary)
tools/word-check/   browser test: rendered words vs. API
tools/reference/    rebuilds the compiled-in reference tables from the API
packaging/          Arch Linux package, desktop entry and icon
```

## Notes

- The API base URL is set in `quran_api::DEFAULT_BASE_URL`. Quran.com has been moving
  API access to the Quran Foundation API, so if the public v4 endpoints start requiring
  auth, change it there and add the token header in `crates/quran-api/src/client.rs`.
- Translation/footnote/search HTML from the API is never injected as raw HTML; it is
  tokenized and rendered as text with only footnote and highlight markup kept.

## License

GPL-3.0-or-later, same as the original project. See [LICENSE](LICENSE).
