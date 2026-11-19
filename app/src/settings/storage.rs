//! Settings persistence: localStorage on web, a JSON file on desktop.
//!
//! All parsing/serialization lives in plain functions so it can be unit-tested
//! on any target. Missing or corrupt data yields `None`; nothing here panics.

use super::Settings;

/// Serialize settings to the JSON stored on disk / in localStorage.
pub fn to_json(settings: &Settings) -> String {
    // Serializing plain data structs can't fail; fall back to an empty object just in case.
    serde_json::to_string(settings).unwrap_or_else(|_| "{}".into())
}

/// Parse stored JSON. Unknown fields are ignored and missing ones take defaults;
/// malformed JSON or wrongly-typed values yield `None`. Values are normalized.
pub fn from_json(raw: &str) -> Option<Settings> {
    let raw = raw.trim();
    if raw.is_empty() {
        return None;
    }
    let mut settings: Settings = serde_json::from_str(raw).ok()?;
    normalize(&mut settings);
    Some(settings)
}

/// Clamp/repair values that may have been hand-edited or written by older versions.
pub fn normalize(s: &mut Settings) {
    s.arabic_font_scale = s.arabic_font_scale.clamp(1, 10);
    s.translation_font_scale = s.translation_font_scale.clamp(1, 10);
    let locale = s.locale.trim();
    let valid = !locale.is_empty()
        && locale.len() <= 16
        && locale.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    s.locale = if valid { locale.to_string() } else { Settings::default().locale };
    let mut seen = std::collections::HashSet::new();
    s.translations.retain(|id| seen.insert(*id));
}

/// Load persisted settings for the current platform.
pub fn load() -> Option<Settings> {
    // Chosen by build target, not just features: a desktop build may still have the
    // `web` feature enabled, and web-sys calls panic on native targets.
    #[cfg(all(feature = "web", target_arch = "wasm32"))]
    return web_backend::load();
    #[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
    return desktop_backend::load();
    #[allow(unreachable_code)]
    None
}

/// Persist settings for the current platform. Failures are silently ignored.
pub fn save(settings: &Settings) {
    #[cfg(all(feature = "web", target_arch = "wasm32"))]
    web_backend::save(settings);
    #[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
    desktop_backend::save(settings);
    let _ = settings;
}

/// Read settings from a JSON file. Missing/unreadable/corrupt file → `None`.
#[cfg_attr(any(feature = "web", not(feature = "desktop")), allow(dead_code))]
pub fn load_from_path(path: &std::path::Path) -> Option<Settings> {
    let raw = std::fs::read_to_string(path).ok()?;
    from_json(&raw)
}

/// Atomically write settings JSON to `path`: create the parent directory, write a
/// sibling temp file, then rename it over the target.
#[cfg_attr(any(feature = "web", not(feature = "desktop")), allow(dead_code))]
pub fn save_to_path(path: &std::path::Path, settings: &Settings) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut tmp_name = path.file_name().unwrap_or_default().to_os_string();
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);
    let result = (|| {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(to_json(settings).as_bytes())?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Compiled whenever `web` is on (so it is type-checked on native too), used only on wasm32.
#[cfg(feature = "web")]
#[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
mod web_backend {
    use super::Settings;

    /// localStorage key.
    pub const STORAGE_KEY: &str = "quran-settings-v1";

    fn storage() -> Option<web_sys::Storage> {
        // `local_storage()` errors e.g. in sandboxed iframes / privacy modes.
        web_sys::window()?.local_storage().ok().flatten()
    }

    pub fn load() -> Option<Settings> {
        let raw = storage()?.get_item(STORAGE_KEY).ok().flatten()?;
        super::from_json(&raw)
    }

    pub fn save(settings: &Settings) {
        if let Some(s) = storage() {
            // Quota errors etc. are non-fatal: settings just won't persist.
            let _ = s.set_item(STORAGE_KEY, &super::to_json(settings));
        }
    }
}

#[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
mod desktop_backend {
    use super::Settings;
    use std::path::PathBuf;

    /// `<config dir>/quran-app/settings.json`
    fn settings_path() -> Option<PathBuf> {
        Some(dirs::config_dir()?.join("quran-app").join("settings.json"))
    }

    pub fn load() -> Option<Settings> {
        super::load_from_path(&settings_path()?)
    }

    pub fn save(settings: &Settings) {
        if let Some(path) = settings_path() {
            // Non-fatal: settings just won't persist (e.g. read-only home).
            let _ = super::save_to_path(&path, settings);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{QuranScript, WbwDisplay};

    #[test]
    fn roundtrip() {
        let s = Settings {
            locale: "ur".into(),
            translations: vec![97, 131],
            script: QuranScript::IndoPak,
            arabic_font_scale: 9,
            word_by_word: crate::settings::WordByWord {
                display: WbwDisplay::Inline,
                show_transliteration: true,
                show_translation: false,
            },
            ..Default::default()
        };
        assert_eq!(from_json(&to_json(&s)), Some(s));
    }

    #[test]
    fn corrupt_or_empty_is_none() {
        for raw in ["", "   ", "not json", "{", "[1,2]", "null", "42", r#"{"translations": "x"}"#,
            r#"{"script": "Klingon"}"#, r#"{"arabic_font_scale": 300}"#, "\u{0}\u{1}"]
        {
            assert_eq!(from_json(raw), None, "input: {raw:?}");
        }
    }

    #[test]
    fn partial_uses_defaults_and_ignores_unknown() {
        let s = from_json(r#"{"locale":"id","future_field":true,"word_by_word":{"display":"Inline"}}"#)
            .unwrap();
        assert_eq!(s.locale, "id");
        assert_eq!(s.translations, Settings::default().translations);
        assert_eq!(s.word_by_word.display, WbwDisplay::Inline);
        assert!(s.word_by_word.show_translation);
        assert_eq!(from_json("{}"), Some(Settings::default()));
    }

    #[test]
    fn values_are_normalized() {
        let s = from_json(
            r#"{"arabic_font_scale":0,"translation_font_scale":99,"locale":"<script>","translations":[1,2,1,3,2]}"#,
        )
        .unwrap();
        assert_eq!(s.arabic_font_scale, 1);
        assert_eq!(s.translation_font_scale, 10);
        assert_eq!(s.locale, "en");
        assert_eq!(s.translations, vec![1, 2, 3]);
    }

    #[test]
    fn empty_translation_list_is_kept() {
        let s = from_json(r#"{"translations":[]}"#).unwrap();
        assert!(s.translations.is_empty());
    }

    #[test]
    fn file_roundtrip_atomic_and_corrupt() {
        let dir = std::env::temp_dir().join(format!("quran-app-test-{}", std::process::id()));
        let path = dir.join("nested").join("settings.json");
        let _ = std::fs::remove_dir_all(&dir);

        assert_eq!(load_from_path(&path), None, "missing file");

        let mut s = Settings { translation_font_scale: 7, ..Default::default() };
        save_to_path(&path, &s).unwrap();
        assert_eq!(load_from_path(&path), Some(s.clone()));
        assert!(!path.with_file_name("settings.json.tmp").exists(), "temp file renamed away");

        // Overwrite existing file.
        s.locale = "fr".into();
        save_to_path(&path, &s).unwrap();
        assert_eq!(load_from_path(&path).unwrap().locale, "fr");

        std::fs::write(&path, b"{\"locale\": ").unwrap();
        assert_eq!(load_from_path(&path), None, "corrupt file");
        std::fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
        assert_eq!(load_from_path(&path), None, "non-utf8 file");

        let _ = std::fs::remove_dir_all(&dir);
    }
}
