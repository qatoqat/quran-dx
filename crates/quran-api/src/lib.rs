//! Typed async client for the public Quran.com API v4 (<https://api.quran.com/api/v4>).
//!
//! Works on native targets (desktop) and `wasm32` (browser, via `fetch`).
//! The base URL is configurable because the public API may move or require auth.

pub mod client;
pub mod query;
pub mod types;

pub use client::{Client, Error, parse_json};
pub use query::VerseQuery;
pub use types::*;

/// Default public endpoint.
pub const DEFAULT_BASE_URL: &str = "https://api.quran.com/api/v4";
