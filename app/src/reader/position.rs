//! The reading position: the word that starts the line at the top of the window.
//!
//! One script follows it while the reader scrolls and reports it here, where the
//! header's mode toggle uses it to switch between surah, juz and page without
//! losing the place. In the browser the script also writes it to the URL hash,
//! so a reload comes back to the same line; the desktop app, which has no URL
//! bar, saves the whole location to disk instead (see `layout`).

use dioxus::prelude::*;

use crate::route::WordRef;
use crate::util;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub at: WordRef,
    /// Mushaf page of that word (0 if unknown).
    pub page: u16,
    /// The view is scrolled to its very top.
    pub top: bool,
}

/// Where the reader is in the view on screen; `None` when no verses are shown.
pub static POSITION: GlobalSignal<Option<Position>> = Signal::global(|| None);

/// The last place read, kept after leaving the view, so a list can lead back to it.
pub static LAST_READ: GlobalSignal<Option<Position>> = Signal::global(|| None);

/// `window.__readerPos`, installed once.
///
/// - `report()` finds the first line whose middle is below the header and sends
///   `[location, page, atTop]` (or `null` without verses).
/// - `restore(location)` scrolls that word's line under the header and keeps it
///   there until the page has settled (fonts, fitting), unless the reader takes over. A
///   surah's first line brings its banner along, and the first line of a view
///   means the top of the view.
/// - While `held` (a view opened at a position that isn't restored yet) nothing
///   is reported, so the position to restore isn't overwritten.
const INSTALL_JS: &str = r##"
if (!window.__readerPos) {
  const WEB = __WEB__;
  const edge = () => (document.querySelector(".site-header")?.getBoundingClientRect().bottom ?? 0) + 4;
  const lines = (root) => root.querySelectorAll(".mushaf-line, .mushaf-flow");
  const lineOf = (el) => el.closest(".mushaf-line, .mushaf-flow") ?? el;
  const firstWord = (root) => root.querySelector(".word[data-location]");
  const current = () => {
    const y = edge();
    for (const page of document.querySelectorAll(".mushaf-page")) {
      if (page.getBoundingClientRect().bottom <= y) continue;
      for (const line of lines(page)) {
        const r = line.getBoundingClientRect();
        const word = r.top + r.height / 2 > y && firstWord(line);
        if (word) return word;
      }
    }
    return null;
  };
  const pos = { held: false, send: null };
  let timer = 0;
  pos.report = () => {
    if (pos.held) return;
    const word = current();
    const atTop = scrollY < 8;
    pos.send?.(word ? [word.dataset.location, Number(word.dataset.page) || 0, atTop] : null);
    if (!WEB || !word) return;
    const hash = atTop ? "" : "#" + word.dataset.location.replace(/:1$/, "");
    if (hash !== location.hash) history.replaceState(history.state, "", location.pathname + location.search + hash);
  };
  pos.soon = () => { clearTimeout(timer); timer = setTimeout(pos.report, 200); };
  pos.release = () => { pos.held = false; pos.soon(); };
  const find = (loc) => {
    const [c, v, w] = loc.split(":");
    return document.querySelector(`.word[data-location="${c}:${v}:${w || 1}"]`) ?? document.getElementById(`verse-${c}:${v}`);
  };
  const target = (el) => {
    let line = lineOf(el);
    const first = firstWord(document);
    if (first && lineOf(first) === line) return null;
    if (/^\d+:1:1$/.test(el.dataset.location ?? "")) {
      for (let p = line.previousElementSibling; p?.matches(".surah-banner, .bismillah, .bismillah-meaning"); p = p.previousElementSibling) line = p;
    }
    return line;
  };
  pos.restore = (loc) => new Promise((done) => {
    pos.held = true;
    const inputs = ["wheel", "touchstart", "keydown", "mousedown"];
    let tries = 0, still = 0, found = false, taken = false;
    const take = () => { taken = true; };
    inputs.forEach((e) => addEventListener(e, take, { passive: true }));
    const step = () => {
      const el = taken ? null : find(loc);
      if (el) {
        found = true;
        const line = target(el);
        const off = line ? line.getBoundingClientRect().top - edge() : -scrollY;
        still = Math.abs(off) < 1 ? still + 1 : 0;
        if (still === 0) scrollBy(0, off);
      }
      // Done once the line has stayed put for about a second (pages above it are
      // still getting their fonts and final size), or after ten seconds.
      if (!taken && still < 8 && ++tries < 70) return setTimeout(step, 150);
      inputs.forEach((e) => removeEventListener(e, take));
      pos.release();
      done(found);
    };
    step();
  });
  addEventListener("scroll", pos.soon, { passive: true });
  window.__readerPos = pos;
}
"##;

fn install_js() -> String {
    INSTALL_JS.replace("__WEB__", if cfg!(target_arch = "wasm32") { "true" } else { "false" })
}

/// Follow the reading position for as long as the app runs. Call once, from the app shell.
pub fn use_position_tracking() {
    use_hook(|| {
        spawn(async move {
            let js = format!("{}\nwindow.__readerPos.send = (msg) => dioxus.send(msg);\nawait new Promise(() => {{}});", install_js());
            let mut eval = document::eval(&js);
            while let Ok(msg) = eval.recv::<Option<(String, u16, bool)>>().await {
                let position =
                    msg.and_then(|(loc, page, top)| Some(Position { at: WordRef::parse(&loc)?, page, top }));
                if *POSITION.peek() != position {
                    *POSITION.write() = position;
                    if position.is_some() {
                        *LAST_READ.write() = position;
                    }
                }
            }
        });
    });
}

fn run(call: &str) {
    document::eval(&format!("{}\nwindow.__readerPos.{call};", install_js()));
}

/// Stop reporting until `restore` or `release`: the view was opened at a position.
pub fn hold() {
    run("held = true");
}

/// Report again (nothing could be restored).
pub fn release() {
    run("release()");
}

/// The verses on screen changed: report the position shortly.
pub fn changed() {
    run("soon()");
}

/// Scroll to `at` (its verse must be loaded). False if it isn't on the page.
pub async fn restore(at: WordRef) -> bool {
    let loc = format!("{}:{}:{}", at.chapter, at.verse, at.word);
    let js = format!("{}\nreturn await window.__readerPos.restore({});", install_js(), util::js_string(&loc));
    util::eval::<bool>(&js).await.unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_is_complete() {
        let js = install_js();
        assert!(!js.contains("__WEB__") && js.contains("const WEB = false;"), "{js}");
        for api in ["pos.report =", "pos.soon =", "pos.release =", "pos.restore ="] {
            assert!(js.contains(api), "{api}");
        }
    }
}
