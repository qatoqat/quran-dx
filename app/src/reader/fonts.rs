//! Loading the King Fahd QCF per-page fonts (and the Unicode fallback face).
//!
//! Each page font is registered once per font choice with the FontFace API
//! (`document.fonts.add` + `await face.load()`), trying every base URL from
//! `Settings::font_bases()` in order. Per-page state lives in [`FONTS`]; words
//! switch from Unicode text to glyphs only when their page is `Loaded`.
//!
//! After loading, a tiny canvas probe draws one glyph of the page: if nothing is
//! painted (e.g. a webview without COLRv1 colour-font support), the page is
//! marked `Failed` so words stay readable as Unicode text instead of blank.
//!
//! Desktop (`native-fonts` feature, non-wasm): the font bytes are downloaded with
//! reqwest and handed to `new FontFace(name, bytes)`, which avoids cross-origin
//! font fetch restrictions in the webview; the URL path is the fallback.

use std::collections::HashMap;

use dioxus::prelude::*;

use crate::settings::{MushafFont, use_settings};

/// Unicode Hafs face used for text-mode words (first in `.unicode-text`'s font stack in reader.css).
pub const FALLBACK_FACE: &str = "UthmanicHafs";
const FALLBACK_PATH: &str = "hafs/uthmanic_hafs/UthmanicHafs1Ver18.woff2";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FontStatus {
    Loading,
    Loaded,
    Failed,
}

/// Font load state shared by every view.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FontRegistry {
    /// Base URLs the current entries were (or are being) loaded from.
    bases: Vec<String>,
    /// Keyed by `(MushafFont::mushaf_id(), page)`.
    pages: HashMap<(u32, u16), FontStatus>,
    fallback: Option<FontStatus>,
}

impl FontRegistry {
    pub fn status(&self, font: MushafFont, page: u16) -> Option<FontStatus> {
        self.pages.get(&(font.mushaf_id(), page)).copied()
    }

    pub fn is_loaded(&self, font: MushafFont, page: u16) -> bool {
        self.status(font, page) == Some(FontStatus::Loaded)
    }

