//! The look of each open window's macOS blur view (`macos_blur_style`), kept in step with
//! the setting and the scheme the window shows.

use iced::{Task, window};

use crate::app::{App, Message};
use crate::chrome::{self, BlurLook};

impl App {
    /// The blur view look for window `id` now: the `macos_blur_style` setting, in the
    /// variant of the scheme its active tab's first pane (the layout's top-left tile) shows,
    /// stable while focus moves between panes (a page tab's stand-in pane follows the global
    /// scheme), with the debug override.
    pub(crate) fn blur_look_for(&self, id: window::Id) -> BlurLook {
        let scheme = self
            .windows
            .get(&id)
            .and_then(|w| w.active_tab())
            .and_then(|tab| tab.pane(tab.layout.first_pane()))
            .map_or_else(|| self.global_scheme(), |pane| self.scheme_for(pane));
        let dark = fbterm_config::schemes::is_dark(&scheme.colors);
        chrome::blur_look(&self.settings.macos_blur_style, dark, chrome::demo_blur())
    }

    /// Windows whose blur view should change, with their new look; records it as applied.
    /// Entries of windows that are gone are dropped. Runs after every message, so it
    /// allocates only when something changed.
    pub(crate) fn blur_retunes(&mut self) -> Vec<(window::Id, BlurLook)> {
        if self.blur_views.is_empty() {
            return Vec::new();
        }
        let windows = &self.windows;
        self.blur_views.retain(|id, _| windows.contains_key(id));
        let mut out: Vec<(window::Id, BlurLook)> = self
            .blur_views
            .iter()
            .filter_map(|(&id, &had)| {
                let look = self.blur_look_for(id);
                (look != had).then_some((id, look))
            })
            .collect();
        out.sort_unstable_by_key(|(id, _)| *id);
        for (id, look) in &out {
            self.blur_views.insert(*id, *look);
        }
        out
    }

    /// Re-tune, in place, every open blur view whose look changed (the style, the scheme, the
    /// active tab): one main-thread task per changed window, none when nothing changed.
    pub(crate) fn sync_blur_views(&mut self) -> Option<Task<Message>> {
        let retunes = self.blur_retunes();
        (!retunes.is_empty()).then(|| {
            Task::batch(
                retunes
                    .into_iter()
                    .map(|(id, look)| chrome::retune_blur(id, look)),
            )
        })
    }

    /// The system backdrop for terminal window `id`, opening now with blur: on macOS its blur
    /// view starts with the current look, remembered so later changes re-tune it.
    pub(crate) fn add_system_backdrop(&mut self, id: window::Id) -> Task<Message> {
        let look = self.blur_look_for(id);
        if cfg!(target_os = "macos") {
            self.blur_views.insert(id, look);
        }
        chrome::system_backdrop(id, look)
    }
}

#[cfg(test)]
mod tests {
    use fbterm_config::MacosBlurStyle;
    use iced::window;

    use crate::app::SettingsSource;
    use crate::chrome::{BlurAppearance, BlurLook, BlurMaterial};
    use crate::restore_tests::profile;
    use crate::schemes_tests::{app_with, with_scheme};
    use crate::tabs::{Pane, Tab, WindowState};

    const BLURRED: &str = "background_opacity = 0.8\nbackground_blur = true\n";

    fn classic(appearance: BlurAppearance) -> BlurLook {
        BlurLook {
            material: BlurMaterial::TITLEBAR,
            appearance,
            alpha: 1.0,
            clear: true,
        }
    }

    fn set_style(app: &mut crate::app::App, style: MacosBlurStyle) {
        let mut s = app.settings.clone();
        s.macos_blur_style = style;
        let _ = app.apply_settings(s, SettingsSource::Window);
    }

    #[test]
    fn an_open_blur_view_follows_the_style_live() {
        let (mut app, _paths, wid, _) = app_with(&[profile("a")], BLURRED, &[]);
        app.blur_views.insert(wid, BlurLook::NATIVE);
        assert!(app.blur_retunes().is_empty(), "already native");

        set_style(&mut app, MacosBlurStyle::Classic);
        assert_eq!(
            app.blur_retunes(),
            vec![(wid, classic(BlurAppearance::Dark))]
        );
        assert!(app.blur_retunes().is_empty(), "retuned once");
        assert_eq!(app.blur_views[&wid], classic(BlurAppearance::Dark));

        set_style(&mut app, MacosBlurStyle::Native);
        assert_eq!(app.blur_retunes(), vec![(wid, BlurLook::NATIVE)]);
    }

