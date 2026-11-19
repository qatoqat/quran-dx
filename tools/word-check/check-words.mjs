#!/usr/bin/env node
// End-to-end "no missing words" check for the web build.
//
// For each surah it fetches the expected words straight from the Quran.com API
// (independent of the app's own code), opens the surah in a real browser, loads
// every verse, and compares verse by verse:
//   - every verse key is on the page, none extra, none duplicated
//   - number of rendered Arabic words == number of non-end words from the API
//   - the rendered word texts equal the API's text_uthmani, in order
//   - no `.word-missing` placeholders and no verses that fell back to verse text
//
// Usage (with the app running via `cd app && dx serve --platform web`):
//   cd tools/word-check && npm install && npx playwright install chromium
//   node check-words.mjs                       # all 114 surahs at http://localhost:8080
//   node check-words.mjs --chapters 1-10,18    # subset
//   node check-words.mjs --base http://127.0.0.1:8080 --api https://api.quran.com/api/v4
// Exit code 0 = everything matched, 1 = problems found (see report.json).

import { chromium } from "playwright";
import { writeFileSync } from "node:fs";

const args = parseArgs(process.argv.slice(2));
const BASE = (args.base ?? "http://localhost:8080").replace(/\/$/, "");
const API = (args.api ?? "https://api.quran.com/api/v4").replace(/\/$/, "");
const CHAPTERS = parseChapters(args.chapters ?? "1-114");
const LOAD_TIMEOUT_MS = Number(args.timeout ?? 20000);

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i++) {
    if (argv[i].startsWith("--")) out[argv[i].slice(2)] = argv[i + 1]?.startsWith("--") ? true : argv[++i];
  }
  return out;
}

function parseChapters(spec) {
  const set = new Set();
  for (const part of String(spec).split(",")) {
    const [a, b] = part.split("-").map(Number);
    for (let n = a; n <= (b || a); n++) if (n >= 1 && n <= 114) set.add(n);
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

/** verse_key -> array of expected word texts (non-end words), from the API. */
async function expectedWords(chapter) {
  const out = new Map();
  for (let page = 1; ; page++) {
    const data = await apiGet(`/verses/by_chapter/${chapter}`, {
      words: "true", word_fields: "text_uthmani", per_page: "50", page: String(page), language: "en",
    });
    for (const v of data.verses) {
      out.set(v.verse_key, v.words.filter((w) => w.char_type_name !== "end").map((w) => norm(w.text_uthmani ?? w.text ?? "")));
    }
    if (!data.pagination?.next_page) break;
    await sleep(100);
  }
  return out;
}

const norm = (s) => s.normalize("NFC").replace(/\s+/g, " ").trim();

/** Load every verse of the surah in the page (clicking "Load more" as needed). */
async function loadAll(page, expectedCount) {
  const count = () => page.locator("article[data-verse-key]").count();
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

/** What the page actually shows, per verse. */
async function renderedWords(page) {
  return page.$$eval("article[data-verse-key]", (articles) =>
    articles.map((a) => {
      const words = [...a.querySelectorAll(".verse-arabic .word")].map((w) => {
        // Tooltip/inline modes nest the Arabic in .word-text next to wbw extras.
        const t = w.querySelector(".word-text") ?? w;
        return t.textContent;
      });
      return {
        key: a.dataset.verseKey,
        declared: Number(a.dataset.wordCount),
        fallback: a.dataset.fallback === "true",
        missing: a.querySelectorAll(".word-missing").length,
        words,
      };
    }),
  );
}

async function checkChapter(page, chapter) {
  const problems = [];
  const expected = await expectedWords(chapter);
  await page.goto(`${BASE}/${chapter}`, { waitUntil: "domcontentloaded" });
  await page.waitForSelector("article[data-verse-key]", { timeout: LOAD_TIMEOUT_MS }).catch(() => {});
  const loaded = await loadAll(page, expected.size);
  const rendered = await renderedWords(page);

  if (await page.locator(".integrity-banner").count()) {
    problems.push({ chapter, kind: "integrity-banner", detail: (await page.locator(".integrity-banner").first().innerText()).slice(0, 300) });
  }
  if (loaded < expected.size) problems.push({ chapter, kind: "not-all-verses-loaded", detail: `${loaded}/${expected.size}` });

  const seen = new Set();
  for (const r of rendered) {
    if (seen.has(r.key)) problems.push({ chapter, key: r.key, kind: "duplicate-verse" });
    seen.add(r.key);
    const exp = expected.get(r.key);
    if (!exp) { problems.push({ chapter, key: r.key, kind: "unexpected-verse" }); continue; }
    const got = r.words.map(norm);
    if (r.fallback) problems.push({ chapter, key: r.key, kind: "fell-back-to-verse-text" });
    if (r.missing) problems.push({ chapter, key: r.key, kind: "word-missing-placeholder", detail: r.missing });
    if (got.length !== exp.length) {
      problems.push({ chapter, key: r.key, kind: "word-count", detail: `rendered ${got.length}, expected ${exp.length}` });
    } else if (!r.fallback) {
      const i = got.findIndex((w, idx) => w !== exp[idx]);
      if (i >= 0) problems.push({ chapter, key: r.key, kind: "word-text", detail: `position ${i + 1}: rendered "${got[i]}", expected "${exp[i]}"` });
    }
    if (r.declared !== got.length) problems.push({ chapter, key: r.key, kind: "declared-count", detail: `data-word-count ${r.declared}, rendered ${got.length}` });
  }
  for (const key of expected.keys()) if (!seen.has(key)) problems.push({ chapter, key, kind: "missing-verse" });
  return { verses: expected.size, words: [...expected.values()].reduce((s, w) => s + w.length, 0), problems };
}

const browser = await chromium.launch();
const context = await browser.newContext();
// Start from default settings (Uthmani script) so texts compare against text_uthmani.
await context.addInitScript(() => { try { localStorage.removeItem("quran-settings-v1"); } catch {} });
const page = await context.newPage();
page.on("pageerror", (e) => console.error("  page error:", e.message));

const report = { base: BASE, api: API, chapters: [], totals: { verses: 0, words: 0, problems: 0 } };
for (const chapter of CHAPTERS) {
  try {
    const r = await checkChapter(page, chapter);
    report.chapters.push({ chapter, ...r });
    report.totals.verses += r.verses;
    report.totals.words += r.words;
    report.totals.problems += r.problems.length;
    console.log(`${String(chapter).padStart(3)}: ${r.verses} verses, ${r.words} words — ${r.problems.length ? `${r.problems.length} PROBLEM(S)` : "ok"}`);
    for (const p of r.problems.slice(0, 5)) console.log("     ", JSON.stringify(p));
  } catch (e) {
    report.chapters.push({ chapter, error: String(e) });
    report.totals.problems += 1;
    console.log(`${String(chapter).padStart(3)}: ERROR ${e}`);
  }
}
await browser.close();

writeFileSync("report.json", JSON.stringify(report, null, 2));
console.log(`\nChecked ${CHAPTERS.length} surahs, ${report.totals.verses} verses, ${report.totals.words} words: ${report.totals.problems} problem(s). Details in report.json`);
process.exit(report.totals.problems ? 1 : 0);
