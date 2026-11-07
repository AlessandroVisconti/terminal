//! The right-click menu on a tab (bar or sidebar): `Copy password` for a password SSH
//! profile's tab, and `Close tab`. Only the user's own right-click opens it.

use fbterm_config::{AuthMethod, ProfileId, ProfileKind};
use iced::{Point, Task, window};

use crate::app::{App, Message};
use crate::context_menu::ContextMenu;
use crate::tabs::TabId;

impl App {
    /// A right-click on the tab at `index` of window `wid`: its menu at `at`, with
    /// `Copy password` when the tab's focused pane is a password SSH profile's. Not while
    /// something covers the window (see [`App::menu_covered`]). The tab is not selected.
    pub(crate) fn open_tab_menu(
        &mut self,
        wid: window::Id,
        index: usize,
        at: Point,
    ) -> Task<Message> {
        if self.menu_covered(wid) {
            return Task::none();
        }
        let Some(tab) = self.windows.get(&wid).and_then(|w| w.tabs.get(index)) else {
            return Task::none();
        };
        let tab = tab.id;
        let password = self.tab_password_profile(wid, tab).is_some();
        self.context_menu = Some(ContextMenu::for_tab(wid, tab, at, password));
        Task::none()
    }

    /// The profile whose password `Copy password` would copy for `tab`: the tab's focused
    /// pane's, when the store still has it and it is an SSH profile with password
    /// authentication.
    pub(crate) fn tab_password_profile(&self, wid: window::Id, tab: TabId) -> Option<ProfileId> {
        let w = self.windows.get(&wid)?;
        let t = w.tabs.iter().find(|t| t.id == tab)?;
        if t.page.is_some() {
            return None;
        }
        let id = t.focused_pane().profile;
        let p = self.profiles.get(id)?;
        match &p.kind {
            ProfileKind::Ssh(s) if s.auth == AuthMethod::Password => Some(id),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use fbterm_config::{AuthMethod, Profile, ProfileId, ProfileKind, SshProfile};
    use iced::{Point, window};

    use crate::app::{App, Message, TabBarMsg};
    use crate::context_menu::{MenuItem, MenuMsg};
    use crate::restore_tests::{paths_with, profile};
    use crate::tabs::{Page, Pane, Tab, WindowState};

    pub(crate) fn ssh_password_profile(name: &str) -> Profile {
        Profile {
            icon: None,
            id: ProfileId::new(),
            name: name.into(),
            group: None,
            accent: None,
            accent_style: Default::default(),
            color_scheme: None,
            kind: ProfileKind::Ssh(SshProfile {
                host: "h.example.com".into(),
                port: 22,
                user: "root".into(),
                auth: AuthMethod::Password,
                jump: vec![],
                tunnels: vec![],
            }),
        }
    }

    /// An app with one window holding one single-pane tab per profile, in order.
    fn app_with_tabs(known: &[Profile], tabs: &[Profile]) -> (App, window::Id) {
        let (mut app, _) = App::build(paths_with(known), true);
        app.state_saving_enabled = false;
        let wid = window::Id::unique();
        let mut w = WindowState::new();
        for p in tabs {
            w.push(Tab::single(Pane::for_profile(p)));
        }
        w.active = 0;
        app.windows.clear();
        app.windows.insert(wid, w);
        app.sizes.insert(wid, (900.0, 600.0));
        (app, wid)
    }

    #[test]
    fn the_menu_offers_copy_password_only_for_password_ssh_tabs() {
        let (ssh, local) = (ssh_password_profile("prod"), profile("sh"));
        let (mut app, wid) = app_with_tabs(&[ssh.clone(), local.clone()], &[ssh, local]);
        app.windows
            .get_mut(&wid)
            .unwrap()
            .push(Tab::page(Page::Settings));
        app.windows.get_mut(&wid).unwrap().active = 0;
        let at = Point::new(40.0, 10.0);
        let _ = app.open_tab_menu(wid, 0, at);
        let m = app.context_menu.clone().unwrap();
        assert_eq!(
            m.items,
            vec![(MenuItem::CopyPassword, true), (MenuItem::CloseTab, true)]
        );
        assert_eq!(m.selected, MenuItem::CopyPassword);
        assert_eq!(m.at, at);
        assert_eq!(
            app.windows[&wid].active, 0,
            "a right-click does not select the tab"
        );
        let _ = app.open_tab_menu(wid, 1, at);
        assert_eq!(
            app.context_menu.clone().unwrap().items,
            vec![(MenuItem::CloseTab, true)]
        );
        let _ = app.open_tab_menu(wid, 2, at);
        assert_eq!(
            app.context_menu.clone().unwrap().items,
            vec![(MenuItem::CloseTab, true)]
        );
        let _ = app.open_tab_menu(wid, 7, at);
        assert!(
            app.context_menu.is_some(),
            "an unknown index changes nothing"
        );
    }

    #[test]
    fn a_deleted_profile_offers_no_copy_password() {
        // The pane keeps a ProfileId the store no longer has.
        let ssh = ssh_password_profile("gone");
        let (mut app, wid) = app_with_tabs(&[], &[ssh]);
        let _ = app.open_tab_menu(wid, 0, Point::ORIGIN);
        assert_eq!(
            app.context_menu.clone().unwrap().items,
            vec![(MenuItem::CloseTab, true)]
        );
    }

    #[test]
    fn close_tab_closes_the_right_clicked_tab_and_the_menu_follows_its_tab() {
        let (a, b) = (profile("a"), profile("b"));
        let (mut app, wid) = app_with_tabs(&[a.clone(), b.clone()], &[a, b]);
        let _ = app.open_tab_menu(wid, 1, Point::ORIGIN);
        let _ = app.update(Message::ContextMenu(
            wid,
            MenuMsg::Choose(MenuItem::CloseTab),
        ));
        assert_eq!(app.windows[&wid].tabs.len(), 1);
        assert_eq!(app.windows[&wid].tabs[0].title(), "a");
        assert_eq!(app.windows[&wid].active, 0);
        assert!(app.context_menu.is_none());
        // The menu's tab closed another way: the menu goes with it.
        let _ = app.open_tab_menu(wid, 0, Point::ORIGIN);
        assert!(app.context_menu.is_some());
        let _ = app.update(Message::TabBar(wid, TabBarMsg::Close(0)));
        assert!(app.context_menu.is_none());
    }

    #[test]
    fn the_tab_menu_does_not_open_under_the_palette() {
        let a = profile("a");
        let (mut app, wid) = app_with_tabs(std::slice::from_ref(&a), std::slice::from_ref(&a));
        let _ = app.open_palette(wid, String::new());
        let _ = app.open_tab_menu(wid, 0, Point::ORIGIN);
        assert!(app.context_menu.is_none());
    }

    #[test]
    fn a_right_click_message_opens_the_menu_through_the_tab_bar() {
        let a = profile("a");
        let (mut app, wid) = app_with_tabs(std::slice::from_ref(&a), std::slice::from_ref(&a));
        let at = Point::new(30.0, 12.0);
        let _ = app.update(Message::TabBar(wid, TabBarMsg::Menu { index: 0, at }));
        assert_eq!(app.context_menu.as_ref().map(|m| m.at), Some(at));
    }
}
