//! Suggested fonts for themes: how a pane's font resolves with `use_theme_fonts`, the
//! suggestion line of the Color schemes tab and its gallery, and the Settings row, against
//! a temporary config directory and a fake installed-font list. Nothing here opens a browser.

use std::sync::Arc;

use fbterm_config::schemes::{Scheme, SuggestedFont, read_file};
use iced::Font;
use parking_lot::Mutex;

use crate::app::{App, Message};
use crate::restore_tests::profile;
use crate::schemes_tests::{app_with, style, user_scheme, with_scheme};
use crate::schemes_window::gallery::{GalleryIo, GalleryLoad};
use crate::schemes_window::{FontState, SchemesMsg, SchemesState};
use crate::settings_window::SettingsMsg;
use crate::theme_fonts::{
    FontIo, NewFace, host_label, installed_families, pick_new_faces, url_host,
};
use crate::watch::Changed;

const SETTINGS_CHANGED: Changed = Changed {
    profiles: false,
    settings: true,
    schemes: false,
};

fn font(family: &str, url: &str) -> SuggestedFont {
    SuggestedFont {
        family: family.into(),
        url: url.into(),
        license: "OFL-1.1".into(),
    }
}

/// A user scheme suggesting `family`.
fn suggesting(name: &str, bg: u32, family: &str) -> Scheme {
    let mut s = user_scheme(name, bg);
    s.suggested_font = Some(font(family, "https://fonts.example/sample"));
    s
}

/// A fake font setup: what the renderer knows, what a system scan would find, how often the
/// renderer's list was read and which families were added to it.
#[derive(Clone, Default)]
struct FakeFonts {
    renderer: Arc<Mutex<Vec<String>>>,
    system: Arc<Mutex<Vec<String>>>,
    lists: Arc<Mutex<usize>>,
    scans: Arc<Mutex<usize>>,
    loaded: Arc<Mutex<Vec<String>>>,
    /// How many times the app told the terminal views that fonts were loaded.
    notified: Arc<Mutex<usize>>,
}

fn strings(families: &[&str]) -> Vec<String> {
    families.iter().map(|f| (*f).to_owned()).collect()
}

/// Replace the app's font access: the renderer knows `renderer`; a scan of the system finds
/// `system` (so families there but not in the renderer are "installed while fbterm runs").
fn fake_fonts(app: &mut App, renderer: &[&str], system: &[&str]) -> FakeFonts {
    let fake = FakeFonts::default();
    *fake.renderer.lock() = strings(renderer);
    *fake.system.lock() = strings(system);
    let (f1, f2, f3, f4) = (fake.clone(), fake.clone(), fake.clone(), fake.clone());
    app.font_io = FontIo {
        list: Arc::new(move |_suggested| {
            *f1.lists.lock() += 1;
            f1.renderer.lock().clone()
        }),
        scan: Arc::new(move |suggested| {
            *f2.scans.lock() += 1;
            let known = f2.renderer.lock().clone();
            f2.system
                .lock()
                .iter()
                .filter(|f| suggested.contains(*f) && !known.contains(f))
                .map(|f| NewFace {
                    family: f.clone(),
                    path: std::path::PathBuf::new(),
                })
                .collect()
        }),
        load: Arc::new(move |faces: &[NewFace]| {
            for face in faces {
                f3.renderer.lock().push(face.family.clone());
                f3.loaded.lock().push(face.family.clone());
            }
        }),
        loaded: Arc::new(move || *f4.notified.lock() += 1),
    };
    fake
}

/// Make `families` the installed fonts, as the font database would report them.
fn installed(app: &mut App, families: &[&str]) {
    fake_fonts(app, families, families);
    app.refresh_installed_fonts();
}

/// Run the system scan the way the blocking task does and hand its result to the app.
fn finish_scan(app: &mut App) {
    let faces = app.font_scan_job()();
    let _ = app.update(Message::FontsScanned(Arc::new(faces)));
}

/// Replace the link opener with one that records what it was asked to open.
fn record_opens(app: &mut App) -> Arc<Mutex<Vec<String>>> {
    let opened = Arc::new(Mutex::new(Vec::new()));
    let seen = opened.clone();
    app.gallery_io = GalleryIo {
        fetch: Arc::new(|_, _| Err("no network in tests".into())),
        open: Arc::new(move |url| seen.lock().push(url.to_owned())),
    };
    opened
}

fn send(app: &mut App, m: SchemesMsg) {
    let _ = app.update(Message::Schemes(m));
}

fn win(app: &App) -> &SchemesState {
    &app.schemes_win.as_ref().expect("schemes window open").1
}

fn family(f: &str) -> Font {
    Font::with_name(crate::settings_window::intern(f))
}

#[test]
fn installed_suggestion_gives_its_panes_that_font() {
    let ocean = suggesting("Ocean", 0x102030, "Sample Mono");
    let plain = user_scheme("Plain", 0x203040);
    let a = with_scheme(&profile("a"), &ocean.id);
    let b = with_scheme(&profile("b"), &plain.id);
    let (mut app, _, wid, ids) = app_with(
        &[a, b],
        "use_theme_fonts = true\nfont_family = \"Geist Mono\"\n",
        &[ocean, plain],
    );
    installed(&mut app, &["Sample Mono"]);
    assert_eq!(style(&app, wid, ids[0]).font, family("Sample Mono"));
    // A scheme without a suggestion keeps the global font.
    assert_eq!(style(&app, wid, ids[1]).font, family("Geist Mono"));
}

