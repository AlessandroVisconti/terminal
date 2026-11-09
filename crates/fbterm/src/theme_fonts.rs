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
//! fonts are scanned again on the blocking pool. Of each missing suggested family the scan
//! finds, only the files of the faces a terminal draws with (the closest regular, bold, italic
//! and bold italic) are added to the renderer, memory-mapped by its font database like the
//! fonts it read at startup (`db_mut().load_font_file`, which also clears cosmic-text's match
//! cache). Every terminal view then measures its cell again (`fbterm_term::fonts_loaded`), so
//! a pane already naming that family stops using the fallback's metrics. Everything else the
//! system has is left alone, and the system fonts are never loaded a second time. Never per
//! frame, never in the background. fbterm never downloads fonts.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use fbterm_config::schemes::{Scheme, theme_font};
use iced::Task;

use crate::app::{App, Message};

/// A font file of a missing suggested family found by a scan, for the renderer to map.
#[derive(Clone, Debug)]
pub(crate) struct NewFace {
    pub family: String,
    pub path: PathBuf,
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

/// The faces a terminal draws with: (weight, italic) for regular, bold, italic, bold italic.
const TERMINAL_FACES: [(u16, bool); 4] = [(400, false), (700, false), (400, true), (700, true)];

/// Of `faces` (family, weight, italic, key) found by a scan, the keys to load: for each
/// suggested family not `known` to the renderer yet, the closest face to each of
/// [`TERMINAL_FACES`] (the same slant when the family has it, then the nearest weight, the
/// first on a tie). Each key once, by family in scan order.
pub(crate) fn pick_new_faces<'a, K: Clone + PartialEq>(
    faces: impl IntoIterator<Item = (&'a str, u16, bool, K)>,
    known: &HashSet<String>,
    suggested: &HashSet<String>,
) -> Vec<K> {
    /// One family's faces: (weight, italic, key).
    type Faces<K> = Vec<(u16, bool, K)>;
    let mut families: Vec<(&str, Faces<K>)> = Vec::new();
    for (family, weight, italic, key) in faces {
        if !suggested.contains(family) || known.contains(family) {
            continue;
        }
        match families.iter_mut().find(|(f, _)| *f == family) {
            Some((_, list)) => list.push((weight, italic, key)),
            None => families.push((family, vec![(weight, italic, key)])),
        }
    }
    let mut out: Vec<K> = Vec::new();
    for (_, list) in &families {
        for (weight, italic) in TERMINAL_FACES {
            let slanted = list.iter().any(|f| f.1 == italic);
            let best = list
                .iter()
                .filter(|f| !slanted || f.1 == italic)
                .min_by_key(|f| f.0.abs_diff(weight));
            if let Some((_, _, key)) = best
                && !out.contains(key)
            {
                out.push(key.clone());
            }
        }
    }
    out
}

/// `host` as `Download` shows it: whole, or when longer than `max` characters its end (the
/// part that names who owns it) after an ellipsis, cut at a label boundary when one is in
/// reach. Never the start, which a long crafted host could fill with a familiar name.
pub(crate) fn host_label(host: &str, max: usize) -> String {
    let count = host.chars().count();
    if count <= max {
        return host.to_owned();
    }
    let keep = max.saturating_sub(1);
    let tail: String = host.chars().skip(count - keep).collect();
    let tail = match tail.split_once('.') {
        Some((_, rest)) if !rest.is_empty() => rest.to_owned(),
        _ => tail,
    };
    format!("\u{2026}{tail}")
}

/// The host of an `https://` link, as `Download` shows it: no user, port, path or query.
pub(crate) fn url_host(url: &str) -> Option<String> {
    if !fbterm_config::schemes::is_plain_https_url(url) {
        return None;
    }
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

/// Scan the system fonts afresh (blocking) and pick the files of the terminal faces of
/// `missing` families the renderer does not know yet.
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
    let faces: Vec<(String, u16, bool, PathBuf)> = db
        .faces()
        .filter_map(|f| {
            let family = f.families.first()?.0.clone();
            let path = match &f.source {
                fontdb::Source::File(p) | fontdb::Source::SharedFile(p, _) => p.clone(),
                fontdb::Source::Binary(_) => return None,
            };
            Some((family, f.weight.0, f.style != fontdb::Style::Normal, path))
        })
        .collect();
    let picked = pick_new_faces(
        faces
            .iter()
            .map(|(f, w, i, p)| (f.as_str(), *w, *i, (f.clone(), p.clone()))),
        &known,
        missing,
    );
    let found: Vec<NewFace> = picked
        .into_iter()
        .map(|(family, path)| NewFace { family, path })
        .collect();
    tracing::info!(
        faces = db.len(),
        found = found.len(),
        ms = start.elapsed().as_millis(),
        "system fonts scanned for suggested fonts"
    );
    found
}

/// Add `faces` to the renderer, each file once, memory-mapped by its font database (as the
/// fonts it read at startup are), so they cost almost no memory until drawn.
fn load_into_renderer(faces: &[NewFace]) {
    let mut system = renderer();
    let mut seen: Vec<&PathBuf> = Vec::new();
    for face in faces {
        if seen.contains(&&face.path) {
            continue;
        }
        seen.push(&face.path);
        if let Err(e) = system.raw().db_mut().load_font_file(&face.path) {
            tracing::warn!(error = %e, family = %face.family, "font file not loaded");
        }
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
        if self.missing_fonts().is_empty() {
            return Task::none();
        }
        if self.fonts_scanning {
            // The running scan may not cover what is missing now (a catalog arrived): one
            // more when it ends.
            self.fonts_rescan = true;
            return Task::none();
        }
        self.fonts_scanning = true;
        let job = self.font_scan_job();
        Task::perform(
            async move { tokio::task::spawn_blocking(job).await.unwrap_or_default() },
            |faces| Message::FontsScanned(Arc::new(faces)),
        )
    }

    /// A scan finished: add what it found to the renderer, make every terminal view measure
    /// again, read the list again, and run the follow-up scan asked for meanwhile, if any.
    pub(crate) fn fonts_scanned(&mut self, faces: &[NewFace]) -> Task<Message> {
        self.fonts_scanning = false;
        if !faces.is_empty() {
            let families: HashSet<&str> = faces.iter().map(|f| f.family.as_str()).collect();
            tracing::info!(?families, files = faces.len(), "suggested fonts loaded");
            (self.font_io.load)(faces);
            fbterm_term::fonts_loaded();
        }
        self.refresh_installed_fonts();
        if std::mem::take(&mut self.fonts_rescan) {
            return self.scan_fonts();
        }
        Task::none()
    }
}
