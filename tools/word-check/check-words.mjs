#!/usr/bin/env node
// End-to-end "no missing words" check for the web build.
//
// For each view (surahs, verse ranges, juz, mushaf pages) it fetches the expected
// words straight from the Quran.com API (independent of the app's own code), opens
// the view in a real browser, loads every verse, and compares verse by verse:
//   - every verse key is on the page, none extra, none duplicated
//   - every word of every verse is rendered, in order, at its `c:v:position`
//   - each word shows the right text: the QCF glyph (`code_v2`) when its page font
//     loaded, otherwise the Unicode fallback (its token of the verse's
//     `qpc_uthmani_hafs`, or `text_uthmani`)
//   - each word sits on the API's mushaf page and line (15-line layout); no line is
//     wrapped or wider than its page; a page view shows exactly that page's lines,
//     each with as many words as printed (verses listed across a page turn included)
//   - glyph words use their own page's QCF font, and that font really contains
//     the glyph (canvas check: inked, and different from a system-font letter)
//   - exactly one ayah marker per verse, after its last word, with the right glyph
//   - each surah banner (and Bismillah, except surahs 1 and 9) sits directly
//     before that surah's first word
//   - no `⟨?⟩` placeholders; words shown as Unicode fallback are reported
//     separately (fonts not loading is a problem, but no word is lost)
//
// Usage (with the app running via `cd app && dx serve --platform web`):
//   cd tools/word-check && npm install && npx playwright install chromium
//   node check-words.mjs                          # surahs 1-114 + sample ranges + juz 1-30
//   node check-words.mjs --views chapters --chapters 1-10,18
//   node check-words.mjs --views pages --pages 1-604
//   node check-words.mjs --font plain --wbw tooltip      # other render modes
//   node check-words.mjs --base http://127.0.0.1:8080 --api https://api.quran.com/api/v4
//   node check-words.mjs --views pages --browser /usr/bin/chromium --base https://example.org/quran-dx
// Options: --views chapters,ranges,juz,pages  --chapters SPEC  --juz SPEC  --pages SPEC  --browser PATH
//          --ranges "2/45-55,2/255"  --font tajweed|plain  --wbw inline|tooltip
//          --allow-fallback   (don't count Unicode-fallback words as problems)
// Exit code 0 = everything matched, 1 = problems found (see report.json).

import { chromium } from "playwright";
import { writeFileSync } from "node:fs";

const args = parseArgs(process.argv.slice(2));
const BASE = (args.base ?? "http://localhost:8080").replace(/\/$/, "");
const API = (args.api ?? "https://api.quran.com/api/v4").replace(/\/$/, "");
const VIEWS = String(args.views ?? "chapters,ranges,juz").split(",");
const CHAPTERS = parseSpec(args.chapters ?? "1-114", 114);
const JUZ = parseSpec(args.juz ?? "1-30", 30);
const PAGES = parseSpec(args.pages ?? "1-604", 604);
// Ranges chosen to hit the tricky paths: API page boundaries (50 verses/page),
// ranges spanning several API pages, single verses, surahs without Bismillah.
const RANGES = String(args.ranges ?? "1/1-7,2/45-55,2/49-51,2/100-151,2/255,2/286,9/1-5,18/60-82,114/1-6")
  .split(",").map((r) => r.trim()).filter(Boolean);
const FONT = String(args.font ?? "tajweed").toLowerCase();
const MUSHAF_ID = FONT === "plain" ? 1 : 19; // QCF V2 : QCF V4 tajweed (as the app requests)
const WBW = String(args.wbw ?? "inline").toLowerCase();
const ALLOW_FALLBACK = Boolean(args["allow-fallback"]);
const LOAD_TIMEOUT_MS = Number(args.timeout ?? 20000);

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i++) {
    if (argv[i].startsWith("--")) out[argv[i].slice(2)] = argv[i + 1]?.startsWith("--") ? true : argv[++i];
  }
  return out;
}