#[test]
fn scheme_own_font_still_wins() {
    let mut ocean = suggesting("Ocean", 0x102030, "Sample Mono");
    ocean.font_family = Some("Own Mono".into());
    let a = with_scheme(&profile("a"), &ocean.id);
    let (mut app, _, wid, ids) = app_with(&[a], "use_theme_fonts = true\n", &[ocean]);
    installed(&mut app, &["Sample Mono", "Own Mono"]);
    assert_eq!(style(&app, wid, ids[0]).font, family("Own Mono"));
}

#[test]
fn missing_suggested_font_falls_back_to_the_global_font() {
    let ocean = suggesting("Ocean", 0x102030, "Sample Mono");
    let a = with_scheme(&profile("a"), &ocean.id);
    let (mut app, _, wid, ids) = app_with(
        &[a],
        "use_theme_fonts = true\nfont_family = \"Geist Mono\"\n",
        &[ocean],
    );
    installed(&mut app, &["Another Mono"]);
    assert_eq!(style(&app, wid, ids[0]).font, family("Geist Mono"));
}

#[test]
fn turning_theme_fonts_off_restores_the_global_font() {
    let ocean = suggesting("Ocean", 0x102030, "Sample Mono");
    let a = with_scheme(&profile("a"), &ocean.id);
    let (mut app, paths, wid, ids) = app_with(
        &[a],
        "use_theme_fonts = true\nfont_family = \"Geist Mono\"\n",
        &[ocean],
    );
    installed(&mut app, &["Sample Mono"]);
    assert_eq!(style(&app, wid, ids[0]).font, family("Sample Mono"));
    std::fs::write(
        paths.settings(),
        "use_theme_fonts = false\nfont_family = \"Geist Mono\"\n",
    )
    .unwrap();
    let _ = app.update(Message::ConfigChanged(SETTINGS_CHANGED));
    assert!(!app.settings.use_theme_fonts);
    assert_eq!(style(&app, wid, ids[0]).font, family("Geist Mono"));
    // And on again, read from the file: the suggestion is back, nothing was written elsewhere.
    std::fs::write(
        paths.settings(),
        "use_theme_fonts = true\nfont_family = \"Geist Mono\"\n",
    )
    .unwrap();
    let _ = app.update(Message::ConfigChanged(SETTINGS_CHANGED));
    assert_eq!(style(&app, wid, ids[0]).font, family("Sample Mono"));
    assert_eq!(app.settings.font_family, "Geist Mono");
}

/// Switching the global theme switches the font of the panes that follow it, and only theirs;
/// no resize of its own is queued (the view measures the new cell and reports the grid once,
/// as for a scheme font).
#[test]
fn theme_change_re_resolves_the_font_of_affected_panes() {
    let ocean = suggesting("Ocean", 0x102030, "Sample Mono");
    let dune = suggesting("Dune", 0x302010, "Other Mono");
    let a = profile("a");
    let b = with_scheme(&profile("b"), "harbor");
    let (mut app, paths, wid, ids) = app_with(
        &[a, b],
        &format!(
            "use_theme_fonts = true\nfont_family = \"Geist Mono\"\ncolor_scheme = \"{}\"\n",
            ocean.id
        ),
        &[ocean, dune.clone()],
    );
    installed(&mut app, &["Sample Mono", "Other Mono"]);
    let pinned = style(&app, wid, ids[1]).font;
    assert_eq!(style(&app, wid, ids[0]).font, family("Sample Mono"));
    std::fs::write(
        paths.settings(),
        format!(
            "use_theme_fonts = true\nfont_family = \"Geist Mono\"\ncolor_scheme = \"{}\"\n",
            dune.id
        ),
    )
    .unwrap();
    let _ = app.update(Message::ConfigChanged(SETTINGS_CHANGED));
    assert_eq!(style(&app, wid, ids[0]).font, family("Other Mono"));
    assert_eq!(
        style(&app, wid, ids[1]).font,
        pinned,
        "own scheme untouched"
    );
    assert!(app.pending_resizes.is_empty());
}

#[test]
fn checkbox_and_settings_row_write_the_same_setting() {
    let (mut app, paths, _, _) = app_with(&[profile("a")], "", &[]);
    send(&mut app, SchemesMsg::Open);
    send(&mut app, SchemesMsg::UseThemeFonts(true));
    assert!(app.settings.use_theme_fonts);
    assert!(win(&app).use_theme_fonts);
    let file = std::fs::read_to_string(paths.settings()).unwrap();
    assert!(file.contains("use_theme_fonts = true"), "{file}");

    // The Settings row writes the same key, and the Color schemes tab follows.
    let _ = app.update(Message::Settings(SettingsMsg::Open));
    let _ = app.update(Message::Settings(SettingsMsg::UseThemeFonts(false)));
    assert!(!app.settings.use_theme_fonts);
    let file = std::fs::read_to_string(paths.settings()).unwrap();
    assert!(file.contains("use_theme_fonts = false"), "{file}");
    assert!(!file.contains("use_theme_fonts = true"), "{file}");
    assert!(!win(&app).use_theme_fonts);
    // The window's checkbox shows in the Settings tab too.
    send(&mut app, SchemesMsg::UseThemeFonts(true));
    assert!(app.settings_win.as_ref().unwrap().1.values.use_theme_fonts);
}

