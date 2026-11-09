//! Suggested fonts for themes in the app: which font a pane is drawn with when
//! `use_theme_fonts` is on, the installed-font list that decision reads, and loading a
//! suggested font installed while fbterm runs.
//!
//! A pane's font resolves as: its scheme's own `font_family` if it sets one, else (with
//! `use_theme_fonts` on) the scheme's suggested font if it is installed, else the global
//! `font_family` (the rule is `fbterm_config::schemes::theme_font`). The answer is cached per
//! scheme ([`App::scheme_fonts`]) whenever the schemes, the installed list or the settings
//! change, so drawing a pane looks it up instead of resolving it. Nothing is written to scheme
//! files or to `font_family`, so turning the setting off restores the global font.
//!
//! "Installed" means known to the renderer's font database, the one the Settings font picker
//! reads: monospaced faces, plus faces of any suggested family whether or not they carry the
//! fixed-pitch flag (suggestions are monospace by the catalog's rule), the bundled face always
//! counting. That database reads the system fonts once, at startup. So on a user action
//! (showing the Color schemes window or its gallery, a catalog arriving, turning
//! `use_theme_fonts` on), and only when some suggested family is still missing, the system
//! fonts are scanned again on the blocking pool; the faces of missing suggested families that
//! the scan finds are read and added to the renderer one by one (`FontSystem::load_font`,
//! which also clears its match cache). Everything else the system has is left alone, and the
//! system fonts are never loaded a second time. Never per frame, never in the background.
//! fbterm never downloads fonts.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use fbterm_config::schemes::{Scheme, theme_font};
use iced::Task;

use crate::app::{App, Message};

/// The font files of one family found by a scan, read and ready for the renderer.
#[derive(Clone)]
pub(crate) struct NewFace {
    pub family: String,
    pub data: Vec<u8>,
}

impl std::fmt::Debug for NewFace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NewFace")
            .field("family", &self.family)
            .field("bytes", &self.data.len())
            .finish()
    }
}

/// The renderer's installed families, given the suggested ones (see the module docs).
pub(crate) type ListFn = dyn Fn(&HashSet<String>) -> Vec<String> + Send + Sync;
/// A blocking scan of the system fonts for the faces of these missing families.
pub(crate) type ScanFn = dyn Fn(&HashSet<String>) -> Vec<NewFace> + Send + Sync;
/// Adds faces to the renderer.
pub(crate) type LoadFn = dyn Fn(&[NewFace]) + Send + Sync;

/// How the app reaches fonts; replaced in tests, which never read the machine's fonts.
#[derive(Clone)]
pub(crate) struct FontIo {
    pub list: Arc<ListFn>,
    pub scan: Arc<ScanFn>,
    pub load: Arc<LoadFn>,
}

impl Default for FontIo {
    fn default() -> Self {
        if cfg!(test) {
            return Self {
                list: Arc::new(|_| Vec::new()),
                scan: Arc::new(|_| Vec::new()),
                load: Arc::new(|_| {}),
            };
        }
        Self {
            list: Arc::new(renderer_families),
            scan: Arc::new(scan_system),
            load: Arc::new(load_into_renderer),
        }
    }
}

