//! Small cross-platform helpers.

/// Async sleep that works on both web (JS timers) and desktop (tokio).
#[allow(dead_code)]
pub async fn sleep_ms(ms: u32) {
    #[cfg(feature = "web")]
    gloo_timers::future::TimeoutFuture::new(ms).await;
    #[cfg(all(feature = "desktop", not(feature = "web")))]
    tokio::time::sleep(std::time::Duration::from_millis(ms as u64)).await;
}