#[test]
fn line_shows_only_for_schemes_with_a_suggestion() {
    let ocean = suggesting("Ocean", 0x102030, "Sample Mono");
    let mut plain = user_scheme("Plain", 0x203040);
    plain.suggested_font = None;
    let (mut app, _, _, _) = app_with(&[profile("a")], "", &[ocean.clone(), plain.clone()]);
    installed(&mut app, &[]);
    send(&mut app, SchemesMsg::Open);
    let w = win(&app);
    assert!(w.font_line(w.draft(&ocean.id).unwrap()).is_some());
    assert!(w.font_line(w.draft(&plain.id).unwrap()).is_none());
    // Every built-in has one.
    for id in fbterm_config::schemes::BUILTIN_IDS {
        assert!(w.font_line(w.draft(id).unwrap()).is_some(), "{id}");
    }
}

#[test]
fn use_font_needs_the_font_installed_and_saves_font_family() {
    let ocean = suggesting("Ocean", 0x102030, "Sample Mono");
    let (mut app, paths, _, _) = app_with(
        &[profile("a")],
        "font_family = \"Geist Mono\"\n",
        std::slice::from_ref(&ocean),
    );
    installed(&mut app, &[]);
    send(&mut app, SchemesMsg::Open);
    // The scan finds nothing new: still missing.
    finish_scan(&mut app);
    send(&mut app, SchemesMsg::Select(ocean.id.clone()));
    let line = win(&app)
        .font_line(win(&app).draft(&ocean.id).unwrap())
        .unwrap();
    assert_eq!(line.family, "Sample Mono");
    assert_eq!(line.state, FontState::NotInstalled);
    assert!(!line.can_use);
    // Refused while missing: nothing written.
    send(&mut app, SchemesMsg::UseFont("Sample Mono".into()));
    assert_eq!(app.settings.font_family, "Geist Mono");

    // Installed while fbterm runs: the next showing of the tab scans and loads it.
    let fake = fake_fonts(&mut app, &[], &["Sample Mono"]);
    send(&mut app, SchemesMsg::Open);
    finish_scan(&mut app);
    assert_eq!(*fake.loaded.lock(), strings(&["Sample Mono"]));
    let line = win(&app)
        .font_line(win(&app).draft(&ocean.id).unwrap())
        .unwrap();
    assert_eq!(line.state, FontState::Installed);
    assert!(line.can_use);
    send(&mut app, SchemesMsg::UseFont("Sample Mono".into()));
    assert_eq!(app.settings.font_family, "Sample Mono");
    let file = std::fs::read_to_string(paths.settings()).unwrap();
    assert!(file.contains("font_family = \"Sample Mono\""), "{file}");
    // Now it is the font: `In use`, and nothing left to press.
    let line = win(&app)
        .font_line(win(&app).draft(&ocean.id).unwrap())
        .unwrap();
    assert_eq!(line.state, FontState::InUse);
    assert!(!line.can_use);
    // The scheme file was not touched.
    let on_disk = read_file(&paths.schemes().join(format!("{}.toml", ocean.id))).unwrap();
    assert_eq!(on_disk, ocean);
}

#[test]
fn in_use_follows_the_resolution_rule() {
    let ocean = suggesting("Ocean", 0x102030, "Sample Mono");
    let mut own = suggesting("Own", 0x203040, "Sample Mono");
    own.font_family = Some("Own Mono".into());
    let (mut app, _, _, _) = app_with(
        &[profile("a")],
        "font_family = \"Geist Mono\"\nuse_theme_fonts = true\n",
        &[ocean.clone(), own.clone()],
    );
    installed(&mut app, &["Sample Mono", "Own Mono"]);
    send(&mut app, SchemesMsg::Open);
    let w = win(&app);
    // Theme fonts on and installed: the suggestion is what its panes use.
    let line = w.font_line(w.draft(&ocean.id).unwrap()).unwrap();
    assert_eq!(line.state, FontState::InUse);
    assert_eq!(line.own_font, None);
    // A scheme with its own font says so, and the suggestion is not in use.
    let line = w.font_line(w.draft(&own.id).unwrap()).unwrap();
    assert_eq!(line.state, FontState::Installed);
    assert_eq!(line.own_font.as_deref(), Some("Own Mono"));
    // The default scheme suggests the default font: in use out of the box.
    let line = w.font_line(w.draft("graphite").unwrap()).unwrap();
    assert_eq!(line.family, "Geist Mono");
    assert_eq!(line.state, FontState::InUse);
}

