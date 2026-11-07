use fbterm_config::keys::Action;
use fbterm_config::{Paths, SavedState, Settings, TabBar};
use iced::window;

use super::WelcomeMsg;
use crate::app::{App, Message, TabBarMsg};
use crate::restore_tests::{paths_with, profile, saved};
use crate::tabs::Page;

/// An app restored from one window of one local tab, with `settings.toml` as given, ready
/// to ask (tests build with the question off).
fn app_with(settings: Option<&str>) -> (App, Paths, window::Id) {
    let a = profile("a");
    let paths = paths_with(std::slice::from_ref(&a));
    SavedState {
        windows: vec![saved(&[&a], 0)],
        ..Default::default()
    }
    .save(&paths.state())
    .unwrap();
    if let Some(s) = settings {
        std::fs::write(paths.settings(), s).unwrap();
    }
    let (mut app, _) = App::build(paths.clone(), true);
    app.ask_welcome = true;
    let wid = *app.windows.keys().next().unwrap();
    (app, paths, wid)
}

fn welcome_tab(app: &App) -> Option<(window::Id, usize)> {
    app.page_tab(Page::Welcome)
}

fn read(paths: &Paths) -> String {
    std::fs::read_to_string(paths.settings()).unwrap_or_default()
}

#[test]
fn a_fresh_install_opens_the_welcome_tab_once_and_only_in_runs_that_keep_state() {
    let (mut app, _, wid) = app_with(None);
    let _ = app.open_welcome_if_due();
    let (w, i) = welcome_tab(&app).expect("the Welcome tab");
    assert_eq!(w, wid);
    assert_eq!(app.windows[&wid].active, i);
    assert!(!app.ask_welcome);
    let _ = app.open_welcome_if_due();
    let count = app.windows[&wid]
        .tabs
        .iter()
        .filter(|t| t.page == Some(Page::Welcome))
        .count();
    assert_eq!(count, 1);
    let (mut app, _, _) = app_with(None);
    app.state_saving_enabled = false;
    let _ = app.open_welcome_if_due();
    assert!(welcome_tab(&app).is_none());
}

#[test]
fn answered_installs_and_unreadable_settings_do_not_get_the_tab() {
    let (mut app, _, _) = app_with(Some("tab_bar = \"top\"\ncheck_updates = false\n"));
    let _ = app.open_welcome_if_due();
    assert!(welcome_tab(&app).is_none(), "both answered");
    let (mut app, _, _) = app_with(Some("check_updates = false\n"));
    let _ = app.open_welcome_if_due();
    assert!(welcome_tab(&app).is_some(), "one still unanswered");
    let (mut app, _, _) = app_with(Some("tab_bar = \"left\"\n"));
    let _ = app.open_welcome_if_due();
    assert!(welcome_tab(&app).is_some(), "the other still unanswered");
    let (mut app, _, _) = app_with(Some("tab_bar = [\n"));
    let _ = app.open_welcome_if_due();
    assert!(
        welcome_tab(&app).is_none(),
        "a broken file could not take the answers"
    );
}

#[test]
fn each_choice_is_written_as_chosen_and_every_window_follows() {
    let (mut app, paths, _) = app_with(None);
    let _ = app.open_welcome_if_due();
    let _ = app.update(Message::Welcome(WelcomeMsg::ChooseTabs(TabBar::Left)));
    assert!(app.settings.tabs_left());
    assert!(read(&paths).contains("\ntab_bar = \"left\"\n"));
    assert_eq!(app.settings.check_updates, None);
    let _ = app.update(Message::Welcome(WelcomeMsg::ChooseUpdates(true)));
    assert_eq!(app.settings.check_updates, Some(true));
    assert!(read(&paths).contains("\ncheck_updates = true\n"));
    assert!(
        welcome_tab(&app).is_some(),
        "choosing does not close the tab"
    );
    let s = Settings::load(&paths.settings()).unwrap();
    assert_eq!(
        (s.tab_bar, s.check_updates),
        (Some(TabBar::Left), Some(true))
    );
}

#[test]
fn closing_the_tab_writes_the_defaults_for_what_was_not_answered() {
    let (mut app, paths, _) = app_with(None);
    let _ = app.open_welcome_if_due();
    let _ = app.update(Message::Welcome(WelcomeMsg::ChooseTabs(TabBar::Left)));
    let _ = app.update(Message::Welcome(WelcomeMsg::Close));
    assert!(welcome_tab(&app).is_none());
    assert_eq!(app.settings.tab_bar, Some(TabBar::Left));
    assert_eq!(app.settings.check_updates, Some(false));
    let s = Settings::load(&paths.settings()).unwrap();
    assert_eq!(
        (s.tab_bar, s.check_updates),
        (Some(TabBar::Left), Some(false))
    );
    // Closed from the tab bar instead: the same defaults.
    let (mut app, paths, wid) = app_with(None);
    let _ = app.open_welcome_if_due();
    let (_, i) = welcome_tab(&app).unwrap();
    let _ = app.update(Message::TabBar(wid, TabBarMsg::Close(i)));
    assert!(welcome_tab(&app).is_none());
    let s = Settings::load(&paths.settings()).unwrap();
    assert_eq!(
        (s.tab_bar, s.check_updates),
        (Some(TabBar::Top), Some(false))
    );
    // And it does not come back.
    app.ask_welcome = true;
    let _ = app.open_welcome_if_due();
    assert!(welcome_tab(&app).is_none());
}

#[test]
fn the_vertical_tabs_chord_acts_on_the_welcome_tab_and_its_cards_follow() {
    let (mut app, _, wid) = app_with(None);
    let _ = app.open_welcome_if_due();
    let _ = app.update(Message::Shortcut(wid, Action::VerticalTabs));
    assert!(app.settings.tabs_left());
    let _ = crate::window::view(&app, wid);
    let _ = app.update(Message::Shortcut(wid, Action::VerticalTabs));
    assert!(!app.settings.tabs_left());
    let _ = crate::window::view(&app, wid);
}

#[test]
fn the_welcome_tab_is_not_restored_and_cannot_be_split() {
    let (mut app, paths, wid) = app_with(None);
    let _ = app.open_welcome_if_due();
    app.save_state();
    let state = SavedState::load_or_default(&paths.state());
    assert_eq!(state.windows.len(), 1);
    assert_eq!(
        state.windows[0].tabs.len(),
        1,
        "only the profile tab is saved"
    );
    let (_, i) = welcome_tab(&app).unwrap();
    let _ = app.split_pane(wid, crate::layout::Side::Right);
    assert_eq!(app.windows[&wid].tabs[i].panes.len(), 1);
}
