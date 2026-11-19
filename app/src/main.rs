//! Quran.com reader in Dioxus. One codebase, two targets: a desktop app
//! (`cargo run`, `dx serve --platform desktop`) and a web app
//! (`dx serve --platform web`).
//!
//! - `route`, `layout`, `util`: routes, app shell, small helpers.
//! - `reader/*`: home, chapter / range / page / juz views, the mushaf renderer.
//! - `settings/*`: settings state, persistence and page; translations and footnotes.
//! - `search/*`: search box, suggestions, results page.

use dioxus::prelude::*;

mod layout;
mod reader;
mod route;
mod search;
mod settings;
mod util;

#[cfg(test)]
mod mount_tests;

pub use route::Route;

const TITLE: &str = "The Noble Quran";

/// All styles, embedded so the app is styled however it was built (`dx` or plain `cargo`).
const CSS: &str = concat!(
    include_str!("../assets/main.css"),
    include_str!("../assets/reader.css"),
    include_str!("../assets/settings.css"),
    include_str!("../assets/search.css"),
);

fn main() {
    #[cfg(all(feature = "desktop", not(target_arch = "wasm32")))]
    {
        use dioxus::desktop::{Config, LogicalSize, WindowBuilder};
        let window = WindowBuilder::new().with_title(TITLE).with_inner_size(LogicalSize::new(1100.0, 860.0));
        // No in-window menu bar: the app has no menu commands of its own.
        let config = Config::new()
            .with_window(window)
            .with_menu(None)
            .with_on_window(|window, _| native_frame(&window));
        dioxus::LaunchBuilder::desktop().with_cfg(config).launch(App);
    }
    #[cfg(not(all(feature = "desktop", not(target_arch = "wasm32"))))]
    dioxus::launch(App);
}

/// Let the desktop draw the window's frame (title bar, buttons, borders).
///
/// On Wayland the window library always gives the window a GTK title bar of its
/// own, which takes the place of the compositor's: under KDE the window then
/// looks like a GNOME one. Without that title bar GTK asks the compositor for
/// its frame, and draws its own plain one only where the compositor has none.
#[cfg(all(feature = "desktop", target_os = "linux"))]
fn native_frame(window: &dioxus::desktop::tao::window::Window) {
    use dioxus::desktop::tao::platform::unix::WindowExtUnix;
    use gtk::prelude::GtkWindowExt;
    window.gtk_window().set_titlebar(None::<&gtk::Widget>);
}

#[cfg(all(feature = "desktop", not(target_arch = "wasm32"), not(target_os = "linux")))]
fn native_frame(_window: &dioxus::desktop::tao::window::Window) {}

#[component]
fn App() -> Element {
    // Shared API client and user settings, available to every component.
    use_context_provider(quran_api::Client::default);
    settings::provide_settings();
    // The desktop app has no URL to reload: it starts where it was last closed.
    #[cfg(not(target_arch = "wasm32"))]
    use_hook(|| {
        if let Some(location) = settings::storage::load_location() {
            let history = dioxus::history::MemoryHistory::with_initial_path(location);
            dioxus::history::provide_history_context(std::rc::Rc::new(history));
        }
    });

    rsx! {
        document::Title { "{TITLE}" }
        document::Meta { name: "viewport", content: "width=device-width, initial-scale=1" }
        document::Style { {CSS} }
        settings::FontLoader {}
        Router::<Route> {}
    }
}

/// The shared API client.
pub fn use_api() -> quran_api::Client {
    use_context::<quran_api::Client>()
}