#[test]
fn download_opens_exactly_the_https_url() {
    let ocean = suggesting("Ocean", 0x102030, "Sample Mono");
    let (mut app, _, _, _) = app_with(&[profile("a")], "", std::slice::from_ref(&ocean));
    let opened = record_opens(&mut app);
    send(&mut app, SchemesMsg::Open);
    let line = win(&app)
        .font_line(win(&app).draft(&ocean.id).unwrap())
        .unwrap();
    let url = line.url.clone().expect("an https url to open");
    assert_eq!(url, "https://fonts.example/sample");
    send(&mut app, SchemesMsg::DownloadFont(url));
    assert_eq!(
        *opened.lock(),
        vec!["https://fonts.example/sample".to_owned()]
    );
    // Anything but an https link is refused.
    send(
        &mut app,
        SchemesMsg::DownloadFont("http://fonts.example/".into()),
    );
    send(
        &mut app,
        SchemesMsg::DownloadFont("file:///etc/passwd".into()),
    );
    assert_eq!(opened.lock().len(), 1);
}

/// One gallery theme with a suggestion, as the catalog publishes it.
fn catalog_with_font() -> Vec<u8> {
    let mut text = String::from("format = 1\n[[themes]]\nid = \"x\"\nname = \"Theme x\"\n");
    text.push_str("version = 1\nvariant = \"dark\"\n\n[themes.colors]\n");
    for (k, v) in [
        ("foreground", "#f5f5f7"),
        ("background", "#101010"),
        ("cursor", "#a584ff"),
        ("selection", "#392f56"),
        ("black", "#262629"),
        ("red", "#ed4a49"),
        ("green", "#2bbb71"),
        ("yellow", "#f2a635"),
        ("blue", "#70b3f7"),
        ("magenta", "#a584ff"),
        ("cyan", "#59c5f5"),
        ("white", "#d0d0d9"),
        ("bright_black", "#919199"),
        ("bright_red", "#fd736d"),
        ("bright_green", "#52cd86"),
        ("bright_yellow", "#fac053"),
        ("bright_blue", "#9ccbfa"),
        ("bright_magenta", "#bea8ff"),
        ("bright_cyan", "#8fd6f7"),
        ("bright_white", "#f5f5f7"),
    ] {
        text.push_str(&format!("{k} = \"{v}\"\n"));
    }
    text.push_str(
        "\n[themes.credit]\nproject = \"Project x\"\nauthor = \"Someone\"\n\
         url = \"https://example.org/x\"\nlicense = \"MIT\"\n\
         copyright = \"Copyright (c) 2026 Someone\"\nlicense_file = \"LICENSES/x-MIT.txt\"\n\
         description = \"A theme.\"\nderived = false\n\
         \n[themes.suggested_font]\nfamily = \"Fira Code\"\n\
         url = \"https://github.com/tonsky/FiraCode\"\nlicense = \"OFL-1.1\"\n",
    );
    text.into_bytes()
}

fn open_gallery(app: &mut App) {
    let bytes = catalog_with_font();
    app.gallery_io = GalleryIo {
        fetch: Arc::new(move |_, _| Ok(bytes.clone())),
        open: Arc::new(|_| {}),
    };
    send(app, SchemesMsg::Open);
    send(app, SchemesMsg::ShowGallery(true));
    let result = app.gallery_job()();
    let generation = app.gallery_generation;
    send(app, SchemesMsg::GalleryFetched(generation, result));
    assert!(matches!(win(app).gallery, GalleryLoad::Loaded(_)));
}

#[test]
fn gallery_rows_carry_the_suggestion_and_install_keeps_it() {
    let (mut app, paths, _, _) = app_with(&[profile("a")], "", &[]);
    installed(&mut app, &[]);
    open_gallery(&mut app);
    let w = win(&app);
    let GalleryLoad::Loaded(catalog) = &w.gallery else {
        unreachable!()
    };
    let rows = crate::schemes_window::gallery::gallery_rows(
        catalog,
        &app.schemes,
        "",
        None,
        &w.font_context(),
    );
    let line = rows[0].font.as_ref().expect("the row shows the suggestion");
    assert_eq!(line.family, "Fira Code");
    assert_eq!(line.state, FontState::NotInstalled);
    assert_eq!(
        line.url.as_deref(),
        Some("https://github.com/tonsky/FiraCode")
    );

    // Install writes the table into the scheme file, which reads back unedited.
    send(&mut app, SchemesMsg::Install("x".into()));
    let saved = read_file(&paths.schemes().join("gallery-x.toml")).unwrap();
    assert_eq!(saved.suggested_font.as_ref().unwrap().family, "Fira Code");
    assert!(!fbterm_config::schemes::is_edited(&saved));

    // Duplicate keeps it (like the credit) and drops the gallery block.
    send(&mut app, SchemesMsg::Select("gallery-x".into()));
    send(&mut app, SchemesMsg::Duplicate);
    let copy = win(&app).selected_draft().unwrap().clone();
    assert!(copy.gallery.is_none());
    assert_eq!(copy.suggested_font, saved.suggested_font);

    // A single-file export keeps it too (only `[gallery]` stays local).
    send(&mut app, SchemesMsg::Select("gallery-x".into()));
    let out = paths.schemes().with_file_name("exported-x.toml");
    send(&mut app, SchemesMsg::Exported(Some(out.clone())));
    let back = read_file(&out).unwrap();
    assert!(back.gallery.is_none());
    assert_eq!(back.suggested_font, saved.suggested_font);
}

