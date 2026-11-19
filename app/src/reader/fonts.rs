//! Loading the King Fahd QCF per-page fonts, the Unicode fallback face and the
//! surah banner fonts.
//!
//! Every face is fetched as bytes (trying each base URL in order, with a
//! timeout) and registered with `new FontFace(family, bytes)`. Fetching ourselves
//! behaves the same in Chromium, Firefox and WebKit (web and the desktop
//! webview); a `FontFace` with a URL list can hang in WebKit when the first URL
//! fails. Per-page state lives in [`FONTS`]; words switch from Unicode text to
//! glyphs only when their page is `Loaded`.
//!
//! After loading, a canvas probe checks EVERY glyph character used on the page:
//! each must paint something and must look different from the same character in
//! the system serif font (a font file that lacks a glyph would otherwise be
//! silently replaced by a system letter). Glyphs that arrive later for an
//! already-loaded page are probed too. Any failure marks the page `Failed`, so
//! its words stay readable as Unicode text.

use std::collections::HashMap;
use std::rc::Rc;

use dioxus::prelude::*;
use serde::Deserialize;

use crate::settings::{MushafFont, use_settings};
use crate::util::js_string;

/// Unicode Hafs face for text-mode words (`.qpc` in reader.css). It expects the
/// QPC text encoding (`qpc_uthmani_hafs`), not plain `text_uthmani`.
pub const FALLBACK_FACE: &str = "UthmanicHafs";
const FALLBACK_PATH: &str = "hafs/uthmanic_hafs/UthmanicHafs1Ver18.woff2";

/// The font files that draw a [`MushafFont`] choice in this browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontSet {
    /// QCF V2, single colour.
    V2,
    /// QCF V4 tajweed as COLRv1 (dark mode uses the font's dark palette).
    V4Colr,
    /// QCF V4 tajweed as OpenType-SVG with baked-in colours, for WebKit, which
    /// can't switch COLRv1 palettes.
    V4SvgLight,
    V4SvgDark,
}

impl FontSet {
    fn key(self) -> &'static str {
        match self {
            FontSet::V2 => "v2",
            FontSet::V4Colr => "v4",
            FontSet::V4SvgLight => "v4l",
            FontSet::V4SvgDark => "v4d",
        }
    }

    /// CSS font-family registered for a page, e.g. `p526-v4`.
    pub fn family(self, page: u16) -> String {
        format!("p{page}-{}", self.key())
    }

    /// Path of a page's font file below a font base URL (quran.com layout).
    pub fn path(self, page: u16) -> String {
        let dir = match self {
            FontSet::V2 => "v2",
            FontSet::V4Colr => "v4/colrv1",
            FontSet::V4SvgLight => "v4/ot-svg/light",
            FontSet::V4SvgDark => "v4/ot-svg/dark",
        };
        format!("hafs/{dir}/woff2/p{page}.woff2")
    }
}

/// What the browser can draw, and its colour scheme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Engine {
    /// COLRv1 fonts with palette switching work (Chromium, Firefox).
    pub colr: bool,
    pub dark: bool,
}

/// `None` until detected; no page font is requested before that.
pub(crate) static ENGINE: GlobalSignal<Option<Engine>> = Signal::global(|| None);