    /// Pages among `pages` whose font failed, sorted.
    pub fn failed_pages(&self, font: MushafFont, pages: &[u16]) -> Vec<u16> {
        let mut out: Vec<u16> =
            pages.iter().copied().filter(|p| self.status(font, *p) == Some(FontStatus::Failed)).collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Loaded pages of `font`, sorted.
    pub fn loaded_pages(&self, font: MushafFont) -> Vec<u16> {
        let mut out: Vec<u16> = self
            .pages
            .iter()
            .filter(|((f, _), s)| *f == font.mushaf_id() && **s == FontStatus::Loaded)
            .map(|((_, p), _)| *p)
            .collect();
        out.sort_unstable();
        out
    }

    /// New base URLs: failed loads get another chance with them.
    fn sync_bases(&mut self, bases: &[String]) {
        if self.bases != bases {
            self.bases = bases.to_vec();
            self.forget_failed();
        }
    }

    fn forget_failed(&mut self) {
        self.pages.retain(|_, s| *s != FontStatus::Failed);
        if self.fallback == Some(FontStatus::Failed) {
            self.fallback = None;
        }
    }

    /// Pages that still need a load (never requested for this font), marking them `Loading`.
    fn claim(&mut self, font: MushafFont, pages: &[u16]) -> Vec<u16> {
        let mut out = Vec::new();
        for &p in pages {
            if let std::collections::hash_map::Entry::Vacant(e) = self.pages.entry((font.mushaf_id(), p)) {
                e.insert(FontStatus::Loading);
                out.push(p);
            }
        }
        out
    }

    fn claim_fallback(&mut self) -> bool {
        if self.fallback.is_none() {
            self.fallback = Some(FontStatus::Loading);
            true
        } else {
            false
        }
    }
}

pub(crate) static FONTS: GlobalSignal<FontRegistry> = Signal::global(FontRegistry::default);
/// Bumped by "Retry" to re-request failed fonts.
static FONT_RETRY: GlobalSignal<u32> = Signal::global(|| 0);

/// Forget failed loads and try again.
pub(crate) fn retry_failed() {
    FONTS.write().forget_failed();
    *FONT_RETRY.write() += 1;
}

/// Full URLs for a font file, one per base.
pub fn font_urls(bases: &[String], path: &str) -> Vec<String> {
    bases.iter().map(|b| format!("{}/{}", b.trim_end_matches('/'), path)).collect()
}

/// CSS `src` descriptor for `urls` (quoted and escaped; control characters dropped).
pub fn css_src(urls: &[String]) -> String {
    urls.iter()
        .map(|u| {
            let escaped: String = u
                .chars()
                .filter(|c| !c.is_control())
                .flat_map(|c| match c {
                    '"' | '\\' => vec!['\\', c],
                    c => vec![c],
                })
                .collect();
            format!("url(\"{escaped}\") format(\"woff2\")")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Encode `s` as a JavaScript string literal (JSON is a subset of JS).
fn js_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// Shared JS: draw `probe` in `family` on a canvas; true if anything was painted
/// (or if the probe can't run at all, so a working font is never rejected).
const PROBE_JS: &str = r##"
const painted = (family, probe) => {
  if (!probe) return true;
  try {
    const c = document.createElement("canvas");
    c.width = 160; c.height = 120;
    const ctx = c.getContext("2d");
    if (!ctx) return true;
    ctx.font = `64px "${family}"`;
    ctx.textAlign = "center";
    ctx.textBaseline = "middle";
    ctx.fillStyle = "#000";
    ctx.fillText(probe, 80, 60);
    const data = ctx.getImageData(0, 0, c.width, c.height).data;
    for (let i = 3; i < data.length; i += 4) if (data[i] !== 0) return true;
    return false;
  } catch (_) { return true; }
};
"##;

/// JS that registers `family` from `src` (a CSS src list) and returns whether it
/// loaded and paints `probe`.
pub fn url_load_js(family: &str, src: &str, display: &str, probe: &str) -> String {
    format!(
        "{PROBE_JS}\nconst family = {};\ntry {{\n  const face = new FontFace(family, {}, {{ display: {} }});\n  document.fonts.add(face);\n  await face.load();\n  return painted(family, {});\n}} catch (_) {{ return false; }}",
        js_string(family),
        js_string(src),
        js_string(display),
        js_string(probe),
    )
}

/// Load one face via URLs. Errors (including "no JS runtime", as in tests) → false.
async fn load_by_url(family: String, urls: Vec<String>, display: &str, probe: String) -> bool {
    if urls.is_empty() {
        return false;
    }
    let js = url_load_js(&family, &css_src(&urls), display, &probe);
    document::eval(&js).join::<bool>().await.unwrap_or(false)
}

#[cfg(all(feature = "native-fonts", not(target_arch = "wasm32")))]
mod native {
    //! Desktop: fetch font bytes natively and pass them to the webview as base64.

    use super::{PROBE_JS, js_string};

    pub(super) async fn load(family: &str, urls: &[String], display: &str, probe: &str) -> bool {
        let client = reqwest::Client::new();
        for url in urls {
            let Ok(resp) = client.get(url).send().await else { continue };
            if !resp.status().is_success() {
                continue;
            }
            let Ok(bytes) = resp.bytes().await else { continue };
            if bytes.is_empty() {
                continue;
            }
            let js = format!(
                "{PROBE_JS}\nconst family = {};\ntry {{\n  const bin = atob({});\n  const buf = new Uint8Array(bin.length);\n  for (let i = 0; i < bin.length; i++) buf[i] = bin.charCodeAt(i);\n  const face = new FontFace(family, buf.buffer, {{ display: {} }});\n  document.fonts.add(face);\n  await face.load();\n  return painted(family, {});\n}} catch (_) {{ return false; }}",
                js_string(family),
                js_string(&super::base64(&bytes)),
                js_string(display),
                js_string(probe),
            );
            return dioxus::prelude::document::eval(&js).join::<bool>().await.unwrap_or(false);
        }
        false
    }
}

/// Standard base64 (with padding).
#[cfg_attr(not(all(feature = "native-fonts", not(target_arch = "wasm32"))), allow(dead_code))]
pub fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(T[(n >> 18) as usize & 63] as char);
        out.push(T[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { T[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { T[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Load a face: natively first on desktop, then via URLs.
async fn load_face(family: String, urls: Vec<String>, display: &'static str, probe: String) -> bool {
    #[cfg(all(feature = "native-fonts", not(target_arch = "wasm32")))]
    if native::load(&family, &urls, display, &probe).await {
        return true;
    }
    load_by_url(family, urls, display, probe).await
}

/// Make sure the QCF fonts for `pages` (`(page, sample glyph)`) and the Unicode
/// fallback face are requested for the current font choice and base URLs.
/// Re-runs when the pages, the font choice, the bases or "Retry" change.
pub(crate) fn use_mushaf_fonts(pages: Memo<Vec<(u16, String)>>) {
    let settings = use_settings();
    use_effect(move || {
        let _ = FONT_RETRY.read();
        let (font, bases) = {
            let s = settings.read();
            (s.mushaf_font, s.font_bases())
        };
        let pages = pages.read().clone();
        let (todo, fallback) = {
            let mut reg = FONTS.write();
            reg.sync_bases(&bases);
            let wanted: Vec<u16> = pages.iter().map(|(p, _)| *p).collect();
            (reg.claim(font, &wanted), reg.claim_fallback())
        };
        if fallback {
            let urls = font_urls(&bases, FALLBACK_PATH);
            dioxus::dioxus_core::spawn_forever(async move {
                let ok = load_face(FALLBACK_FACE.to_string(), urls, "swap", String::new()).await;
                FONTS.write().fallback = Some(if ok { FontStatus::Loaded } else { FontStatus::Failed });
            });
        }
        for page in todo {
            let probe = pages.iter().find(|(p, _)| *p == page).map(|(_, g)| g.clone()).unwrap_or_default();
            let urls = font_urls(&bases, &font.font_path(page));
            let family = font.family(page);
            dioxus::dioxus_core::spawn_forever(async move {
                let ok = load_face(family, urls, "block", probe).await;
                let status = if ok { FontStatus::Loaded } else { FontStatus::Failed };
                if !ok {
                    dioxus::logger::tracing::warn!("mushaf font for page {page} could not be loaded");
                }
                FONTS.write().pages.insert((font.mushaf_id(), page), status);
            });
        }
    });
}

/// Whether `page`'s QCF font (current font choice) is loaded. Reactive per page.
pub(crate) fn use_page_font_ready(page: Option<u16>) -> bool {
    let settings = use_settings();
    let ready = use_memo(use_reactive!(|page| {
        let font = settings.read().mushaf_font;
        page.is_some_and(|p| FONTS.read().is_loaded(font, p))
    }));
    ready()
}

/// `@font-palette-values` so the tajweed (COLRv1) fonts use their dark palette
/// in dark mode (palette 1 is dark, as on quran.com). Family names are generated
/// from page numbers, so the CSS is safe to inline.
pub fn palette_css(font: MushafFont, loaded: &[u16]) -> String {
    if font != MushafFont::TajweedV4 || loaded.is_empty() {
        return String::new();
    }
    let families: Vec<String> = loaded.iter().map(|p| format!("\"{}\"", font.family(*p))).collect();
    format!(
        "@font-palette-values --qcf-dark {{ font-family: {}; base-palette: 1; }}\n\
         @media (prefers-color-scheme: dark) {{ .glyph-text {{ font-palette: --qcf-dark; }} }}",
        families.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_claims_each_page_once_per_font() {
        let mut r = FontRegistry::default();
        let bases = vec!["https://a".to_string()];
        r.sync_bases(&bases);
        assert_eq!(r.claim(MushafFont::TajweedV4, &[1, 2, 2]), vec![1, 2]);
        assert_eq!(r.claim(MushafFont::TajweedV4, &[1, 2, 3]), vec![3]);
        // Another font choice loads its own files.
        assert_eq!(r.claim(MushafFont::MadaniV2, &[1]), vec![1]);
        assert_eq!(r.status(MushafFont::TajweedV4, 1), Some(FontStatus::Loading));
        let v4 = MushafFont::TajweedV4.mushaf_id();
        r.pages.insert((v4, 1), FontStatus::Loaded);
        r.pages.insert((v4, 2), FontStatus::Failed);
        assert!(r.is_loaded(MushafFont::TajweedV4, 1) && !r.is_loaded(MushafFont::MadaniV2, 1));
        assert_eq!(r.failed_pages(MushafFont::TajweedV4, &[2, 1, 3]), vec![2]);
        assert_eq!(r.loaded_pages(MushafFont::TajweedV4), vec![1]);
        // Same bases: failed pages are not retried automatically.
        r.sync_bases(&bases);
        assert!(r.claim(MushafFont::TajweedV4, &[2]).is_empty());
        // New bases: failed ones are retried, loaded ones kept.
        r.sync_bases(&["https://b".to_string()]);
        assert_eq!(r.claim(MushafFont::TajweedV4, &[1, 2]), vec![2]);
        assert!(r.claim_fallback());
        assert!(!r.claim_fallback());
        r.fallback = Some(FontStatus::Failed);
        r.forget_failed();
        assert!(r.claim_fallback());
    }

    #[test]
    fn urls_and_css_src() {
        let bases = vec!["https://x.test/fonts/quran/".to_string(), "https://y.test/q".to_string()];
        let urls = font_urls(&bases, "hafs/v4/colrv1/woff2/p3.woff2");
        assert_eq!(urls[0], "https://x.test/fonts/quran/hafs/v4/colrv1/woff2/p3.woff2");
        assert_eq!(urls[1], "https://y.test/q/hafs/v4/colrv1/woff2/p3.woff2");
        assert_eq!(
            css_src(&urls[..1]),
            "url(\"https://x.test/fonts/quran/hafs/v4/colrv1/woff2/p3.woff2\") format(\"woff2\")"
        );
        // Hostile base URL can't break out of the CSS string or the JS literal.
        let src = css_src(&["a\")\nb\\".to_string()]);
        assert_eq!(src, "url(\"a\\\")b\\\\\") format(\"woff2\")");
        let js = url_load_js("p1-v4", &src, "block", "\u{fb51}");
        assert!(js.contains(&serde_json::to_string(&src).unwrap()));
        assert!(js.contains("await face.load()"));
    }

    #[test]
    fn base64_encoding() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xff, 0xfe, 0x00]), "//4A");
    }

    #[test]
    fn palettes_only_for_tajweed() {
        assert_eq!(palette_css(MushafFont::MadaniV2, &[1]), "");
        assert_eq!(palette_css(MushafFont::TajweedV4, &[]), "");
        let css = palette_css(MushafFont::TajweedV4, &[1, 22]);
        assert!(css.contains("font-family: \"p1-v4\", \"p22-v4\"; base-palette: 1;"));
    }
}