#[test]
fn new_faces_are_only_new_suggested_families() {
    let faces = [
        ("Sample Mono", 400, false, 5, "/fonts/sample-regular.ttf"),
        ("Sample Mono", 700, false, 5, "/fonts/sample-bold.ttf"),
        ("Sample Mono", 400, false, 5, "/fonts/sample-regular.ttf"),
        ("Known Mono", 400, false, 5, "/fonts/known.ttf"),
        ("Unrelated Sans", 400, false, 5, "/fonts/unrelated.ttf"),
    ];
    let known = ["Known Mono".to_owned()].into_iter().collect();
    let suggested = ["Sample Mono".to_owned(), "Known Mono".to_owned()]
        .into_iter()
        .collect();
    assert_eq!(
        pick_new_faces(faces, &known, &suggested),
        vec!["/fonts/sample-regular.ttf", "/fonts/sample-bold.ttf"]
    );
    // Nothing suggested: nothing to load.
    assert!(pick_new_faces(faces, &known, &Default::default()).is_empty());
}

/// A large family loads only what a terminal draws with: the closest face to regular, bold,
/// italic and bold italic, not every weight and width.
#[test]
fn only_the_four_terminal_faces_of_a_family_load() {
    let faces = [
        ("Big Mono", 100, false, 5, "thin"),
        ("Big Mono", 300, false, 5, "light"),
        ("Big Mono", 400, false, 5, "regular"),
        ("Big Mono", 500, false, 5, "medium"),
        ("Big Mono", 600, false, 5, "semibold"),
        ("Big Mono", 700, false, 5, "bold"),
        ("Big Mono", 800, false, 5, "extrabold"),
        ("Big Mono", 300, true, 5, "light-italic"),
        ("Big Mono", 450, true, 5, "book-italic"),
        ("Big Mono", 800, true, 5, "extrabold-italic"),
        ("Odd Mono", 350, false, 5, "odd-light"),
        ("Odd Mono", 650, false, 5, "odd-heavy"),
    ];
    let suggested = ["Big Mono".to_owned(), "Odd Mono".to_owned()]
        .into_iter()
        .collect();
    let picked = pick_new_faces(faces, &Default::default(), &suggested);
    assert_eq!(
        picked,
        vec![
            "regular",
            "bold",
            "book-italic",
            "extrabold-italic",
            "odd-light",
            "odd-heavy",
        ]
    );
}

/// Condensed and expanded faces sharing the family name never stand in for the normal width,
/// whatever the scan order; a family with no normal width gets its closest one.
#[test]
fn normal_width_faces_are_preferred() {
    let faces = [
        ("Wide Mono", 400, false, 3, "condensed"),
        ("Wide Mono", 700, false, 3, "condensed-bold"),
        ("Wide Mono", 400, true, 7, "expanded-italic"),
        ("Wide Mono", 400, false, 5, "regular"),
        ("Wide Mono", 600, false, 5, "semibold"),
        ("Wide Mono", 400, true, 5, "italic"),
        ("Narrow Only", 400, false, 2, "extra-condensed"),
        ("Narrow Only", 400, false, 4, "semi-condensed"),
    ];
    let suggested = ["Wide Mono".to_owned(), "Narrow Only".to_owned()]
        .into_iter()
        .collect();
    assert_eq!(
        pick_new_faces(faces, &Default::default(), &suggested),
        vec!["regular", "semibold", "italic", "semi-condensed"]
    );
}

#[test]
fn suggested_families_count_as_installed_without_the_fixed_pitch_flag() {
    let faces = [
        ("Fixed Mono", true),
        ("Flagless Code", false),
        ("Body Sans", false),
        ("Fixed Mono", true),
    ];
    let suggested = ["Flagless Code".to_owned()].into_iter().collect();
    assert_eq!(
        installed_families(faces, &suggested),
        strings(&["Fixed Mono", "Flagless Code"])
    );
    assert_eq!(
        installed_families(faces, &Default::default()),
        strings(&["Fixed Mono"])
    );
}

#[test]
fn download_shows_its_host() {
    assert_eq!(
        url_host("https://github.com/IBM/plex").as_deref(),
        Some("github.com")
    );
    assert_eq!(
        url_host("https://vercel.com/font").as_deref(),
        Some("vercel.com")
    );
    assert_eq!(
        url_host("https://www.jetbrains.com/lp/mono/").as_deref(),
        Some("www.jetbrains.com")
    );
    assert_eq!(
        url_host("https://a.example:8443/x?y#z").as_deref(),
        Some("a.example")
    );
    // Anything but a plain host gives no label at all (validation drops such tables).
    assert_eq!(url_host("https://user@evil.example/"), None);
    assert_eq!(url_host("https://evil.example\\@github.com/"), None);
    assert_eq!(url_host("https://gith%75b.com/"), None);
    assert_eq!(url_host("https://a.example").as_deref(), Some("a.example"));
    assert_eq!(url_host("http://a.example/"), None);

    let ocean = suggesting("Ocean", 0x102030, "Sample Mono");
    let (mut app, _, _, _) = app_with(&[profile("a")], "", std::slice::from_ref(&ocean));
    send(&mut app, SchemesMsg::Open);
    let line = win(&app)
        .font_line(win(&app).draft(&ocean.id).unwrap())
        .unwrap();
    assert_eq!(line.host.as_deref(), Some("fonts.example"));
}