const ENGINE_JS: &str = r#"
const ua = navigator.userAgent;
const colr = !(/AppleWebKit/.test(ua) && !/Chrome|Chromium|Edg\//.test(ua));
const dark = matchMedia("(prefers-color-scheme: dark)");
dioxus.send([colr, dark.matches]);
dark.addEventListener("change", () => dioxus.send([colr, dark.matches]));
await new Promise(() => {});
"#;

/// Detects the engine once per app run and follows colour-scheme changes.
fn use_engine() {
    use_hook(|| {
        if ENGINE.peek().is_some() {
            return;
        }
        dioxus::dioxus_core::spawn_forever(async move {
            let mut eval = document::eval(ENGINE_JS);
            while let Ok((colr, dark)) = eval.recv::<(bool, bool)>().await {
                let engine = Some(Engine { colr, dark });
                if *ENGINE.peek() != engine {
                    *ENGINE.write() = engine;
                }
            }
        });
    });
}

/// Whether the page is dark here: what the theme says (`Settings::dark`), or the
/// system's scheme when it leaves that open. `None` while the engine is still
/// unknown. Reactive.
pub fn is_dark(theme: Option<bool>) -> Option<bool> {
    let engine = (*ENGINE.read())?;
    Some(theme.unwrap_or(engine.dark))
}

/// The font files for `font` here, for a theme that is dark, light or the
/// system's (`None`); `None` while the engine is still unknown. Reactive.
pub fn font_set(font: MushafFont, theme: Option<bool>) -> Option<FontSet> {
    let engine = (*ENGINE.read())?;
    Some(match font {
        MushafFont::MadaniV2 => FontSet::V2,
        MushafFont::TajweedV4 if engine.colr => FontSet::V4Colr,
        MushafFont::TajweedV4 if theme.unwrap_or(engine.dark) => FontSet::V4SvgDark,
        MushafFont::TajweedV4 => FontSet::V4SvgLight,
    })
}

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
    pages: HashMap<(FontSet, u16), FontStatus>,
    /// Glyph characters already verified for a loaded page.
    verified: HashMap<(FontSet, u16), String>,
    fallback: Option<FontStatus>,
    /// Bumped when the bases change; results of loads started earlier are dropped.
    generation: u64,
}

impl FontRegistry {
    pub fn status(&self, set: FontSet, page: u16) -> Option<FontStatus> {
        self.pages.get(&(set, page)).copied()
    }

    pub fn is_loaded(&self, set: FontSet, page: u16) -> bool {
        self.status(set, page) == Some(FontStatus::Loaded)
    }