function parseSpec(spec, max) {
  const set = new Set();
  for (const part of String(spec).split(",")) {
    const [a, b] = part.split("-").map(Number);
    for (let n = a; n <= (b || a); n++) if (n >= 1 && n <= max) set.add(n);
  }
  return [...set].sort((x, y) => x - y);
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function apiGet(path, params = {}) {
  const url = `${API}${path}?${new URLSearchParams(params)}`;
  for (let attempt = 1; ; attempt++) {
    const res = await fetch(url, { headers: { Accept: "application/json" } });
    if (res.ok) return res.json();
    if (attempt >= 3) throw new Error(`${res.status} for ${url}`);
    await sleep(500 * attempt);
  }
}

/** verse_key -> the end marker's glyph (filled by expectedWords). */
const ENDS = new Map();

/**
 * verse_key -> expected non-end words for an API listing:
 * [{ location, glyph, text, page, line }]
 */
async function expectedWords(apiPath, keep = () => true, countLines = false) {
  const out = new Map();
  for (let page = 1; ; page++) {
    const data = await apiGet(apiPath, {
      words: "true", per_page: "50", page: String(page), language: "en", mushaf: String(MUSHAF_ID),
      fields: "qpc_uthmani_hafs",
      word_fields: "location,code_v2,v2_page,page_number,line_number,text_uthmani",
    });
    if (!data.pagination) throw new Error(`no pagination in ${apiPath} page ${page}`);
    for (const v of data.verses) {
      if (!keep(v)) continue;
      const end = v.words.find((w) => w.char_type_name === "end");
      ENDS.set(v.verse_key, norm(end?.code_v2 ?? ""));
      // Where the marker sits: its own line, or its verse's last word's when the API
      // places it earlier (84:21), which is the app's rule too.
      const lastWord = v.words.filter((w) => w.char_type_name !== "end").at(-1);
      const before = (a, b) => a.page_number < b.page_number || (a.page_number === b.page_number && a.line_number < b.line_number);
      const endAt = end && lastWord && before(end, lastWord) ? lastWord : end;
      if (countLines) {
        for (const w of v.words) {
          const at = w.char_type_name === "end" ? endAt : w;
          const slot = `${at.page_number}:${at.line_number}`;
          LINES.set(`${apiPath}|${slot}|${v.verse_key}:${w.position}`, slot);
        }
      }
      // The API has QPC Hafs text per verse only: one whitespace-separated token per
      // word (the last one is the ayah number), when the counts line up.
      const tokens = (v.qpc_uthmani_hafs ?? "").split(/\s+/).filter(Boolean);
      const qpc = tokens.length === v.words.length ? tokens : null;
      out.set(v.verse_key, v.words.map((w, i) => ({ w, i })).filter(({ w }) => w.char_type_name !== "end").map(({ w, i }) => ({
        location: w.location ?? `${v.verse_key}:${w.position}`,
        glyph: norm(w.code_v2 ?? ""),
        text: norm(qpc?.[i] ?? w.text_uthmani ?? ""),
        alt: norm(w.text_uthmani ?? ""),
        page: w.page_number ?? null,
        line: w.line_number ?? null,
      })));
      out.get(v.verse_key).endPage = endAt?.page_number ?? null;
    }
    if (!data.pagination.next_page) break;
    await sleep(100);
  }
  return out;
}

const norm = (s) => s.normalize("NFC").replace(/\s+/g, " ").trim();

/**
 * What mushaf page `p` holds: for every verse with a word on it, the words printed
 * on that page (a verse may run over a page turn). The API lists a few verses under
 * the neighbouring page, so the listings of p-1, p and p+1 are read.
 * Each entry also carries `total` (the verse's full word count), `marker` (its ayah
 * marker is on this page) and `starts` (its first word is on this page).
 */
async function expectedOnPage(p) {
  const out = new Map();
  for (const listing of [p - 1, p, p + 1].filter((n) => n >= 1 && n <= 604)) {
    const all = await expectedWords(`/verses/by_page/${listing}`, () => true, true);
    for (const [key, words] of all) {
      const here = words.filter((w) => w.page === p);
      const marker = words.endPage === p;
      if (!here.length && !marker) continue;
      here.total = words.length;
      here.marker = marker;
      here.starts = words[0]?.page === p;
      out.set(key, here);
    }
  }
  // Mushaf order (listings were read in order, but be explicit).
  const order = (k) => k.split(":").map(Number);
  return new Map([...out].sort(([a], [b]) => order(a)[0] - order(b)[0] || order(a)[1] - order(b)[1]));
}

/** Words (markers included) per line of the mushaf page, from what `expectedOnPage` read. */
const LINES = new Map();

/** Load every verse of the surah in the page (clicking "Load more" as needed). */
async function loadAll(page, expectedCount) {
  const count = () => page.locator("[data-word-count]").count();
  let last = -1;
  let stalledSince = Date.now();
  while (true) {
    const n = await count();
    if (n >= expectedCount) return n;
    if (n !== last) { last = n; stalledSince = Date.now(); }
    if (Date.now() - stalledSince > LOAD_TIMEOUT_MS) return n;
    const more = page.locator(".load-more-wrap .load-more");
    if (await more.count()) {
      await more.first().scrollIntoViewIfNeeded().catch(() => {});
      await more.first().click({ timeout: 2000 }).catch(() => {});
    } else {
      await page.mouse.wheel(0, 4000);
    }
    await sleep(400);
  }
}

/** What the page actually shows: verse anchors, words, markers, headers. */
async function rendered(page) {
  return page.evaluate(async () => {
    // Same independent coverage test the app uses: the glyph must ink, and must
    // differ from the same character in plain serif (i.e. no substitution).
    const canvas = document.createElement("canvas");
    canvas.width = 160; canvas.height = 120;
    const ctx = canvas.getContext("2d", { willReadFrequently: true });
    const draw = (font, ch) => {
      ctx.clearRect(0, 0, 160, 120);
      ctx.font = font; ctx.textAlign = "center"; ctx.textBaseline = "middle"; ctx.fillStyle = "#000";
      ctx.fillText(ch, 80, 60);
      return ctx.getImageData(0, 0, 160, 120).data;
    };
    const coverCache = new Map();
    const covers = (family, text) => {
      for (const ch of text) {
        if (!ch.trim()) continue;
        const k = family + "|" + ch;
        if (!coverCache.has(k)) {
          const a = draw(`48px "${family}", serif`, ch);
          let ink = false;
          for (let i = 3; i < a.length; i += 4) if (a[i] !== 0) { ink = true; break; }
          let same = true;
          if (ink) { const b = draw("48px serif", ch); for (let i = 0; i < a.length; i++) if (a[i] !== b[i]) { same = false; break; } }
          // The tajweed fonts have a few glyphs that draw nothing and take no room
          // (after الٓر at 10:1, for one); a missing glyph would get a fallback's box.
          const blank = !ink && ctx.measureText(ch).width === 0;
          coverCache.set(k, (ink && !same) || blank);
        }
        if (!coverCache.get(k)) return false;
      }
      return true;
    };
    // Same "cut off" rule as the app's audit: the whole word (with meanings) must be
    // inside the window and inside every ancestor that clips.
    const cutOff = (node) => {
      const r = node.getBoundingClientRect();
      const vw = document.documentElement.clientWidth;
      if (r.left < -1 || r.right > vw + 1) return "outside the window";
      for (let n = node.parentElement; n && n !== document.documentElement; n = n.parentElement) {
        const cs = getComputedStyle(n);
        const clips = /hidden|clip/.test(cs.overflowX + " " + cs.overflowY)
          || /paint|strict|content/.test(cs.contain || "") || cs.contentVisibility === "auto" || cs.contentVisibility === "hidden";
        if (!clips) continue;
        const a = n.getBoundingClientRect();
        if (r.left < a.left - 1 || r.right > a.right + 1 || r.top < a.top - 1 || r.bottom > a.bottom + 1) return "partly outside its page";
      }
      return null;
    };
    const firstFamily = (el) => getComputedStyle(el).fontFamily.split(",")[0].trim().replace(/^["']|["']$/g, "");

    // A word's (or marker's) Arabic text is its own text; meanings are child elements.
    const own = (el) => [...el.childNodes].filter((n) => n.nodeType === Node.TEXT_NODE).map((n) => n.data).join("");
    // A verse's first word carries its anchor: `id="verse-c:v"` and the verse's word count.
    const anchors = [...document.querySelectorAll("[data-word-count]")].map((a) => ({
      key: a.dataset.location.split(":").slice(0, 2).join(":"),
      declared: Number(a.dataset.wordCount),
    }));
    // Document order of everything we check positions of.
    const ordered = [...document.querySelectorAll(".word[data-location], .ayah-marker[data-verse-key], .surah-banner[data-chapter], .bismillah")];
    const index = new Map(ordered.map((el, i) => [el, i]));
    const words = [...document.querySelectorAll(".word[data-location]")].map((w) => {
      const text = own(w);
      const family = w.dataset.mode === "glyph" ? firstFamily(w) : null;
      return {
        location: w.dataset.location,
        mode: w.dataset.mode,
        page: w.dataset.page ? Number(w.dataset.page) : null,
        line: w.dataset.line ? Number(w.dataset.line) : null,
        text,
        family,
        covered: family ? covers(family, text.trim()) : null,
        cut: cutOff(w),
        order: index.get(w),
      };
    });
    const markers = [...document.querySelectorAll(".ayah-marker[data-verse-key]")].map((m) => {
      const glyph = m.dataset.mode === "glyph";
      const text = own(m);
      const family = glyph ? firstFamily(m) : null;
      return {
        key: m.dataset.verseKey,
        mode: m.dataset.mode,
        page: m.dataset.page ? Number(m.dataset.page) : null,
        text: glyph ? text : null,
        family,
        covered: glyph ? covers(family, text.trim()) : null,
        order: index.get(m),
      };
    });
    const headers = [...document.querySelectorAll(".surah-banner[data-chapter]")].map((h) => {
      // The next checked element after the banner, skipping a Bismillah line.
      let i = index.get(h) + 1;
      const bismillah = ordered[i]?.classList.contains("bismillah") ?? false;
      if (bismillah) i++;
      const next = ordered[i];
      return { chapter: Number(h.dataset.chapter), bismillah, next: next?.dataset?.location ?? null };
    });
    // Lines: how many words each holds, whether it stays on one row, and whether it
    // fits its page (the app never wraps or cuts a mushaf line).
    const lines = [...document.querySelectorAll(".mushaf-line")].map((l) => {
      const kids = [...l.children];
      const tops = kids.map((k) => Math.round(k.getBoundingClientRect().top));
      const box = l.getBoundingClientRect();
      const page = l.closest(".mushaf-page")?.getBoundingClientRect();
      const gap = parseFloat(getComputedStyle(l).columnGap) || 0;
      const natural = kids.reduce((w, k) => w + k.getBoundingClientRect().width, 0) + gap * Math.max(0, kids.length - 1);
      return {
        page: Number(kids[0]?.dataset.page ?? 0),
        line: Number(l.dataset.line),
        words: kids.length,
        wrapped: Math.max(...tops) - Math.min(...tops) > box.height / 2,
        overflow: page ? Math.max(0, Math.round(natural - page.width)) : 0,
      };
    });
    const missingPlaceholders = document.querySelectorAll(".word.missing").length;
    // The app's own on-screen audit (render_audit.rs) marks failed words.
    const auditFailures = [...document.querySelectorAll(".word-render-error")].map((b) =>
      `${b.closest("[data-location]")?.dataset.location ?? "?"}: ${b.getAttribute("aria-label") ?? ""}`);
    return { anchors, words, markers, headers, lines, missingPlaceholders, auditFailures };
  });
}

async function checkView(page, view) {
  const problems = [];
  let fallbackWords = 0;
  const at = view.label;
  LINES.clear();
  const expected = await view.expected();
  await page.goto(`${BASE}${view.path}`, { waitUntil: "domcontentloaded" });
  await page.waitForSelector("[data-word-count]", { timeout: LOAD_TIMEOUT_MS }).catch(() => {});
  const loaded = await loadAll(page, expected.size);
  // Give page fonts a moment so glyph mode can kick in before we read the DOM.
  await page.evaluate(() => document.fonts.ready).catch(() => {});
  await sleep(500);
  const { anchors, words, markers, headers, lines, missingPlaceholders, auditFailures } = await rendered(page);
  for (const l of lines) {
    if (l.wrapped) problems.push({ at, kind: "line-wrapped", detail: `page ${l.page} line ${l.line}` });
    if (l.overflow > 1) problems.push({ at, kind: "line-overflow", detail: `page ${l.page} line ${l.line}: ${l.overflow}px wider than its page` });
  }
  // A page view must show that page's lines whole: as many lines, and as many words on each, as printed.
  if (view.mushafPage) {
    const want = new Map();
    for (const [k, slot] of LINES) {
      if (!k.startsWith(`/verses/by_page/`)) continue;
      const [pg, ln] = slot.split(":").map(Number);
      if (pg === view.mushafPage) want.set(ln, (want.get(ln) ?? new Set()).add(k.split("|")[2]));
    }
    const got = new Map(lines.filter((l) => l.page === view.mushafPage).map((l) => [l.line, l.words]));
    for (const [ln, set] of want) {
      if (got.get(ln) !== set.size) problems.push({ at, kind: "line-word-count", detail: `line ${ln}: ${got.get(ln) ?? 0} of ${set.size} words` });
    }
    for (const ln of got.keys()) if (!want.has(ln)) problems.push({ at, kind: "unexpected-line", detail: `line ${ln}` });
    if (lines.some((l) => l.page !== view.mushafPage)) problems.push({ at, kind: "other-page-shown", detail: [...new Set(lines.map((l) => l.page))].join(",") });
  }
  for (const f of auditFailures) problems.push({ at, kind: "app-render-audit", detail: f });
  const familyFor = (p) => (FONT === "plain" ? `p${p}-v2` : `p${p}-v4`);

  if (await page.locator(".integrity-banner").count()) {
    problems.push({ at, kind: "integrity-banner", detail: (await page.locator(".integrity-banner").first().innerText()).slice(0, 300) });
  }
  if (loaded < expected.size) problems.push({ at, kind: "not-all-verses-loaded", detail: `${loaded}/${expected.size}` });
  if (missingPlaceholders) problems.push({ at, kind: "word-missing-placeholder", detail: missingPlaceholders });

  // Verse anchors: present once each, nothing unexpected, declared counts right.
  const seen = new Set();
  for (const a of anchors) {
    if (seen.has(a.key)) problems.push({ at, key: a.key, kind: "duplicate-verse" });
    seen.add(a.key);
    const exp = expected.get(a.key);
    if (!exp) { problems.push({ at, key: a.key, kind: "unexpected-verse" }); continue; }
    const total = exp.total ?? exp.length;
    if (a.declared !== total) problems.push({ at, key: a.key, kind: "declared-count", detail: `data-word-count ${a.declared}, API ${total}` });
  }
  for (const key of expected.keys()) if (!seen.has(key)) problems.push({ at, key, kind: "missing-verse" });

  // Words, per verse, in order.
  const byVerse = new Map();
  for (const w of words) {
    const key = w.location.split(":").slice(0, 2).join(":");
    if (!byVerse.has(key)) byVerse.set(key, []);
    byVerse.get(key).push(w);
  }
  for (const [key, exp] of expected) {
    const got = byVerse.get(key) ?? [];
    if (got.length !== exp.length) {
      problems.push({ at, key, kind: "word-count", detail: `rendered ${got.length}, expected ${exp.length}` });
    }
    const n = Math.min(got.length, exp.length);
    for (let i = 0; i < n; i++) {
      const g = got[i], e = exp[i];
      if (g.location !== e.location) { problems.push({ at, key, kind: "word-order", detail: `#${i + 1}: rendered ${g.location}, expected ${e.location}` }); break; }
      const text = norm(g.text);
      if (g.mode === "glyph") {
        if (text !== e.glyph) problems.push({ at, key, kind: "glyph", detail: `${e.location}: rendered ${JSON.stringify(text)}, expected ${JSON.stringify(e.glyph)}` });
        if (e.page != null && g.family !== familyFor(e.page)) problems.push({ at, key, kind: "glyph-font", detail: `${e.location}: drawn in "${g.family}", expected "${familyFor(e.page)}"` });
        if (g.covered === false) problems.push({ at, key, kind: "glyph-not-in-font", detail: `${e.location}: font "${g.family}" lacks this glyph; the browser drew a substitute` });
      } else {
        fallbackWords++;
        if (text !== e.text && text !== e.alt) problems.push({ at, key, kind: "word-text", detail: `${e.location}: rendered "${text}", expected "${e.text}"` });
      }
      if (g.cut) problems.push({ at, key, kind: "cut-off", detail: `${e.location}: ${g.cut}` });
      if (e.page != null && g.page !== e.page) problems.push({ at, key, kind: "page", detail: `${e.location}: on page ${g.page}, API ${e.page}` });
      if (e.line != null && g.line !== e.line) problems.push({ at, key, kind: "line", detail: `${e.location}: on line ${g.line}, API ${e.line}` });
    }
  }
  // Ayah markers: one per verse, after its last word, right glyph and font.
  const markersByKey = new Map();
  for (const m of markers) {
    if (!markersByKey.has(m.key)) markersByKey.set(m.key, []);
    markersByKey.get(m.key).push(m);
  }
  for (const [key, exp] of expected) {
    const ms = markersByKey.get(key) ?? [];
    // On a page view a verse that runs on to the next page has its marker there.
    const want = exp.marker === false ? 0 : 1;
    if (ms.length !== want) { problems.push({ at, key, kind: "ayah-marker-count", detail: `${ms.length} markers, expected ${want}` }); continue; }
    if (!want) continue;
    const m = ms[0];
    const got = byVerse.get(key) ?? [];
    const last = got[got.length - 1];
    if (last && !(m.order > last.order)) problems.push({ at, key, kind: "ayah-marker-position", detail: "marker is not after the verse's last word" });
    if (m.mode === "glyph") {
      const exp = ENDS.get(key) ?? "";
      if (norm(m.text ?? "") !== exp) problems.push({ at, key, kind: "ayah-marker-glyph", detail: `rendered ${JSON.stringify(m.text)}, expected ${JSON.stringify(exp)}` });
      if (m.page != null && m.family !== familyFor(m.page)) problems.push({ at, key, kind: "ayah-marker-font", detail: `"${m.family}" vs "${familyFor(m.page)}"` });
      if (m.covered === false) problems.push({ at, key, kind: "ayah-marker-not-in-font" });
    }
  }
  for (const key of markersByKey.keys()) if (!expected.has(key)) problems.push({ at, key, kind: "unexpected-ayah-marker" });

  // Surah headers: exactly for the surahs whose verse 1 is shown, directly before
  // X:1:1, with a Bismillah line except for surahs 1 and 9.
  const startsHere = [...expected].filter(([k, exp]) => k.endsWith(":1") && exp.starts !== false).map(([k]) => Number(k.split(":")[0]));
  for (const c of startsHere) {
    const hs = headers.filter((h) => h.chapter === c);
    if (hs.length !== 1) { problems.push({ at, kind: "surah-header-count", detail: `surah ${c}: ${hs.length} headers` }); continue; }
    const h = hs[0];
    if (h.next !== `${c}:1:1`) problems.push({ at, kind: "surah-header-position", detail: `surah ${c}: header is followed by ${h.next}, expected ${c}:1:1` });
    const wantBismillah = c !== 1 && c !== 9;
    if (h.bismillah !== wantBismillah) problems.push({ at, kind: "bismillah", detail: `surah ${c}: bismillah ${h.bismillah ? "present" : "missing"}` });
  }
  for (const h of headers) if (!startsHere.includes(h.chapter)) problems.push({ at, kind: "unexpected-surah-header", detail: `surah ${h.chapter}` });

  if (fallbackWords && !ALLOW_FALLBACK) {
    problems.push({ at, kind: "font-fallback", detail: `${fallbackWords} word(s) shown as Unicode text because the page font did not load (no words lost)` });
  }
  return { verses: expected.size, words: [...expected.values()].reduce((s, w) => s + w.length, 0), fallbackWords, problems };
}

function buildViews() {
  const views = [];
  if (VIEWS.includes("chapters")) {
    for (const c of CHAPTERS) views.push({ label: `surah ${c}`, path: `/${c}`, expected: () => expectedWords(`/verses/by_chapter/${c}`) });
  }
  if (VIEWS.includes("ranges")) {
    for (const r of RANGES) {
      const m = r.match(/^(\d+)\/(\d+)(?:-(\d+))?$/);
      if (!m) { console.error(`bad range ${r}`); continue; }
      const [c, a, b] = [Number(m[1]), Number(m[2]), Number(m[3] ?? m[2])];
      const path = a === b ? `/${c}/${a}` : `/${c}/${a}-${b}`;
      views.push({
        label: `range ${path}`, path,
        expected: () => expectedWords(`/verses/by_chapter/${c}`, (v) => v.verse_number >= a && v.verse_number <= b),
      });
    }
  }
  if (VIEWS.includes("juz")) {
    for (const j of JUZ) views.push({ label: `juz ${j}`, path: `/juz/${j}`, expected: () => expectedWords(`/verses/by_juz/${j}`) });
  }
  if (VIEWS.includes("pages")) {
    for (const p of PAGES) views.push({ label: `page ${p}`, path: `/page/${p}`, mushafPage: p, expected: () => expectedOnPage(p) });
  }
  return views;
}

// --browser PATH uses an installed Chromium-based browser instead of Playwright's own download.
// (`--disable-vivaldi` makes Vivaldi behave as plain Chromium; other browsers ignore it.)
const browser = await chromium.launch(args.browser ? { executablePath: String(args.browser), args: ["--disable-vivaldi"] } : {});
const context = await browser.newContext();
// Start from known settings so rendered text compares against the matching API field.
const settings = {
  mushaf_font: FONT === "plain" ? "MadaniV2" : "TajweedV4",
  word_by_word: { show_translation: true, show_transliteration: true, display: WBW === "tooltip" ? "Tooltip" : "Inline" },
};
await context.addInitScript((json) => {
  try { localStorage.setItem("quran-settings-v1", json); } catch {}
}, JSON.stringify(settings));
const page = await context.newPage();
page.on("pageerror", (e) => console.error("  page error:", e.message));

const views = buildViews();
const report = { base: BASE, api: API, font: FONT, wbw: WBW, views: [], totals: { views: views.length, verses: 0, words: 0, problems: 0 } };
for (const view of views) {
  try {
    const r = await checkView(page, view);
    report.views.push({ view: view.label, path: view.path, ...r });
    report.totals.verses += r.verses;
    report.totals.words += r.words;
    report.totals.problems += r.problems.length;
    console.log(`${view.label.padEnd(18)} ${r.verses} verses, ${r.words} words — ${r.problems.length ? `${r.problems.length} PROBLEM(S)` : "ok"}`);
    for (const p of r.problems.slice(0, 5)) console.log("     ", JSON.stringify(p));
  } catch (e) {
    report.views.push({ view: view.label, path: view.path, error: String(e) });
    report.totals.problems += 1;
    console.log(`${view.label.padEnd(18)} ERROR ${e}`);
  }
}
await browser.close();

writeFileSync("report.json", JSON.stringify(report, null, 2));
console.log(`\nChecked ${views.length} views (${FONT}, ${WBW}), ${report.totals.verses} verses, ${report.totals.words} words: ${report.totals.problems} problem(s). Details in report.json`);
process.exit(report.totals.problems ? 1 : 0);