/// The families to count as installed among `faces` (family, fixed-pitch flag): every
/// monospaced one, and every suggested one with or without the flag. Sorted, each once.
pub(crate) fn installed_families<'a>(
    faces: impl IntoIterator<Item = (&'a str, bool)>,
    suggested: &HashSet<String>,
) -> Vec<String> {
    let mut out: Vec<String> = faces
        .into_iter()
        .filter(|(family, mono)| *mono || suggested.contains(*family))
        .map(|(family, _)| family.to_owned())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// Of `faces` (family, key) found by a scan, the keys of faces whose family is suggested and
/// not `known` to the renderer yet, each once, in scan order.
pub(crate) fn pick_new_faces<'a, K: Clone + PartialEq>(
    faces: impl IntoIterator<Item = (&'a str, K)>,
    known: &HashSet<String>,
    suggested: &HashSet<String>,
) -> Vec<K> {
    let mut out: Vec<K> = Vec::new();
    for (family, key) in faces {
        if suggested.contains(family) && !known.contains(family) && !out.contains(&key) {
            out.push(key);
        }
    }
    out
}

/// The host of an `https://` link, as `Download` shows it: no user, port, path or query.
pub(crate) fn url_host(url: &str) -> Option<String> {
    let rest = url.strip_prefix("https://")?;
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let host_port = authority.rsplit('@').next().unwrap_or("");
    let host = if let Some(v6) = host_port.strip_prefix('[') {
        format!("[{}]", v6.split(']').next().unwrap_or(""))
    } else {
        host_port.split(':').next().unwrap_or("").to_owned()
    };
    (!host.is_empty() && host != "[]").then_some(host)
}

/// Whether `family` is installed by `installed` (the family names exactly as the font
/// database reports them, sorted as [`App::refresh_installed_fonts`] keeps them); the
/// bundled face always is.
pub(crate) fn is_installed(installed: &[String], family: &str) -> bool {
    family == crate::settings_window::BUNDLED_FONT
        || installed
            .binary_search_by(|f| f.as_str().cmp(family))
            .is_ok()
}

fn renderer() -> std::sync::RwLockWriteGuard<'static, iced::advanced::graphics::text::FontSystem> {
    iced::advanced::graphics::text::font_system()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The renderer's families that count as installed.
fn renderer_families(suggested: &HashSet<String>) -> Vec<String> {
    let mut system = renderer();
    let faces: Vec<(String, bool)> = system
        .raw()
        .db()
        .faces()
        .filter_map(|f| f.families.first().map(|(n, _)| (n.clone(), f.monospaced)))
        .collect();
    drop(system);
    installed_families(faces.iter().map(|(n, m)| (n.as_str(), *m)), suggested)
}

/// Scan the system fonts afresh (blocking) and read the files of the faces of `missing`
/// families the renderer does not know yet.
fn scan_system(missing: &HashSet<String>) -> Vec<NewFace> {
    use iced::advanced::graphics::text::cosmic_text::fontdb;
    let start = std::time::Instant::now();
    let known: HashSet<String> = renderer()
        .raw()
        .db()
        .faces()
        .filter_map(|f| f.families.first().map(|(n, _)| n.clone()))
        .collect();
    let mut db = fontdb::Database::new();
    db.load_system_fonts();
    let faces: Vec<(String, PathBuf)> = db
        .faces()
        .filter_map(|f| {
            let family = f.families.first()?.0.clone();
            let path = match &f.source {
                fontdb::Source::File(p) | fontdb::Source::SharedFile(p, _) => p.clone(),
                fontdb::Source::Binary(_) => return None,
            };
            Some((family, path))
        })
        .collect();
    let picked = pick_new_faces(
        faces
            .iter()
            .map(|(f, p)| (f.as_str(), (f.clone(), p.clone()))),
        &known,
        missing,
    );
    let found: Vec<NewFace> = picked
        .into_iter()
        .filter_map(|(family, path)| match std::fs::read(&path) {
            Ok(data) => Some(NewFace { family, data }),
            Err(e) => {
                tracing::warn!(error = %e, %family, "font file not read");
                None
            }
        })
        .collect();
    tracing::info!(
        faces = db.len(),
        found = found.len(),
        ms = start.elapsed().as_millis(),
        "system fonts scanned for suggested fonts"
    );
    found
}

/// Add `faces` to the renderer, each file once.
fn load_into_renderer(faces: &[NewFace]) {
    let mut system = renderer();
    for face in faces {
        system.load_font(std::borrow::Cow::Owned(face.data.clone()));
    }
}

impl App {
    /// Every family a scheme or the loaded catalog suggests.
    pub(crate) fn suggested_families(&self) -> HashSet<String> {
        let catalog = self
            .gallery_catalog
            .iter()
            .flat_map(|c| c.themes.iter().map(|t| &t.scheme));
        self.schemes
            .all()
            .iter()
            .chain(catalog)
            .filter_map(|s| s.suggested_font.as_ref().map(|f| f.family.clone()))
            .collect()
    }

    /// Read the renderer's installed families again (on a user action, see the module docs)
    /// and resolve each scheme's font from them.
    pub(crate) fn refresh_installed_fonts(&mut self) {
        let mut list = (self.font_io.list)(&self.suggested_families());
        list.sort();
        list.dedup();
        if list != self.installed_fonts {
            tracing::debug!(families = list.len(), "installed fonts read");
            self.installed_fonts = list;
            self.warm_ligature_sets();
        }
        if let Some((_, state)) = &mut self.schemes_win {
            state.set_installed(self.installed_fonts.clone());
        }
        self.rebuild_scheme_fonts();
    }

    /// Resolve each scheme's font once, for drawing (after the schemes, the installed list or
    /// the settings change).
    pub(crate) fn rebuild_scheme_fonts(&mut self) {
        let fonts: HashMap<String, Option<&'static str>> = self
            .schemes
            .all()
            .iter()
            .map(|s| {
                let font = theme_font(s, self.settings.use_theme_fonts, |f| {
                    is_installed(&self.installed_fonts, f)
                })
                .map(crate::settings_window::intern);
                (s.id.clone(), font)
            })
            .collect();
        self.scheme_fonts = fonts;
    }

    /// The family a pane using `scheme` is drawn with when it is not the global font: the
    /// cached answer, resolved again only for a scheme the cache does not hold.
    pub(crate) fn scheme_font<'a>(&self, scheme: &'a Scheme) -> Option<&'a str> {
        if let Some(font) = self.scheme_fonts.get(&scheme.id) {
            return *font;
        }
        theme_font(scheme, self.settings.use_theme_fonts, |f| {
            is_installed(&self.installed_fonts, f)
        })
    }

    /// The suggested families not installed now.
    fn missing_fonts(&self) -> HashSet<String> {
        self.suggested_families()
            .into_iter()
            .filter(|f| !is_installed(&self.installed_fonts, f))
            .collect()
    }

    /// The scan as the blocking pool runs it.
    pub(crate) fn font_scan_job(&self) -> impl FnOnce() -> Vec<NewFace> + Send + 'static + use<> {
        let scan = self.font_io.scan.clone();
        let missing = self.missing_fonts();
        move || scan(&missing)
    }

    /// On a user action: scan the system fonts on the blocking pool for suggested families
    /// that are still missing (none missing, or a scan under way: nothing to do).
    pub(crate) fn scan_fonts(&mut self) -> Task<Message> {
        if self.fonts_scanning || self.missing_fonts().is_empty() {
            return Task::none();
        }
        self.fonts_scanning = true;
        let job = self.font_scan_job();
        Task::perform(
            async move { tokio::task::spawn_blocking(job).await.unwrap_or_default() },
            |faces| Message::FontsScanned(Arc::new(faces)),
        )
    }

    /// A scan finished: add what it found to the renderer, then read the list again.
    pub(crate) fn fonts_scanned(&mut self, faces: &[NewFace]) {
        self.fonts_scanning = false;
        if !faces.is_empty() {
            let families: HashSet<&str> = faces.iter().map(|f| f.family.as_str()).collect();
            tracing::info!(?families, files = faces.len(), "suggested fonts loaded");
            (self.font_io.load)(faces);
        }
        self.refresh_installed_fonts();
    }
}