#[test]
fn use_font_is_off_while_in_use() {
    let ocean = suggesting("Ocean", 0x102030, "Sample Mono");
    let (mut app, _, _, _) = app_with(
        &[profile("a")],
        "font_family = \"Geist Mono\"\nuse_theme_fonts = true\n",
        std::slice::from_ref(&ocean),
    );
    installed(&mut app, &["Sample Mono"]);
    send(&mut app, SchemesMsg::Open);
    let line = win(&app)
        .font_line(win(&app).draft(&ocean.id).unwrap())
        .unwrap();
    assert_eq!(line.state, FontState::InUse);
    assert!(!line.can_use, "already what its panes use");
}

/// The restart gap: a font installed while fbterm runs is loaded into the renderer on the next
/// user action, after which the line, `Use font` and the panes agree.
#[test]
fn a_font_installed_while_running_is_loaded_on_the_next_showing() {
    let ocean = suggesting("Ocean", 0x102030, "Sample Mono");
    let a = with_scheme(&profile("a"), &ocean.id);
    let (mut app, _, wid, ids) = app_with(
        &[a],
        "use_theme_fonts = true\nfont_family = \"Geist Mono\"\n",
        std::slice::from_ref(&ocean),
    );
    let fake = fake_fonts(&mut app, &[], &[]);
    app.refresh_installed_fonts();
    assert_eq!(style(&app, wid, ids[0]).font, family("Geist Mono"));

    // The user installs the font, then shows the Color schemes tab.
    fake.system.lock().push("Sample Mono".into());
    let task = app.update(Message::Schemes(SchemesMsg::Open));
    drop(task);
    assert!(app.fonts_scanning, "the scan runs on the blocking pool");
    finish_scan(&mut app);
    assert!(!app.fonts_scanning);
    assert_eq!(*fake.loaded.lock(), strings(&["Sample Mono"]));
    assert_eq!(style(&app, wid, ids[0]).font, family("Sample Mono"));
    let line = win(&app)
        .font_line(win(&app).draft(&ocean.id).unwrap())
        .unwrap();
    assert_eq!(line.state, FontState::InUse);
    // Showing again scans for the suggestions still missing (the built-ins' here) and loads
    // nothing twice.
    send(&mut app, SchemesMsg::Open);
    finish_scan(&mut app);
    assert_eq!(*fake.scans.lock(), 2);
    assert_eq!(*fake.loaded.lock(), strings(&["Sample Mono"]));
}

/// Select the first tab of the window (not the Color schemes page).
fn select_first_tab(app: &mut App, wid: iced::window::Id) {
    app.windows.get_mut(&wid).unwrap().select(0);
}

#[test]
fn selecting_the_schemes_tab_scans() {
    let ocean = suggesting("Ocean", 0x102030, "Sample Mono");
    let (mut app, _, wid, _) = app_with(&[profile("a")], "", std::slice::from_ref(&ocean));
    let fake = fake_fonts(&mut app, &[], &[]);
    send(&mut app, SchemesMsg::Open);
    assert!(app.fonts_scanning);
    finish_scan(&mut app);
    assert_eq!(*fake.scans.lock(), 1);
    select_first_tab(&mut app, wid);
    fake.system.lock().push("Sample Mono".into());
    send(&mut app, SchemesMsg::Open);
    assert!(app.fonts_scanning);
    finish_scan(&mut app);
    assert_eq!(*fake.scans.lock(), 2);
    let line = win(&app)
        .font_line(win(&app).draft(&ocean.id).unwrap())
        .unwrap();
    assert_ne!(line.state, FontState::NotInstalled);
}

#[test]
fn focusing_the_window_with_the_tab_active_scans() {
    let ocean = suggesting("Ocean", 0x102030, "Sample Mono");
    let (mut app, _, wid, _) = app_with(&[profile("a")], "", std::slice::from_ref(&ocean));
    let fake = fake_fonts(&mut app, &[], &[]);
    send(&mut app, SchemesMsg::Open);
    finish_scan(&mut app);
    assert_eq!(*fake.scans.lock(), 1);
    let _ = app.update(Message::WindowFocused(wid));
    assert!(app.fonts_scanning, "focus with the tab active scans");
    finish_scan(&mut app);
    assert_eq!(*fake.scans.lock(), 2);
    select_first_tab(&mut app, wid);
    let _ = app.update(Message::WindowFocused(wid));
    assert!(!app.fonts_scanning, "another tab active: no scan");
    assert_eq!(*fake.scans.lock(), 2);
}

