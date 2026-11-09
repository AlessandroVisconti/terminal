//! Suggested fonts for themes in the app: which font a pane is drawn with when
//! `use_theme_fonts` is on, and the installed-font list that decision reads.
//!
//! A pane's font resolves as: its scheme's own `font_family` if it sets one, else (with
//! `use_theme_fonts` on) the scheme's suggested font if it is installed, else the global
//! `font_family` (the rule is `fbterm_config::schemes::theme_font`). Nothing is written to
//! scheme files or to `font_family`, so turning the setting off restores the global font.
//!
//! "Installed" means listed by the font database the Settings font picker reads
//! ([`crate::settings_window::installed_monospace`], the bundled face always counting). The
//! list is read into [`App::installed_fonts`] when the setting turns on and each time the Color
//! schemes window shows a scheme's details, never per frame. fbterm never downloads fonts.

use std::sync::Arc;

use fbterm_config::schemes::{Scheme, theme_font};

use crate::app::App;

/// Reads the installed monospace families (the system font database; a fake in tests).
pub(crate) type FontList = Arc<dyn Fn() -> Vec<String> + Send + Sync>;

/// The font database the Settings font picker reads; in unit tests, an empty list (only the
/// bundled face counts), so no test depends on the machine's fonts.
pub(crate) fn system_font_list() -> FontList {
    if cfg!(test) {
        Arc::new(Vec::new)
    } else {
        Arc::new(crate::settings_window::installed_monospace)
    }
}

/// Whether `family` is installed by `installed` (the family names exactly as the font
/// database reports them); the bundled face always is.
pub(crate) fn is_installed(installed: &[String], family: &str) -> bool {
    family == crate::settings_window::BUNDLED_FONT || installed.iter().any(|f| f == family)
}

impl App {
    /// Read the installed families again (see the module docs for when).
    pub(crate) fn refresh_installed_fonts(&mut self) {
        let mut list = (self.font_list)();
        list.sort();
        list.dedup();
        if list != self.installed_fonts {
            tracing::debug!(families = list.len(), "installed fonts read");
            self.installed_fonts = list;
            self.warm_ligature_sets();
        }
    }

    /// The family a pane using `scheme` is drawn with when it is not the global font.
    pub(crate) fn scheme_font<'a>(&self, scheme: &'a Scheme) -> Option<&'a str> {
        theme_font(scheme, self.settings.use_theme_fonts, |f| {
            is_installed(&self.installed_fonts, f)
        })
    }
}
