//! URL routes. Mirrors quran.com URLs so links keep working:
//! `/`, `/2`, `/2/255`, `/2/1-5`, `/2:255` (handled inside `ChapterPage`),
//! `/page/3`, `/juz/30`, `/search?q=...`, `/settings`.

use dioxus::prelude::*;

use crate::layout::{NotFound, Shell};
use crate::reader::{ChapterPage, ChapterRange, Home, JuzPage, MushafPage};
use crate::search::SearchPage;
use crate::settings::SettingsPage;

#[derive(Routable, Clone, PartialEq, Debug)]
#[rustfmt::skip]
pub enum Route {
    #[layout(Shell)]
        #[route("/")]
        Home {},
        #[route("/search?:q")]
        SearchPage { q: String },
        #[route("/settings")]
        SettingsPage {},
        #[route("/page/:page")]
        MushafPage { page: u16 },
        #[route("/juz/:juz")]
        JuzPage { juz: u8 },
        /// `chapter` is a number ("2") or a legacy key form ("2:255", "2:1-5", "2-1-5").
        #[route("/:chapter")]
        ChapterPage { chapter: String },
        /// `range` is "255" or "1-5".
        #[route("/:chapter/:range")]
        ChapterRange { chapter: String, range: String },
        #[route("/:..segments")]
        NotFound { segments: Vec<String> },
}