#[test]
fn focusing_during_a_scan_queues_one_more() {
    let ocean = suggesting("Ocean", 0x102030, "Sample Mono");
    let (mut app, _, wid, _) = app_with(&[profile("a")], "", std::slice::from_ref(&ocean));
    let fake = fake_fonts(&mut app, &[], &[]);
    send(&mut app, SchemesMsg::Open);
    assert!(app.fonts_scanning);
    // The user installs the font while the scan runs, then focuses the window.
    fake.system.lock().push("Sample Mono".into());
    let _ = app.update(Message::WindowFocused(wid));
    assert!(app.fonts_rescan, "one more scan is queued");
    // The running scan ends (it read the system before the install) and the follow-up runs.
    let stale = Vec::new();
    let _ = app.update(Message::FontsScanned(Arc::new(stale)));
    assert!(app.fonts_scanning, "the follow-up scan started");
    finish_scan(&mut app);
    assert!(!app.fonts_scanning);
    let line = win(&app)
        .font_line(win(&app).draft(&ocean.id).unwrap())
        .unwrap();
    assert_ne!(line.state, FontState::NotInstalled);
}

#[test]
fn nothing_missing_means_no_scan_on_show() {
    let (mut app, _, wid, _) = app_with(&[profile("a")], "", &[]);
    let fake = fake_fonts(
        &mut app,
        &["Geist Mono", "JetBrains Mono", "IBM Plex Mono"],
        &[],
    );
    send(&mut app, SchemesMsg::Open);
    select_first_tab(&mut app, wid);
    send(&mut app, SchemesMsg::Open);
    let _ = app.update(Message::WindowFocused(wid));
    assert!(!app.fonts_scanning);
    assert_eq!(*fake.scans.lock(), 0);
}

#[test]
fn nothing_missing_means_no_scan() {
    let (mut app, _, _, _) = app_with(&[profile("a")], "", &[]);
    let fake = fake_fonts(
        &mut app,
        &["Geist Mono", "JetBrains Mono", "IBM Plex Mono"],
        &[],
    );
    send(&mut app, SchemesMsg::Open);
    assert!(!app.fonts_scanning);
    assert_eq!(*fake.scans.lock(), 0);
}

/// The label shows the whole host, or its end when it is long: the part that says who owns it.
#[test]
fn long_hosts_keep_their_end() {
    assert_eq!(host_label("github.com", 32), "github.com");
    let long = "github.com.aaaaaaaaaaaaaaaaaaaaaa.evil.example";
    let label = host_label(long, 32);
    assert!(label.starts_with('\u{2026}'), "{label}");
    assert!(label.ends_with("evil.example"), "{label}");
    assert!(!label.contains("github.com"), "{label}");
    assert!(label.chars().count() <= 32, "{label}");
    // Cut at a label boundary when there is one in reach.
    assert_eq!(
        host_label("fonts.cdn.example.org", 14),
        "\u{2026}example.org"
    );
}

/// The label never shrinks to a bare public suffix (`…com`, `…github.io`): it keeps the last
/// two labels (three under a two-label public suffix), and shows the whole host when even
/// those do not fit, leaving the cut to the layout.
#[test]
fn host_labels_keep_the_owner() {
    let e = '\u{2026}';
    // A tail that starts exactly at a label boundary keeps that label.
    let boundary = format!("fonts.{}.com", "a".repeat(27));
    assert_eq!(
        host_label(&boundary, 32),
        format!("{e}{}.com", "a".repeat(27))
    );
    // A long owner name: the whole host, never `…com`.
    let long_owner = format!("{}.com", "b".repeat(40));
    assert_eq!(host_label(&long_owner, 32), long_owner);
    let long_sub = format!("fonts.{}.com", "b".repeat(40));
    assert_eq!(host_label(&long_sub, 32), long_sub);
    // Under github.io (and co.uk style suffixes) the tenant label stays.
    let pages = format!("fonts.{}.github.io", "c".repeat(20));
    assert_eq!(
        host_label(&pages, 32),
        format!("{e}{}.github.io", "c".repeat(20))
    );
    let long_tenant = format!("{}.github.io", "d".repeat(30));
    assert_eq!(host_label(&long_tenant, 32), long_tenant);
    let uk = format!("fonts.{}.foundry.co.uk", "x".repeat(20));
    assert_eq!(host_label(&uk, 32), format!("{e}foundry.co.uk"));
    // Short hosts stay whole.
    assert_eq!(host_label("vercel.com", 32), "vercel.com");
}

/// A scheme built in code with a crafted URL (validation would drop it) still never gets a
/// `Download` button or a host label.
#[test]
fn a_crafted_url_gets_no_download() {
    let mut s = suggesting("Ocean", 0x102030, "Sample Mono");
    s.suggested_font.as_mut().unwrap().url = "https://evil.example\\@github.com/".into();
    let fonts = crate::schemes_window::FontContext {
        installed: &[],
        global_font: "Geist Mono",
        use_theme_fonts: false,
    };
    let line = fonts.line(&s).unwrap();
    assert_eq!(line.url, None);
    assert_eq!(line.host, None);
    let (mut app, _, _, _) = app_with(&[profile("a")], "", &[]);
    let opened = record_opens(&mut app);
    send(
        &mut app,
        SchemesMsg::DownloadFont("https://evil.example\\@github.com/".into()),
    );
    assert!(opened.lock().is_empty());
}

