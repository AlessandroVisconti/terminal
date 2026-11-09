//! The app's chrome colors follow the global color scheme, against a temporary config
//! directory.

use crate::app::{DEFAULT_GRID, Message};
use crate::restore_tests::profile;
use crate::schemes_tests::{app_with, builtin, rgb, user_scheme, with_scheme};
use crate::schemes_window::{Entry, SchemesMsg};
use crate::tabs::{Pane, Tab};
use crate::theme::{DARK, tokens_for};
use crate::watch::Changed;

const SETTINGS_CHANGED: Changed = Changed {
    profiles: false,
    settings: true,
    schemes: false,
};

fn global(id: &str) -> String {
    format!("color_scheme = \"{id}\"\n")
}

#[test]
fn global_scheme_colors_the_chrome() {
    let night = user_scheme("Night", 0x102030);
    let (app, _, _, _) = app_with(
        &[profile("a")],
        &global(&night.id),
        std::slice::from_ref(&night),
    );
    assert_ne!(app.tokens.app, DARK.app);
    assert_eq!(*app.tokens, *tokens_for(&night.id, &night.colors));
    assert_eq!(app.chrome_scheme().id, night.id);
}

#[test]
fn profile_override_leaves_the_chrome() {
    // A guard: the chrome must come from the global scheme, never from the focused pane's.
    let night = user_scheme("Night", 0x102030);
    let dawn = user_scheme("Dawn", 0xf0e8d0);
    let plain = profile("plain");
    let a = with_scheme(&profile("a"), &dawn.id);
    let (mut app, paths, wid, _) = app_with(
        std::slice::from_ref(&plain),
        "",
        &[night.clone(), dawn.clone()],
    );
    // A second tab whose profile overrides the scheme, opened and focused.
    let mut pane = Pane::for_profile(&a);
    let term = app.new_terminal(DEFAULT_GRID);
    app.scheme_colors(&pane).apply_to(&mut term.lock());
    pane.placeholder = Some(term);
    app.windows.get_mut(&wid).unwrap().push(Tab::single(pane));
    app.focused = Some(wid);
    let _ = app.update(Message::SelectTab(1));
    let _ = app.update(Message::WindowFocused(wid));
    // Change the global scheme, so the chrome is derived again while that tab is focused.
    std::fs::write(paths.settings(), global(&night.id)).unwrap();
    let _ = app.update(Message::ConfigChanged(SETTINGS_CHANGED));

    let w = &app.windows[&wid];
    assert_eq!(w.active, 1);
    let focused = w.active_tab().unwrap().focused_pane();
    assert_eq!(app.scheme_for(focused).id, dawn.id, "the override is live");
    assert_eq!(app.pane_background(focused), rgb(0xf0e8d0));
    assert_ne!(
        *tokens_for(&dawn.id, &dawn.colors),
        *tokens_for(&night.id, &night.colors)
    );
    assert_eq!(*app.tokens, *tokens_for(&night.id, &night.colors));
    assert_eq!(app.chrome_scheme().id, night.id);
}

#[test]
fn missing_global_scheme_gives_graphite_chrome() {
    let (app, _, _, _) = app_with(&[profile("a")], &global("user-gone"), &[]);
    let graphite = builtin("graphite");
    assert!(std::ptr::eq(
        app.tokens,
        tokens_for("graphite", &graphite.colors)
    ));
    assert_eq!(app.chrome_scheme().id, "graphite");
}

#[test]
fn editing_the_global_scheme_recolors_on_save_only() {
    let night = user_scheme("Night", 0x102030);
    let (mut app, _, _, _) = app_with(
        &[profile("a")],
        &global(&night.id),
        std::slice::from_ref(&night),
    );
    let before = app.tokens;
    let _ = app.update(Message::Schemes(SchemesMsg::Open));
    let _ = app.update(Message::Schemes(SchemesMsg::Select(night.id.clone())));
    let _ = app.update(Message::Schemes(SchemesMsg::Color(
        Entry::Background,
        rgb(0x302010),
    )));
    assert!(std::ptr::eq(app.tokens, before), "a draft recolors nothing");
    let _ = app.update(Message::Schemes(SchemesMsg::Save));
    assert_ne!(*app.tokens, *before);
    let saved = app.schemes.get(&night.id).unwrap();
    assert_eq!(saved.colors.background, rgb(0x302010));
    assert_eq!(*app.tokens, *tokens_for(&saved.id, &saved.colors));
}

#[test]
fn file_change_of_color_scheme_retheme() {
    let night = user_scheme("Night", 0x102030);
    let (mut app, paths, _, _) = app_with(&[profile("a")], "", std::slice::from_ref(&night));
    let graphite = builtin("graphite");
    assert!(std::ptr::eq(
        app.tokens,
        tokens_for("graphite", &graphite.colors)
    ));
    std::fs::write(paths.settings(), global(&night.id)).unwrap();
    let _ = app.update(Message::ConfigChanged(SETTINGS_CHANGED));
    assert!(std::ptr::eq(
        app.tokens,
        tokens_for(&night.id, &night.colors)
    ));
    assert_eq!(app.theme.palette().background, app.tokens.app);
    // And back: Graphite Light gives the light set.
    std::fs::write(paths.settings(), global("graphite-light")).unwrap();
    let _ = app.update(Message::ConfigChanged(SETTINGS_CHANGED));
    let light = builtin("graphite-light");
    assert!(std::ptr::eq(
        app.tokens,
        tokens_for("graphite-light", &light.colors)
    ));
    assert_eq!(app.theme.palette().background, app.tokens.app);
}
