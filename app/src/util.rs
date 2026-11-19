//! Small cross-platform helpers.

/// Async sleep: JS timers in the browser, tokio on desktop.
pub async fn sleep_ms(ms: u32) {
    #[cfg(target_arch = "wasm32")]
    gloo_timers::future::TimeoutFuture::new(ms).await;
    #[cfg(not(target_arch = "wasm32"))]
    tokio::time::sleep(std::time::Duration::from_millis(ms.into())).await;
}

/// Wraps the body of a script whose result is awaited so that its `dioxus`
/// channel object stays referenced until a few seconds after it returned.
///
/// The desktop webview drops an eval, result included, when JavaScript
/// finalizes that object, which it can do the moment the script has returned:
/// if that notice reaches Rust before the awaiting task has run, the result is
/// lost ("eval has already ran").
fn keep_result(js: &str) -> String {
    format!(
        "const evals = (window.__awaitedEvals ??= new Set());\nevals.add(dioxus);\ntry {{\n{js}\n}} finally {{\n  \
         setTimeout(() => evals.delete(dioxus), 5000);\n}}"
    )
}

/// Run `js` (the body of an async function) and return what it returns.
/// `None` if there is no JS runtime (as in tests) or the result didn't arrive.
pub async fn eval<T: serde::de::DeserializeOwned>(js: &str) -> Option<T> {
    match dioxus::prelude::document::eval(&keep_result(js)).join::<T>().await {
        Ok(value) => Some(value),
        Err(e) => {
            dioxus::logger::tracing::debug!("script result unavailable: {e}");
            None
        }
    }
}

/// Encode `s` as a JavaScript string literal (JSON is a subset of JS).
pub fn js_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

/// "a, b, c (+N more)": at most `shown` items of a list, for banners.
pub fn short_list<T: ToString>(items: &[T], shown: usize) -> String {
    let mut s = items.iter().take(shown).map(T::to_string).collect::<Vec<_>>().join(", ");
    if items.len() > shown {
        s.push_str(&format!(" (+{} more)", items.len() - shown));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn awaited_scripts_keep_their_channel() {
        let js = keep_result("return 1;");
        assert!(js.contains("evals.add(dioxus);") && js.contains("try {\nreturn 1;\n} finally {"), "{js}");
        assert!(js.contains("setTimeout(() => evals.delete(dioxus), 5000)"), "{js}");
    }

    #[test]
    fn js_string_escapes() {
        assert_eq!(js_string("a\"b\n</script>"), r#""a\"b\n</script>""#);
    }

    #[test]
    fn short_list_truncates() {
        let keys: Vec<String> = (1..=14).map(|n| format!("2:{n}")).collect();
        assert_eq!(short_list(&keys[..2], 12), "2:1, 2:2");
        assert!(short_list(&keys, 12).ends_with("2:12 (+2 more)"));
        assert_eq!(short_list(&[3u16, 4], 1), "3 (+1 more)");
    }
}