    /// Pages among `pages` whose font failed, sorted.
    pub fn failed_pages(&self, set: FontSet, pages: &[u16]) -> Vec<u16> {
        let mut out: Vec<u16> =
            pages.iter().copied().filter(|p| self.status(set, *p) == Some(FontStatus::Failed)).collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Loaded pages of `set`, sorted.
    pub fn loaded_pages(&self, set: FontSet) -> Vec<u16> {
        let mut out: Vec<u16> = self
            .pages
            .iter()
            .filter(|((s, _), status)| *s == set && **status == FontStatus::Loaded)
            .map(|((_, p), _)| *p)
            .collect();
        out.sort_unstable();
        out
    }

    /// New base URLs: failed and in-flight loads start over with them (in-flight
    /// results from the old bases are dropped via `generation`).
    fn sync_bases(&mut self, bases: &[String]) {
        if self.bases != bases {
            self.bases = bases.to_vec();
            self.generation += 1;
            self.pages.retain(|_, s| *s == FontStatus::Loaded);
            if self.fallback != Some(FontStatus::Loaded) {
                self.fallback = None;
            }
        }
    }

    /// Characters of `glyphs` not yet verified for a loaded page (deduplicated).
    fn unverified(&self, key: (FontSet, u16), glyphs: &str) -> String {
        let done = self.verified.get(&key).map(String::as_str).unwrap_or("");
        let mut out = String::new();
        for c in glyphs.chars().filter(|c| !c.is_whitespace()) {
            if !done.contains(c) && !out.contains(c) {
                out.push(c);
            }
        }
        out
    }

    fn mark_verified(&mut self, key: (FontSet, u16), chars: &str) {
        let entry = self.verified.entry(key).or_default();
        for c in chars.chars() {
            if !entry.contains(c) {
                entry.push(c);
            }
        }
    }

    fn forget_failed(&mut self) {
        self.pages.retain(|_, s| *s != FontStatus::Failed);
        if self.fallback == Some(FontStatus::Failed) {
            self.fallback = None;
        }
    }

    /// Pages that still need a load (never requested for this set), marking them `Loading`.
    fn claim(&mut self, set: FontSet, pages: &[u16]) -> Vec<u16> {
        let mut out = Vec::new();
        for &p in pages {
            if let std::collections::hash_map::Entry::Vacant(e) = self.pages.entry((set, p)) {
                e.insert(FontStatus::Loading);
                out.push(p);
            }
        }
        out
    }

    fn claim_fallback(&mut self) -> bool {
        let fresh = self.fallback.is_none();
        if fresh {
            self.fallback = Some(FontStatus::Loading);
        }
        fresh
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

/// Shared JS. `painted(family, chars)` is true when every character of `chars`,
/// drawn in `family` (with a serif fallback), paints something and differs from
/// the same character in plain serif, i.e. the face really has the glyph.
/// `paintedWhole` does the same for one ligature string. If the canvas can't be
/// used at all the check can't run and passes (as quran.com trusts a loaded face).
const PROBE_JS: &str = r##"
const probeCtx = () => {
  if (window.__qcfProbe === undefined) {
    try {
      const c = document.createElement("canvas");
      c.width = 96; c.height = 80;
      window.__qcfProbe = c.getContext("2d", { willReadFrequently: true });
    } catch (_) { window.__qcfProbe = null; }
  }
  return window.__qcfProbe;
};
const alphaOf = (ctx, font, text) => {
  ctx.clearRect(0, 0, 96, 80);
  ctx.font = font;
  ctx.textAlign = "center";
  ctx.textBaseline = "middle";
  ctx.fillStyle = "#000";
  ctx.fillText(text, 48, 40);
  return ctx.getImageData(0, 0, 96, 80).data;
};
// `blankOk`: a glyph that draws nothing and takes no room counts as drawn. The
// tajweed fonts have such glyphs (the second glyph of الٓر at 10:1, for one); a
// character the font lacks is drawn by a fallback font instead, with ink and width.
const drawn = (ctx, family, text, px, blankOk) => {
  const a = alphaOf(ctx, `${px}px "${family}", serif`, text);
  let ink = false;
  for (let i = 3; i < a.length; i += 4) if (a[i] !== 0) { ink = true; break; }
  if (!ink) return !!blankOk && ctx.measureText(text).width === 0;
  const b = alphaOf(ctx, `${px}px serif`, text);
  for (let i = 3; i < a.length; i += 4) if (a[i] !== b[i]) return true;
  return false;
};
const painted = (family, chars) => {
  const ctx = probeCtx();
  if (!ctx || !chars) return true;
  try {
    for (const ch of chars) if (ch.trim() && !drawn(ctx, family, ch, 32, true)) return false;
    return true;
  } catch (_) { return true; }
};
const paintedWhole = (family, text) => {
  const ctx = probeCtx();
  if (!ctx || !text) return true;
  try { return drawn(ctx, family, text, 24); } catch (_) { return true; }
};
"##;

/// Seconds before a font download is given up.
const FETCH_TIMEOUT_SECS: u32 = 20;

/// JS that downloads `family` from the first of `urls` that yields a usable
/// font, registers it and checks that it paints `probe` (`whole`: as one
/// ligature string instead of per character). Returns `""` on success, else why
/// each URL failed.
pub fn load_js(family: &str, urls: &[String], probe: &str, whole: bool) -> String {
    let check = if whole { "paintedWhole" } else { "painted" };
    format!(
        r#"{PROBE_JS}
const family = {family}, probe = {probe}, failures = [];
for (const url of {urls}) {{
  let face = null;
  const abort = new AbortController();
  const timer = setTimeout(() => abort.abort(), {FETCH_TIMEOUT_SECS}000);
  try {{
    const resp = await fetch(url, {{ signal: abort.signal }});
    if (!resp.ok) throw new Error(`HTTP ${{resp.status}}`);
    face = new FontFace(family, await resp.arrayBuffer());
    await face.load();
    document.fonts.add(face);
    if ({check}(family, probe)) return "";
    throw new Error("the font does not draw its glyphs");
  }} catch (e) {{
    failures.push(`${{url}}: ${{e && e.message || e}}`);
  }} finally {{
    clearTimeout(timer);
  }}
  if (face) document.fonts.delete(face);
}}
return failures.join("; ") || "no font URL";"#,
        family = js_string(family),
        probe = js_string(probe),
        urls = serde_json::to_string(urls).unwrap_or_else(|_| "[]".into()),
    )
}

/// JS that checks more glyph characters against an already-loaded `family`.
pub fn probe_js(family: &str, chars: &str) -> String {
    format!("{PROBE_JS}\nreturn painted({}, {});", js_string(family), js_string(chars))
}

/// Load one face; failures (including "no JS runtime", as in tests) are logged.
async fn load_face(family: &str, urls: &[String], probe: &str, whole: bool) -> bool {
    let failure = crate::util::eval::<String>(&load_js(family, urls, probe, whole))
        .await
        .unwrap_or_else(|| "the script's result is unavailable".into());
    if !failure.is_empty() {
        dioxus::logger::tracing::warn!("font {family} could not be loaded: {failure}");
    }
    failure.is_empty()
}

/// Make sure the QCF fonts for `pages` (`(page, glyphs shown)`) and the Unicode
/// fallback face are requested for the font set and base URLs in use.
/// Re-runs when the pages, the font set, the bases or "Retry" change.
pub(crate) fn use_mushaf_fonts(pages: Memo<Vec<(u16, String)>>, set: Memo<Option<FontSet>>) {
    use_engine();
    let settings = use_settings();
    let bases = use_memo(move || settings.read().font_bases());
    use_effect(move || {
        let _ = FONT_RETRY.read();
        let bases = bases();
        let pages = pages.read().clone();
        let set = set();
        let (todo, fallback, recheck, generation) = {
            let mut reg = FONTS.write();
            reg.sync_bases(&bases);
            let wanted: Vec<u16> = pages.iter().map(|(p, _)| *p).collect();
            let todo = set.map(|s| reg.claim(s, &wanted)).unwrap_or_default();
            // Loaded pages that now show glyph characters not yet probed.
            let recheck: Vec<(u16, String)> = pages
                .iter()
                .filter_map(|(p, g)| {
                    let key = (set?, *p);
                    let chars = reg.unverified(key, g);
                    (reg.is_loaded(key.0, key.1) && !chars.is_empty()).then_some((*p, chars))
                })
                .collect();
            (todo, reg.claim_fallback(), recheck, reg.generation)
        };
        let current = move || FONTS.peek().generation == generation;
        if fallback {
            let urls = font_urls(&bases, FALLBACK_PATH);
            dioxus::dioxus_core::spawn_forever(async move {
                let ok = load_face(FALLBACK_FACE, &urls, "", false).await;
                if current() {
                    FONTS.write().fallback = Some(if ok { FontStatus::Loaded } else { FontStatus::Failed });
                }
            });
        }
        let Some(set) = set else { return };
        for page in todo {
            let probe = pages.iter().find(|(p, _)| *p == page).map(|(_, g)| g.clone()).unwrap_or_default();
            let urls = font_urls(&bases, &set.path(page));
            dioxus::dioxus_core::spawn_forever(async move {
                let ok = load_face(&set.family(page), &urls, &probe, false).await;
                if !current() {
                    return; // bases changed meanwhile; a fresh load was started
                }
                let mut reg = FONTS.write();
                if ok {
                    reg.mark_verified((set, page), &probe);
                }
                reg.pages.insert((set, page), if ok { FontStatus::Loaded } else { FontStatus::Failed });
            });
        }
        for (page, chars) in recheck {
            // Mark first so the same characters aren't probed twice concurrently.
            FONTS.write().mark_verified((set, page), &chars);
            let js = probe_js(&set.family(page), &chars);
            dioxus::dioxus_core::spawn_forever(async move {
                // Only a definite "no" fails the page; a lost answer leaves it loaded.
                let ok = crate::util::eval::<bool>(&js).await.unwrap_or(true);
                if !ok && current() {
                    dioxus::logger::tracing::warn!("mushaf font for page {page} lacks glyphs; showing text");
                    FONTS.write().pages.insert((set, page), FontStatus::Failed);
                }
            });
        }
    });
}

/// Whether `page`'s QCF font is loaded. Reactive per page. `set` must be the
/// font set used for the words' family, so readiness and family can't disagree.
pub(crate) fn use_page_font_ready(set: Option<FontSet>, page: Option<u16>) -> bool {
    let ready = use_memo(use_reactive!(|set, page| {
        set.zip(page).is_some_and(|(s, p)| FONTS.read().is_loaded(s, p))
    }));
    ready()
}

/// `@font-palette-values` so the COLRv1 tajweed fonts use their dark palette in
/// a dark theme (palette 1, as on quran.com). Family names are generated from page
/// numbers, so the CSS is safe to inline.
pub fn palette_css(set: FontSet, loaded: &[u16], dark: bool) -> String {
    if set != FontSet::V4Colr || loaded.is_empty() || !dark {
        return String::new();
    }
    let families: Vec<String> = loaded.iter().map(|p| format!("\"{}\"", set.family(*p))).collect();
    format!(
        "@font-palette-values --qcf-dark {{ font-family: {}; base-palette: 1; }}\n.glyph {{ font-palette: --qcf-dark; }}",
        families.join(", ")
    )
}

// ---------- Printed-mushaf surah banner (QUL fonts, loaded at runtime) ----------

/// Ornamental banner frame: ligature `header` in QUL's "quran-common" font.
pub const BANNER_FRAME_FAMILY: &str = "qul-quran-common";
/// Calligraphic surah names: ligatures `surah001`…`surah114` and `surah-icon` ("سورة").
pub const BANNER_NAME_FAMILY: &str = "qul-surah-names-v4";
const BANNER_FRAME_URL: &str = "https://static-cdn.tarteel.ai/qul/fonts/common/quran-common.ttf";
const BANNER_NAME_URL: &str = "https://static-cdn.tarteel.ai/qul/fonts/surah-names/v4/surah-name-v4.ttf";
/// Ligature text that draws the banner frame.
pub const BANNER_FRAME_TEXT: &str = "header";

/// Ligature text for "سورة <name>" (the names font draws `surah-icon` as "سورة").
pub fn banner_name_text(chapter: u16) -> String {
    format!("surah{chapter:03} surah-icon")
}

/// Font size the banner is measured and drawn at (SVG user units).
pub const BANNER_UNITS: f64 = 1000.0;

/// Ink box of a drawing at font size [`BANNER_UNITS`]: `(left, right, ascent,
/// descent)` from its text origin, as measured by the canvas.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct Ink(pub f64, pub f64, pub f64, pub f64);

impl Ink {
    pub fn width(self) -> f64 {
        self.0 + self.1
    }
    pub fn height(self) -> f64 {
        self.2 + self.3
    }
}

/// Measured banner drawings: the frame and every surah name (index = chapter - 1).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BannerMetrics {
    pub frame: Ink,
    pub names: Vec<Ink>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BannerFonts {
    Loading,
    Ready(Rc<BannerMetrics>),
    Failed,
}

pub(crate) static BANNER: GlobalSignal<Option<BannerFonts>> = Signal::global(|| None);

/// Starts loading the banner fonts (once per app run) and returns their state.
/// Until they are verified, callers draw the plain text banner.
pub(crate) fn use_banner_fonts() -> Option<BannerFonts> {
    use_hook(|| {
        if BANNER.peek().is_some() {
            return;
        }
        *BANNER.write() = Some(BannerFonts::Loading);
        dioxus::dioxus_core::spawn_forever(async move {
            let frame = load_face(BANNER_FRAME_FAMILY, &[BANNER_FRAME_URL.into()], BANNER_FRAME_TEXT, true).await;
            let name = frame
                && load_face(BANNER_NAME_FAMILY, &[BANNER_NAME_URL.into()], &banner_name_text(1), true).await;
            let metrics = if name { measure_banner().await } else { None };
            // Only settle a load that is still pending (tests set the state directly).
            if *BANNER.peek() == Some(BannerFonts::Loading) {
                *BANNER.write() = Some(metrics.map_or(BannerFonts::Failed, |m| BannerFonts::Ready(Rc::new(m))));
            }
        });
    });
    BANNER.read().clone()
}

async fn measure_banner() -> Option<BannerMetrics> {
    let js = format!(
        r#"const c = document.createElement("canvas").getContext("2d");
if (!c) return null;
const ink = (family, text) => {{
  c.font = `{BANNER_UNITS}px "${{family}}"`;
  const m = c.measureText(text);
  return [m.actualBoundingBoxLeft, m.actualBoundingBoxRight, m.actualBoundingBoxAscent, m.actualBoundingBoxDescent];
}};
const frame = ink({frame_family}, {frame_text});
const names = [];
for (let n = 1; n <= 114; n++) names.push(ink({name_family}, `surah${{String(n).padStart(3, "0")}} surah-icon`));
const ok = (b) => b.every(Number.isFinite) && b[0] + b[1] > 100 && b[2] + b[3] > 100;
return ok(frame) && names.every(ok) ? {{ frame, names }} : null;"#,
        frame_family = js_string(BANNER_FRAME_FAMILY),
        frame_text = js_string(BANNER_FRAME_TEXT),
        name_family = js_string(BANNER_NAME_FAMILY),
    );
    crate::util::eval::<Option<BannerMetrics>>(&js).await.flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_claims_each_page_once_per_font_set() {
        let mut r = FontRegistry::default();
        let bases = vec!["https://a".to_string()];
        r.sync_bases(&bases);
        assert_eq!(r.claim(FontSet::V4Colr, &[1, 2, 2]), vec![1, 2]);
        assert_eq!(r.claim(FontSet::V4Colr, &[1, 2, 3]), vec![3]);
        // Another font set loads its own files.
        assert_eq!(r.claim(FontSet::V2, &[1]), vec![1]);
        assert_eq!(r.claim(FontSet::V4SvgDark, &[1]), vec![1]);
        assert_eq!(r.status(FontSet::V4Colr, 1), Some(FontStatus::Loading));
        r.pages.insert((FontSet::V4Colr, 1), FontStatus::Loaded);
        r.pages.insert((FontSet::V4Colr, 2), FontStatus::Failed);
        assert!(r.is_loaded(FontSet::V4Colr, 1) && !r.is_loaded(FontSet::V2, 1));
        assert_eq!(r.failed_pages(FontSet::V4Colr, &[2, 1, 3]), vec![2]);
        assert_eq!(r.loaded_pages(FontSet::V4Colr), vec![1]);
        // Same bases: failed pages are not retried automatically.
        r.sync_bases(&bases);
        assert!(r.claim(FontSet::V4Colr, &[2]).is_empty());
        // New bases: failed ones are retried, loaded ones kept.
        r.sync_bases(&["https://b".to_string()]);
        assert_eq!(r.claim(FontSet::V4Colr, &[1, 2]), vec![2]);
        assert!(r.claim_fallback());
        assert!(!r.claim_fallback());
        r.fallback = Some(FontStatus::Failed);
        r.forget_failed();
        assert!(r.claim_fallback());
    }

    #[test]
    fn font_set_follows_engine_and_scheme() {
        let dom = VirtualDom::new(|| rsx! {});
        dom.in_runtime(|| {
            assert_eq!(font_set(MushafFont::TajweedV4, None), None, "nothing is chosen before detection");
            assert_eq!(is_dark(Some(true)), None);
            let pick = |colr, dark, font, theme| {
                *ENGINE.write() = Some(Engine { colr, dark });
                font_set(font, theme)
            };
            assert_eq!(pick(true, true, MushafFont::TajweedV4, None), Some(FontSet::V4Colr));
            assert_eq!(pick(false, false, MushafFont::TajweedV4, None), Some(FontSet::V4SvgLight));
            assert_eq!(pick(false, true, MushafFont::TajweedV4, None), Some(FontSet::V4SvgDark));
            assert_eq!(pick(false, true, MushafFont::MadaniV2, None), Some(FontSet::V2));
            // A chosen theme wins over the system's scheme.
            assert_eq!(pick(false, false, MushafFont::TajweedV4, Some(true)), Some(FontSet::V4SvgDark));
            assert_eq!(pick(false, true, MushafFont::TajweedV4, Some(false)), Some(FontSet::V4SvgLight));
            assert_eq!((is_dark(None), is_dark(Some(false))), (Some(true), Some(false)));
        });
    }

    #[test]
    fn families_and_paths() {
        assert_eq!(FontSet::V4Colr.family(526), "p526-v4");
        assert_eq!(FontSet::V2.path(3), "hafs/v2/woff2/p3.woff2");
        assert_eq!(FontSet::V4Colr.path(3), "hafs/v4/colrv1/woff2/p3.woff2");
        assert_eq!(FontSet::V4SvgDark.path(3), "hafs/v4/ot-svg/dark/woff2/p3.woff2");
        assert_ne!(FontSet::V4SvgLight.family(3), FontSet::V4SvgDark.family(3));
        let bases = vec!["https://x.test/fonts/quran/".to_string(), "https://y.test/q".to_string()];
        assert_eq!(
            font_urls(&bases, &FontSet::V4Colr.path(3)),
            ["https://x.test/fonts/quran/hafs/v4/colrv1/woff2/p3.woff2", "https://y.test/q/hafs/v4/colrv1/woff2/p3.woff2"]
        );
    }

    #[test]
    fn load_js_escapes_values_and_removes_failed_faces() {
        let js = load_js("p1-v4", &["https://x.test/a\"b.woff2".to_string()], "\u{FB51}\"", false);
        assert!(js.contains("const family = \"p1-v4\", probe = \"\u{FB51}\\\"\", failures = [];"), "{js}");
        assert!(js.contains(r#"["https://x.test/a\"b.woff2"]"#), "{js}");
        assert!(js.contains("document.fonts.delete(face)") && js.contains("if (painted(family, probe))"));
        assert!(js.contains(r#"if (painted(family, probe)) return "";"#) && js.contains("failures.join"), "{js}");
        assert!(load_js("qul", &[], "header", true).contains("if (paintedWhole(family, probe))"));
        assert!(probe_js("p2-v4", "a\"b").contains(r#"painted("p2-v4", "a\"b")"#));
    }

    #[test]
    fn palettes_only_for_colr_tajweed() {
        assert_eq!(palette_css(FontSet::V2, &[1], true), "");
        assert_eq!(palette_css(FontSet::V4SvgDark, &[1], true), "");
        assert_eq!(palette_css(FontSet::V4Colr, &[], true), "");
        assert_eq!(palette_css(FontSet::V4Colr, &[1, 22], false), "", "light themes keep the default palette");
        let css = palette_css(FontSet::V4Colr, &[1, 22], true);
        assert!(css.ends_with(".glyph { font-palette: --qcf-dark; }"), "{css}");
        assert!(css.contains("font-family: \"p1-v4\", \"p22-v4\"; base-palette: 1;"));
    }

    #[test]
    fn new_glyphs_on_loaded_pages_are_rechecked() {
        let mut r = FontRegistry::default();
        let key = (FontSet::V4Colr, 7);
        r.mark_verified(key, "ab");
        assert_eq!(r.unverified(key, "a b c a"), "c");
        assert_eq!(r.unverified((key.0, 8), "xx y"), "xy");
        r.mark_verified(key, "c");
        assert_eq!(r.unverified(key, "abc"), "");
    }

    #[test]
    fn changing_bases_restarts_failed_and_in_flight_loads() {
        let mut r = FontRegistry::default();
        r.sync_bases(&["https://a".to_string()]);
        let set = FontSet::V4Colr;
        assert_eq!(r.claim(set, &[1, 2, 3]), vec![1, 2, 3]);
        r.pages.insert((set, 1), FontStatus::Loaded);
        r.pages.insert((set, 2), FontStatus::Failed);
        let generation = r.generation;
        r.sync_bases(&["https://b".to_string()]);
        assert_ne!(r.generation, generation, "old in-flight results must be dropped");
        // Page 1 stays loaded; failed page 2 and in-flight page 3 are claimed again.
        assert_eq!(r.claim(set, &[1, 2, 3]), vec![2, 3]);
        let generation = r.generation;
        r.sync_bases(&["https://b".to_string()]);
        assert_eq!(r.generation, generation);
    }

    #[test]
    fn banner_ligatures_and_metrics() {
        assert_eq!(banner_name_text(1), "surah001 surah-icon");
        assert_eq!(banner_name_text(114), "surah114 surah-icon");
        let m: BannerMetrics = serde_json::from_str(r#"{"frame":[0,8047,813,188],"names":[[0,2628,688,78]]}"#).unwrap();
        assert_eq!((m.frame.width(), m.frame.height()), (8047.0, 1001.0));
        assert_eq!(m.names[0], Ink(0.0, 2628.0, 688.0, 78.0));
    }
}
