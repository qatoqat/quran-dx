#!/usr/bin/env node
// End-to-end "no missing words" check for the web build.
//
// For each view (surahs, verse ranges, juz, mushaf pages) it fetches the expected
// words straight from the Quran.com API (independent of the app's own code), opens
// the view in a real browser, loads every verse, and compares verse by verse:
//   - every verse key is on the page, none extra, none duplicated
//   - every word of every verse is rendered, in order, at its `c:v:position`
//   - each word shows the right text: the QCF glyph (`code_v2`) when its page font
//     loaded, otherwise the Unicode fallback (`qpc_uthmani_hafs`/`text_uthmani`)
//   - each word sits on the API's mushaf page and line (15-line layout)
//   - glyph words use their own page's QCF font, and that font really contains
//     the glyph (canvas check: inked, and different from a system-font letter)
//   - exactly one ayah marker per verse, after its last word, with the right glyph
//   - each surah header (and Bismillah, except surahs 1 and 9) sits directly
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
// Options: --views chapters,ranges,juz,pages  --chapters SPEC  --juz SPEC  --pages SPEC
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
async function expectedWords(apiPath, keep = () => true) {
  const out = new Map();
  for (let page = 1; ; page++) {
    const data = await apiGet(apiPath, {
      words: "true", per_page: "50", page: String(page), language: "en", mushaf: String(MUSHAF_ID),
      word_fields: "location,code_v2,v2_page,page_number,line_number,qpc_uthmani_hafs,text_uthmani",
    });
    if (!data.pagination) throw new Error(`no pagination in ${apiPath} page ${page}`);
    for (const v of data.verses) {
      if (!keep(v)) continue;
      const end = v.words.find((w) => w.char_type_name === "end");
      ENDS.set(v.verse_key, norm(end?.code_v2 ?? ""));
      out.set(v.verse_key, v.words.filter((w) => w.char_type_name !== "end").map((w) => ({
        location: w.location ?? `${v.verse_key}:${w.position}`,
        glyph: norm(w.code_v2 ?? ""),
        text: norm(w.qpc_uthmani_hafs ?? w.text_uthmani ?? w.text ?? ""),
        alt: norm(w.text_uthmani ?? w.text ?? ""),
        page: w.page_number ?? null,
        line: w.line_number ?? null,
      })));
    }
    if (!data.pagination.next_page) break;
    await sleep(100);
  }
  return out;
}

const norm = (s) => s.normalize("NFC").replace(/\s+/g, " ").trim();

/** Load every verse of the surah in the page (clicking "Load more" as needed). */
async function loadAll(page, expectedCount) {
  const count = () => page.locator(".verse-anchor[data-verse-key]").count();
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
          coverCache.set(k, ink && !same);
        }
        if (!coverCache.get(k)) return false;
      }
      return true;
    };
    const firstFamily = (el) => getComputedStyle(el).fontFamily.split(",")[0].trim().replace(/^["']|["']$/g, "");

    const anchors = [...document.querySelectorAll(".verse-anchor[data-verse-key]")].map((a) => ({
      key: a.dataset.verseKey,
      declared: Number(a.dataset.wordCount),
    }));
    // Document order of everything we check positions of.
    const ordered = [...document.querySelectorAll(".word[data-location], .ayah-marker[data-verse-key], .surah-header-line[data-chapter], .bismillah-line")];
    const index = new Map(ordered.map((el, i) => [el, i]));
    const words = [...document.querySelectorAll(".word[data-location]")].map((w) => {
      const t = w.querySelector(".word-text") ?? w;
      const text = t.textContent;
      const family = w.dataset.mode === "glyph" ? firstFamily(t.querySelector(".glyph-text") ?? t) : null;
      return {
        location: w.dataset.location,
        mode: w.dataset.mode,
        page: w.dataset.page ? Number(w.dataset.page) : null,
        line: w.dataset.line ? Number(w.dataset.line) : null,
        text,
        family,
        covered: family ? covers(family, text.trim()) : null,
        order: index.get(w),
      };
    });
    const markers = [...document.querySelectorAll(".ayah-marker[data-verse-key]")].map((m) => {
      const glyphEl = m.querySelector(".glyph-text");
      const text = (glyphEl ?? m.querySelector(".word-text") ?? m).textContent;
      const family = m.dataset.mode === "glyph" ? firstFamily(glyphEl ?? m) : null;
      return {
        key: m.dataset.verseKey,
        mode: m.dataset.mode,
        page: m.dataset.page ? Number(m.dataset.page) : null,
        text: glyphEl ? glyphEl.textContent : null,
        family,
        covered: family && glyphEl ? covers(family, glyphEl.textContent.trim()) : null,
        order: index.get(m),
      };
    });
    const headers = [...document.querySelectorAll(".surah-header-line[data-chapter]")].map((h) => {
      // The next checked element after the header, skipping a Bismillah line.
      let i = index.get(h) + 1;
      const bismillah = ordered[i]?.classList.contains("bismillah-line") ?? false;
      if (bismillah) i++;
      const next = ordered[i];
      return { chapter: Number(h.dataset.chapter), bismillah, next: next?.dataset?.location ?? null };
    });
    const missingPlaceholders = document.querySelectorAll(".word-missing").length;
    // The app's own on-screen audit (render_audit.rs) marks failed words.
    const auditFailures = [...document.querySelectorAll(".word-render-error")].map((b) =>
      `${b.closest("[data-location]")?.dataset.location ?? "?"}: ${b.getAttribute("aria-label") ?? ""}`);
    return { anchors, words, markers, headers, missingPlaceholders, auditFailures };
  });
}

async function checkView(page, view) {
  const problems = [];
  let fallbackWords = 0;
  const at = view.label;
  const expected = await view.expected();
  await page.goto(`${BASE}${view.path}`, { waitUntil: "domcontentloaded" });
  await page.waitForSelector(".verse-anchor[data-verse-key]", { timeout: LOAD_TIMEOUT_MS }).catch(() => {});
  const loaded = await loadAll(page, expected.size);
  // Give page fonts a moment so glyph mode can kick in before we read the DOM.
  await page.evaluate(() => document.fonts.ready).catch(() => {});
  await sleep(500);
  const { anchors, words, markers, headers, missingPlaceholders, auditFailures } = await rendered(page);
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
    if (a.declared !== exp.length) problems.push({ at, key: a.key, kind: "declared-count", detail: `data-word-count ${a.declared}, API ${exp.length}` });
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
  for (const key of expected.keys()) {
    const ms = markersByKey.get(key) ?? [];
    if (ms.length !== 1) { problems.push({ at, key, kind: "ayah-marker-count", detail: `${ms.length} markers` }); continue; }
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
  const startsHere = [...expected.keys()].filter((k) => k.endsWith(":1")).map((k) => Number(k.split(":")[0]));
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
    for (const p of PAGES) views.push({ label: `page ${p}`, path: `/page/${p}`, expected: () => expectedWords(`/verses/by_page/${p}`) });
  }
  return views;
}

const browser = await chromium.launch();
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
