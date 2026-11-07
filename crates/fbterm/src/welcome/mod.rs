//! The Welcome tab: a page tab shown once, at launch, asking the first-launch questions
//! (where tabs go, whether to check for updates) in place of popups. Each answer is
//! written as it is chosen; closing the tab writes the defaults for what was not answered,
//! so it opens once per installation (unless the lines leave `settings.toml`).

use fbterm_config::{SettingChange, Settings, TabBar};
use iced::Task;

use crate::app::{App, Message};
use crate::tabs::Page;

pub(crate) mod view;

pub(crate) const TITLE: &str = "Welcome to fbterm";
pub(crate) const INTRO: &str =
    "A couple of choices before you start. Everything here can be changed later in Settings.";
pub(crate) const TABS_QUESTION: &str = "Where should open sessions be listed?";
/// The update question (the Settings tab's Updates switch says the same).
pub(crate) const UPDATES_BODY: &str = "fbterm can check GitHub once a day for a newer version and \
    show a dot on the settings button when one is out. Nothing is downloaded or installed. You \
    can change this later in Settings.";

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum WelcomeMsg {
    /// `Done`, the close-tab chord or the tab's close button.
    Close,
    ChooseTabs(TabBar),
    ChooseUpdates(bool),
}

impl App {
    /// Whether the tab opens at this launch: asked at most once per run, only in runs that
    /// keep state, while a question is unanswered, and only when `settings.toml` is absent
    /// or loads (a broken file could not take the answers).
    pub(crate) fn welcome_due(&self) -> bool {
        if !self.ask_welcome || !self.state_saving_enabled {
            return false;
        }
        if self.settings.tab_bar.is_some() && self.settings.check_updates.is_some() {
            return false;
        }
        let path = self.paths.settings();
        !path.exists() || Settings::load(&path).is_ok()
    }

    /// Open the tab, selected, in the window that takes page tabs, when it is due.
    pub(crate) fn open_welcome_if_due(&mut self) -> Task<Message> {
        if !self.welcome_due() {
            return Task::none();
        }
        self.ask_welcome = false;
        self.show_page(Page::Welcome).1
    }

    pub(crate) fn welcome_msg(&mut self, msg: WelcomeMsg) -> Task<Message> {
        match msg {
            WelcomeMsg::Close => self.close_page(Page::Welcome),
            WelcomeMsg::ChooseTabs(v) => self.set_setting(SettingChange::TabBar(v)),
            WelcomeMsg::ChooseUpdates(v) => self.set_setting(SettingChange::CheckUpdates(v)),
        }
    }

    /// The tab closed (any way but the app quitting): the questions still unanswered get
    /// their defaults, tabs on top and no update check, as a dismissed question would.
    /// Quitting with it open leaves them unanswered, so the tab returns next launch.
    pub(crate) fn welcome_closed(&mut self) -> Task<Message> {
        if self.quitting {
            return Task::none();
        }
        let mut tasks = Vec::new();
        if self.settings.tab_bar.is_none() {
            tasks.push(self.set_setting(SettingChange::TabBar(TabBar::Top)));
        }
        if self.settings.check_updates.is_none() {
            tasks.push(self.set_setting(SettingChange::CheckUpdates(false)));
        }
        Task::batch(tasks)
    }
}

#[cfg(test)]
mod tests;
