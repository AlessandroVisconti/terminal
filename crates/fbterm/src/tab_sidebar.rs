//! Tabs listed down the left side of the window (`tab_bar = "left"`): the sidebar beside
//! the content, and switching sides.

use fbterm_config::{SettingChange, TabBar};
use iced::Task;

use crate::app::{App, Message};

impl App {
    /// The `Vertical tabs` action: tabs to the left, or back along the top. An unanswered
    /// setting counts as top, so the first press moves them left. Written and applied like a
    /// Settings tab change.
    pub(crate) fn toggle_vertical_tabs(&mut self) -> Task<Message> {
        let next = if self.settings.tabs_left() {
            TabBar::Top
        } else {
            TabBar::Left
        };
        self.set_setting(SettingChange::TabBar(next))
    }
}
