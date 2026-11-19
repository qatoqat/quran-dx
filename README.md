# The Noble Quran — Dioxus port

A Rust/[Dioxus 0.7](https://dioxuslabs.com) port of the
[quran.com-frontend-v2](https://github.com/quran/quran.com-frontend-v2) reader.
One codebase builds both a **web app** (WebAssembly) and a **desktop app**.

Data comes from the public [Quran.com API v4](https://api-docs.quran.com)
(`https://api.quran.com/api/v4`) instead of the original's Postgres + Elasticsearch.

## Features

- **Reader:** list of 114 surahs and 30 juz, surah view with Bismillah and "load more",
  verse ranges (`/2/1-5`, `/2/255`, plus legacy `/2:255`-style URLs), mushaf page view
  (`/page/1`…`/page/604`), juz view (`/juz/1`…`/juz/30`).
- **Settings:** UI/word language, multiple translations, Uthmani or IndoPak script, Arabic
  and translation font sizes, word-by-word translation/transliteration (tooltip or
  inline), footnotes. Saved to localStorage (web) or `~/.config/quran-app/settings.json` (desktop).
- **Search:** header search with suggestions and shortcuts (`2:255`, `juz 30`, `page 3`,
  surah names) plus a full results page with highlighted matches.

Out of scope for this port: audio recitation, tafsir, surah info, bookmarks, sharing and
advanced copy, static pages, PWA/service worker.

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

## Layout

```
crates/quran-api/   typed async API client (reqwest; fetch on wasm) + fixtures
app/src/route.rs    URL routes (mirror quran.com URLs)
app/src/reader/     surah list, chapter/range/page/juz views, verse rendering
app/src/settings/   settings state, persistence, settings page, word-by-word, footnotes
app/src/search/     search box, shortcut parser, results page
app/assets/         CSS
```

## Notes

- The API base URL is set in `quran_api::DEFAULT_BASE_URL`. Quran.com has been moving
  API access to the Quran Foundation API, so if the public v4 endpoints start requiring
  auth, change it there and add the token header in `crates/quran-api/src/client.rs`.
- Translation/footnote/search HTML from the API is never injected as raw HTML; it is
  tokenized and rendered as text with only footnote and highlight markup kept.

## License

GPL-3.0-or-later, same as the original project.