    #[test]
    fn classic_follows_the_scheme_the_window_shows() {
        let light = profile("light");
        let (mut app, _paths, wid, _) = app_with(
            &[profile("a")],
            &format!("{BLURRED}macos_blur_style = \"classic\"\n"),
            &[],
        );
        app.blur_views.insert(wid, BlurLook::NATIVE);
        assert_eq!(
            app.blur_retunes(),
            vec![(wid, classic(BlurAppearance::Dark))]
        );

        // The global scheme turns light: so does the blur view.
        let mut s = app.settings.clone();
        s.color_scheme = "graphite-light".into();
        let _ = app.apply_settings(s, SettingsSource::Window);
        assert_eq!(
            app.blur_retunes(),
            vec![(wid, classic(BlurAppearance::Light))]
        );

        // A window whose active tab's focused pane has its own (dark) scheme follows that
        // pane, not the global one.
        let own = with_scheme(&light, "graphite");
        let mut w = WindowState::new();
        w.push(Tab::single(Pane::for_profile(&own)));
        let other = window::Id::unique();
        app.windows.insert(other, w);
        app.blur_views.insert(other, BlurLook::NATIVE);
        assert_eq!(
            app.blur_retunes(),
            vec![(other, classic(BlurAppearance::Dark))]
        );
    }

    /// In a split whose panes use schemes of both variants, the blur follows the tab's first
    /// pane, so moving focus between panes never flips the window's backdrop.
    #[test]
    fn a_split_follows_its_first_pane_whatever_has_focus() {
        let dark = with_scheme(&profile("dark"), "graphite");
        let light = with_scheme(&profile("light"), "graphite-light");
        let (mut app, _paths, wid, ids) = app_with(
            &[light, dark],
            &format!("{BLURRED}macos_blur_style = \"classic\"\n"),
            &[],
        );
        app.blur_views.insert(wid, BlurLook::NATIVE);
        assert_eq!(
            app.blur_retunes(),
            vec![(wid, classic(BlurAppearance::Light))]
        );
        for focus in [ids[1], ids[0], ids[1]] {
            app.windows.get_mut(&wid).unwrap().tabs[0].focused = focus;
            assert!(app.blur_retunes().is_empty(), "focus moved: no re-tune");
        }
        // "First" is the top-left tile of the layout, whatever order the tab keeps its
        // panes in (that list has none).
        app.windows.get_mut(&wid).unwrap().tabs[0].panes.reverse();
        assert!(app.blur_retunes().is_empty(), "the left tile still decides");
        let tab = &mut app.windows.get_mut(&wid).unwrap().tabs[0];
        let _ = tab.layout.remove(ids[0]);
        tab.layout.split(ids[1], crate::layout::Side::Right, ids[0]);
        assert_eq!(
            tab.layout.panes()[0],
            ids[1],
            "the dark pane is top-left now"
        );
        assert_eq!(
            app.blur_retunes(),
            vec![(wid, classic(BlurAppearance::Dark))]
        );
    }

    #[test]
    fn windows_without_a_blur_view_are_left_alone() {
        let (mut app, _paths, wid, _) = app_with(
            &[profile("a")],
            &format!("{BLURRED}macos_blur_style = \"classic\"\n"),
            &[],
        );
        assert!(
            app.blur_retunes().is_empty(),
            "no blur view: nothing to tune"
        );
        // A window that closed drops its entry, and is never tuned.
        app.blur_views.insert(wid, BlurLook::NATIVE);
        app.windows.remove(&wid);
        assert!(app.blur_retunes().is_empty());
        assert!(!app.blur_views.contains_key(&wid));
    }

    #[test]
    fn a_new_blur_view_starts_with_the_current_look() {
        let (app, _paths, wid, _) = app_with(
            &[profile("a")],
            &format!("{BLURRED}macos_blur_style = \"classic\"\n"),
            &[],
        );
        assert_eq!(app.blur_look_for(wid), classic(BlurAppearance::Dark));
        let (app, _paths, wid, _) = app_with(&[profile("a")], BLURRED, &[]);
        assert_eq!(app.blur_look_for(wid), BlurLook::NATIVE);
    }
}