/// A pane whose scheme names a missing font by itself was measured with the fallback face; a
/// load moves the font epoch, so every terminal view measures again (and reports its grid only
/// when the cell size changed).
#[test]
fn a_load_makes_the_views_measure_again() {
    let (mut app, _, _, _) = app_with(&[profile("a")], "", &[]);
    // This app's own hook, so tests running in parallel cannot disturb the count.
    let fake = fake_fonts(&mut app, &[], &[]);
    let _ = app.update(Message::FontsScanned(Arc::new(Vec::new())));
    assert_eq!(
        *fake.notified.lock(),
        0,
        "nothing loaded, nothing to measure"
    );
    let _ = app.update(Message::FontsScanned(Arc::new(vec![NewFace {
        family: "Sample Mono".into(),
        path: std::path::PathBuf::new(),
    }])));
    assert_eq!(*fake.notified.lock(), 1);
}

#[test]
fn showing_the_gallery_scans() {
    let (mut app, _, _, _) = app_with(&[profile("a")], "", &[]);
    let fake = fake_fonts(&mut app, &[], &["JetBrains Mono"]);
    send(&mut app, SchemesMsg::Open);
    finish_scan(&mut app);
    // Installed after the tab opened: showing the gallery finds it.
    fake.system.lock().push("IBM Plex Mono".into());
    send(&mut app, SchemesMsg::ShowGallery(true));
    assert!(app.fonts_scanning);
    finish_scan(&mut app);
    assert_eq!(
        *fake.loaded.lock(),
        strings(&["JetBrains Mono", "IBM Plex Mono"])
    );
}

#[test]
fn a_loaded_catalog_scans_and_one_arriving_mid_scan_queues_one_more() {
    let (mut app, _, _, _) = app_with(&[profile("a")], "", &[]);
    let fake = fake_fonts(&mut app, &[], &["Fira Code"]);
    let bytes = catalog_with_font();
    app.gallery_io = GalleryIo {
        fetch: Arc::new(move |_, _| Ok(bytes.clone())),
        open: Arc::new(|_| {}),
    };
    send(&mut app, SchemesMsg::Open);
    finish_scan(&mut app);
    assert!(
        fake.loaded.lock().is_empty(),
        "no scheme suggests Fira Code yet"
    );

    // The gallery starts the fetch and a scan; the catalog arrives while the scan runs.
    send(&mut app, SchemesMsg::ShowGallery(true));
    assert!(app.fonts_scanning);
    let running = app.font_scan_job();
    let result = app.gallery_job()();
    let generation = app.gallery_generation;
    send(&mut app, SchemesMsg::GalleryFetched(generation, result));
    // The running scan finishes, then one follow-up scan covers the catalog's families.
    let faces = running();
    assert!(
        faces.is_empty(),
        "the running scan did not ask for Fira Code"
    );
    let task = app.update(Message::FontsScanned(Arc::new(faces)));
    drop(task);
    assert!(app.fonts_scanning, "a follow-up scan started");
    finish_scan(&mut app);
    assert!(!app.fonts_scanning);
    assert_eq!(*fake.loaded.lock(), strings(&["Fira Code"]));

    // A catalog loaded with no scan running scans at once.
    let _ = app.update(Message::Schemes(SchemesMsg::RefreshGallery));
    fake.system.lock().push("Later Mono".into());
    let result = app.gallery_job()();
    let generation = app.gallery_generation;
    send(&mut app, SchemesMsg::GalleryFetched(generation, result));
    assert!(app.fonts_scanning);
    finish_scan(&mut app);
}

#[test]
fn turning_theme_fonts_on_scans_and_select_never_does() {
    let ocean = suggesting("Ocean", 0x102030, "Sample Mono");
    let a = with_scheme(&profile("a"), &ocean.id);
    let (mut app, _, wid, ids) = app_with(&[a], "", std::slice::from_ref(&ocean));
    let fake = fake_fonts(&mut app, &[], &["Sample Mono"]);
    let task = app.update(Message::Settings(SettingsMsg::Open));
    drop(task);
    let task = app.update(Message::Settings(SettingsMsg::UseThemeFonts(true)));
    drop(task);
    assert!(app.fonts_scanning, "turning it on starts a scan");
    finish_scan(&mut app);
    assert_eq!(style(&app, wid, ids[0]).font, family("Sample Mono"));

    // Moving through the scheme list reads nothing again.
    send(&mut app, SchemesMsg::Open);
    let (lists, scans) = (*fake.lists.lock(), *fake.scans.lock());
    for id in ["harbor", "ember", ocean.id.as_str(), "graphite"] {
        send(&mut app, SchemesMsg::Select(id.into()));
    }
    assert_eq!(*fake.lists.lock(), lists);
    assert_eq!(*fake.scans.lock(), scans);
}
