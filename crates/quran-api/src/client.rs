//! HTTP client. Uses `reqwest`, which compiles to `fetch` on wasm32.

use crate::{DEFAULT_BASE_URL, query::VerseQuery, types::*};
use serde::de::DeserializeOwned;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("API returned HTTP {status}: {body}")]
    Status { status: u16, body: String },
    #[error("could not parse API response: {0}")]
    Decode(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone)]
pub struct Client {
    base_url: String,
    http: reqwest::Client,
}

impl Default for Client {
    fn default() -> Self {
        Self::new(DEFAULT_BASE_URL)
    }
}

impl PartialEq for Client {
    fn eq(&self, other: &Self) -> bool {
        self.base_url == other.base_url
    }
}

impl Client {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into().trim_end_matches('/').to_string(),
            http: build_http(),
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Build the full URL for `path` (must start with `/`) and query pairs.
    pub fn url(&self, path: &str, query: &[(&str, String)]) -> String {
        let mut url = format!("{}{}", self.base_url, path);
        if !query.is_empty() {
            url.push('?');
            let encoded: Vec<String> = query
                .iter()
                .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
                .collect();
            url.push_str(&encoded.join("&"));
        }
        url
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, query: &[(&str, String)]) -> Result<T> {
        let resp = self
            .http
            .get(self.url(path, query))
            .header("Accept", "application/json")
            .send()
            .await?;
        let status = resp.status();
        let body = resp.text().await?;
        if !status.is_success() {
            return Err(Error::Status { status: status.as_u16(), body: truncate(&body, 300) });
        }
        parse_json(&body)
    }

    /// All 114 chapters, names translated into `language`.
    pub async fn chapters(&self, language: &str) -> Result<Vec<Chapter>> {
        let r: ChaptersResponse = self.get("/chapters", &[("language", language.into())]).await?;
        Ok(r.chapters)
    }

    pub async fn chapter(&self, id: u16, language: &str) -> Result<Chapter> {
        let r: ChapterResponse =
            self.get(&format!("/chapters/{id}"), &[("language", language.into())]).await?;
        Ok(r.chapter)
    }

    pub async fn verses_by_chapter(&self, chapter: u16, q: &VerseQuery) -> Result<VersesResponse> {
        self.get(&format!("/verses/by_chapter/{chapter}"), &q.to_pairs()).await
    }

    pub async fn verses_by_page(&self, page: u16, q: &VerseQuery) -> Result<VersesResponse> {
        self.get(&format!("/verses/by_page/{page}"), &q.to_pairs()).await
    }

    pub async fn verses_by_juz(&self, juz: u8, q: &VerseQuery) -> Result<VersesResponse> {
        self.get(&format!("/verses/by_juz/{juz}"), &q.to_pairs()).await
    }

    /// Single verse, `key` like `"2:255"`.
    pub async fn verse_by_key(&self, key: &str, q: &VerseQuery) -> Result<Verse> {
        let r: VerseResponse = self.get(&format!("/verses/by_key/{key}"), &q.to_pairs()).await?;
        Ok(r.verse)
    }

    pub async fn juzs(&self) -> Result<Vec<Juz>> {
        let r: JuzsResponse = self.get("/juzs", &[]).await?;
        Ok(r.juzs)
    }

    /// Available translations; names localized into `language`.
    pub async fn translations(&self, language: &str) -> Result<Vec<TranslationResource>> {
        let r: TranslationsResponse =
            self.get("/resources/translations", &[("language", language.into())]).await?;
        Ok(r.translations)
    }

    pub async fn languages(&self, language: &str) -> Result<Vec<Language>> {
        let r: LanguagesResponse =
            self.get("/resources/languages", &[("language", language.into())]).await?;
        Ok(r.languages)
    }

    pub async fn footnote(&self, id: u32) -> Result<FootNote> {
        let r: FootNoteResponse = self.get(&format!("/foot_notes/{id}"), &[]).await?;
        Ok(r.foot_note)
    }

    /// Full-text search. `page` is 1-based.
    pub async fn search(&self, query: &str, size: u32, page: u32, language: &str) -> Result<Search> {
        let r: SearchResponse = self
            .get(
                "/search",
                &[
                    ("q", query.into()),
                    ("size", size.to_string()),
                    ("page", page.to_string()),
                    ("language", language.into()),
                ],
            )
            .await?;
        Ok(r.search)
    }
}

/// Parse an API response body. The API repeats some keys in an object when a
/// field it sends by default is also requested explicitly (e.g. `line_number` in
/// `word_fields`), and derived serde types reject duplicate fields. Going through
/// `serde_json::Value` first keeps the last value of a repeated key, as browsers'
/// `JSON.parse` does, so such responses still load.
pub fn parse_json<T: DeserializeOwned>(body: &str) -> Result<T> {
    let value: serde_json::Value = serde_json::from_str(body)?;
    Ok(serde_json::from_value(value)?)
}

/// On native targets give every request a deadline so a hung connection turns into
/// a visible error (with retry) instead of an endless "Loading…". In the browser,
/// `fetch` has its own network timeouts and reqwest's wasm client has no timeout option.
fn build_http() -> reqwest::Client {
    #[cfg(not(target_arch = "wasm32"))]
    {
        reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(15))
            .timeout(std::time::Duration::from_secs(45))
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    }
    #[cfg(target_arch = "wasm32")]
    {
        reqwest::Client::new()
    }
}

/// Minimal application/x-www-form-urlencoded-style percent encoding (RFC 3986 unreserved kept).
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b',' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_urls() {
        let c = Client::new("https://example.com/api/v4/");
        assert_eq!(
            c.url("/search", &[("q", "light & dark".into()), ("size", "5".into())]),
            "https://example.com/api/v4/search?q=light%20%26%20dark&size=5"
        );
    }

    #[test]
    fn encodes_arabic() {
        assert_eq!(encode("نور"), "%D9%86%D9%88%D8%B1");
    }
}
