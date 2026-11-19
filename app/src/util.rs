//! Small cross-platform helpers.

/// Async sleep that works on both web (JS timers) and desktop (tokio).
/// Chosen by build target: web-sys/gloo calls panic on native targets even if the
/// `web` feature happens to be enabled in a desktop build.
#[allow(dead_code)]
pub async fn sleep_ms(ms: u32) {
    #[cfg(all(feature = "web", target_arch = "wasm32"))]
    gloo_timers::future::TimeoutFuture::new(ms).await;
    #[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
    tokio::time::sleep(std::time::Duration::from_millis(ms as u64)).await;
    let _ = ms;
}
