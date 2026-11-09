//! Suggested fonts for themes: how a pane's font resolves with `use_theme_fonts`, the
//! suggestion line of the Color schemes window and its gallery, and the Settings row, against
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

/// Make `families` the installed fonts, as the font database would report them.
fn installed(app: &mut App, families: &[&str]) {
    let list: Vec<String> = families.iter().map(|f| (*f).to_owned()).collect();
    app.font_list = Arc::new(move || list.clone());
    app.refresh_installed_fonts();
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

    // The Settings row writes the same key, and the Color schemes window follows.
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

    // Installed while fbterm runs: found the next time the details show.
    app.font_list = Arc::new(|| vec!["Sample Mono".to_owned()]);
    send(&mut app, SchemesMsg::Select(ocean.id.clone()));
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
