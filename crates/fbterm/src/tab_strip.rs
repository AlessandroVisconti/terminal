//! The tab bar's strip: where the tabs sit when they no longer fit, and how they are clipped.
//!
//! Tabs share the bar's room from [`TAB_MAX_WIDTH`] down to [`TAB_MIN_WIDTH`]; past that the
//! strip is narrower than its tabs and scrolls, while the buttons after it stay in view. The
//! layout here is pure: the window width, the chrome and the tab count decide everything.

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Operation, Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell, overlay, renderer};
use iced::{Element, Event, Length, Point, Rectangle, Size, Vector, mouse, window};

use crate::app::{App, Message};
use crate::chrome::{self, Chrome};
use crate::tabs::TabId;
use crate::window::{BAR_SPACING, BUTTON_WIDTH, trailing_width};

/// A tab's widest and narrowest width; tabs share the bar's room between them.
pub(crate) const TAB_MAX_WIDTH: f32 = 196.0;
pub(crate) const TAB_MIN_WIDTH: f32 = 80.0;
/// The gap between two tabs.
pub(crate) const TAB_SPACING: f32 = 2.0;
/// The two chevron buttons and their spacing.
pub(crate) const CHEVRONS_WIDTH: f32 = 2.0 * (BUTTON_WIDTH + BAR_SPACING);

/// Where the tabs sit in a bar of one window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct StripLayout {
    /// Each tab's width.
    pub tab_w: f32,
    /// The strip's visible width.
    pub strip_w: f32,
    /// All the tabs' width side by side.
    pub content_w: f32,
    /// The tabs do not fit at [`TAB_MIN_WIDTH`]: the strip scrolls and the chevrons show.
    pub overflow: bool,
}

/// The strip for `count` tabs in a window `width` wide (`None`: not known yet, full-size tabs),
/// with `trailing` px reserved for the button group after it (see `window::trailing_width`).
pub(crate) fn strip_layout(
    chrome: Chrome,
    fullscreen: bool,
    width: Option<f32>,
    count: usize,
    trailing: f32,
) -> StripLayout {
    let Some(width) = width else {
        return StripLayout {
            tab_w: TAB_MAX_WIDTH,
            strip_w: content_width(TAB_MAX_WIDTH, count),
            content_w: content_width(TAB_MAX_WIDTH, count),
            overflow: false,
        };
    };
    let controls = if chrome.draws_controls() {
        let setup = chrome::setup();
        chrome::controls_width(setup.look, setup.controls.order.len())
    } else {
        0.0
    };
    let room = width - chrome.leading_inset(fullscreen) - controls - trailing;
    let tab_w = (room / count.max(1) as f32 - TAB_SPACING).clamp(TAB_MIN_WIDTH, TAB_MAX_WIDTH);
    let content_w = content_width(tab_w, count);
    // Half a pixel of slack: widths come out of float arithmetic.
    let overflow = count as f32 * (TAB_MIN_WIDTH + TAB_SPACING) > room + 0.5;
    let strip_w = if overflow {
        (room - CHEVRONS_WIDTH - TAB_SPACING).max(0.0)
    } else {
        content_w
    };
    StripLayout {
        tab_w,
        strip_w,
        content_w,
        overflow,
    }
}

/// `count` tabs of `tab_w` side by side.
fn content_width(tab_w: f32, count: usize) -> f32 {
    count as f32 * tab_w + count.saturating_sub(1) as f32 * TAB_SPACING
}

/// `offset` kept within the strip: 0 to the hidden part of the tabs (0 when nothing scrolls).
pub(crate) fn clamp_offset(offset: f32, layout: &StripLayout) -> f32 {
    if layout.overflow {
        offset.clamp(0.0, (layout.content_w - layout.strip_w).max(0.0))
    } else {
        0.0
    }
}

/// The offset that shows the span `[start, start + len)` of a strip `viewport` wide scrolled by
/// `offset`, just enough, when it is not fully in view; `None` when it shows. A span longer
/// than the viewport shows its start.
pub(crate) fn reveal_span(start: f32, len: f32, offset: f32, viewport: f32) -> Option<f32> {
    if start < offset || len > viewport {
        Some(start.max(0.0))
    } else if start + len > offset + viewport {
        Some((start + len - viewport).max(0.0))
    } else {
        None
    }
}

/// The strip itself: `content` (the tabs' row) laid out `offset` px to the left of the strip's
/// own left edge and clipped to a strip `width` wide. The content's layout is therefore in
/// window coordinates, so whatever reads the tabs' rectangles (the bar's drags, hover tints
/// and outline) needs no knowledge of the offset. Only a pointer inside the strip reaches the
/// content: a tab scrolled out of view cannot be hovered or pressed.
pub(crate) struct Viewport<'a> {
    content: Element<'a, Message>,
    width: f32,
    offset: f32,
}

impl<'a> Viewport<'a> {
    pub(crate) fn new(content: Element<'a, Message>, width: f32, offset: f32) -> Self {
        Self {
            content,
            width,
            offset,
        }
    }

    /// The pointer as the content may see it: gone while it is outside the strip.
    fn inside(layout: Layout<'_>, cursor: mouse::Cursor) -> mouse::Cursor {
        if cursor.is_over(layout.bounds()) {
            cursor
        } else {
            mouse::Cursor::Unavailable
        }
    }
}

impl Widget<Message, iced::Theme, iced::Renderer> for Viewport<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::stateless()
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        Size::new(Length::Fixed(self.width), Length::Shrink)
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        let unbounded =
            layout::Limits::new(Size::ZERO, Size::new(f32::INFINITY, limits.max().height));
        let child = self
            .content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, &unbounded)
            .move_to(Point::new(-self.offset, 0.0));
        let size = Size::new(self.width, child.size().height);
        layout::Node::with_children(size, vec![child])
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        if let Some(child) = layout.children().next() {
            self.content
                .as_widget_mut()
                .operate(&mut tree.children[0], child, renderer, operation);
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        clipboard: &mut dyn Clipboard,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let Some(child) = layout.children().next() else {
            return;
        };
        let clip = layout.bounds().intersection(viewport).unwrap_or_default();
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            child,
            Self::inside(layout, cursor),
            renderer,
            clipboard,
            shell,
            &clip,
        );
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        let Some(child) = layout.children().next() else {
            return mouse::Interaction::None;
        };
        let clip = layout.bounds().intersection(viewport).unwrap_or_default();
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            child,
            Self::inside(layout, cursor),
            &clip,
            renderer,
        )
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout<'_>,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        use iced::advanced::Renderer as _;
        let Some(child) = layout.children().next() else {
            return;
        };
        let bounds = layout.bounds();
        let clip = bounds.intersection(viewport).unwrap_or_default();
        let cursor = Self::inside(layout, cursor);
        renderer.with_layer(bounds, |renderer| {
            self.content.as_widget().draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                child,
                cursor,
                &clip,
            );
        });
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        let child = layout.children().next()?;
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            child,
            renderer,
            viewport,
            translation,
        )
    }
}

impl<'a> From<Viewport<'a>> for Element<'a, Message> {
    fn from(strip: Viewport<'a>) -> Self {
        Element::new(strip)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window::trailing_width;

    fn layout(chrome: Chrome, width: Option<f32>, count: usize) -> StripLayout {
        strip_layout(chrome, false, width, count, trailing_width(3, true))
    }

    #[test]
    fn everything_fits_without_overflow() {
        let l = layout(Chrome::Unified, Some(1600.0), 3);
        assert_eq!(l.tab_w, TAB_MAX_WIDTH);
        assert!(!l.overflow);
        assert_eq!(l.content_w, 3.0 * l.tab_w + 2.0 * TAB_SPACING);
        assert_eq!(l.strip_w, l.content_w);
    }

    #[test]
    fn tabs_shrink_to_eighty_then_overflow() {
        let few = layout(Chrome::Unified, Some(1000.0), 5);
        assert!(
            TAB_MIN_WIDTH < few.tab_w && few.tab_w < TAB_MAX_WIDTH,
            "{few:?}"
        );
        assert!(!few.overflow);
        let more = layout(Chrome::Unified, Some(1000.0), 8);
        assert!(more.tab_w < few.tab_w, "more tabs, narrower tabs");
        assert!(
            layout(Chrome::Unified, Some(1000.0), 5).tab_w
                > layout(Chrome::Unified, Some(700.0), 5).tab_w,
            "narrower window, narrower tabs"
        );

        let l = layout(Chrome::Unified, Some(400.0), 30);
        assert_eq!(l.tab_w, 80.0);
        assert!(l.overflow);
        assert_eq!(
            l.strip_w,
            400.0
                - Chrome::Unified.leading_inset(false)
                - trailing_width(3, true)
                - CHEVRONS_WIDTH
                - TAB_SPACING
        );
        assert_eq!(l.content_w, 30.0 * 80.0 + 29.0 * TAB_SPACING);
    }

    #[test]
    fn the_overflow_threshold_is_exact() {
        let n = 10;
        let trailing = trailing_width(3, true);
        let inset = Chrome::Unified.leading_inset(false);
        // The room left of the group exactly holds `n` tabs of 80 px with their gaps.
        let exact = inset + trailing + n as f32 * (TAB_MIN_WIDTH + TAB_SPACING);
        let at = |width| strip_layout(Chrome::Unified, false, Some(width), n, trailing);
        assert!(!at(exact).overflow, "exactly fits");
        assert!(at(exact - 1.0).overflow, "a pixel less does not");
    }

    #[test]
    fn a_window_too_narrow_for_the_group_has_an_empty_strip() {
        let l = layout(Chrome::Unified, Some(10.0), 5);
        assert_eq!(l.strip_w, 0.0);
        assert_eq!(l.tab_w, TAB_MIN_WIDTH);
        assert!(l.overflow);
    }

    #[test]
    fn no_tabs_and_one_tab() {
        for count in [0, 1] {
            let l = layout(Chrome::Unified, Some(600.0), count);
            assert!(!l.overflow, "{count}: {l:?}");
            assert!(l.strip_w.is_finite() && l.strip_w >= 0.0);
            assert!(l.content_w.is_finite() && l.content_w >= 0.0);
        }
        assert_eq!(layout(Chrome::Unified, Some(600.0), 0).content_w, 0.0);
        for count in [0, 1, 50] {
            let l = layout(Chrome::Unified, None, count);
            assert_eq!(l.tab_w, TAB_MAX_WIDTH);
            assert!(!l.overflow);
        }
    }

    #[test]
    fn drawn_controls_and_full_screen_change_the_room() {
        let plain = layout(Chrome::Unified, Some(400.0), 30);
        let drawn = layout(Chrome::Drawn, Some(400.0), 30);
        let setup = chrome::setup();
        let controls = chrome::controls_width(setup.look, setup.controls.order.len());
        let inset_diff = Chrome::Unified.leading_inset(false) - Chrome::Drawn.leading_inset(false);
        assert_eq!(plain.strip_w - drawn.strip_w, controls - inset_diff);
        let full = strip_layout(
            Chrome::Unified,
            true,
            Some(400.0),
            30,
            trailing_width(3, true),
        );
        assert_eq!(
            full.strip_w - plain.strip_w,
            Chrome::Unified.leading_inset(false) - Chrome::Unified.leading_inset(true)
        );
    }

    #[test]
    fn clamp_offset_never_leaves_a_blank_strip() {
        let l = StripLayout {
            tab_w: 80.0,
            strip_w: 300.0,
            content_w: 420.0,
            overflow: true,
        };
        assert_eq!(clamp_offset(500.0, &l), 120.0);
        assert_eq!(clamp_offset(-5.0, &l), 0.0);
        assert_eq!(clamp_offset(50.0, &l), 50.0);
        let fits = StripLayout {
            overflow: false,
            strip_w: 420.0,
            ..l
        };
        assert_eq!(clamp_offset(50.0, &fits), 0.0);
    }

    #[test]
    fn reveal_span_cases() {
        // View [100, 400).
        assert_eq!(
            reveal_span(20.0, 80.0, 100.0, 300.0),
            Some(20.0),
            "left of it"
        );
        assert_eq!(
            reveal_span(450.0, 80.0, 100.0, 300.0),
            Some(230.0),
            "right of it"
        );
        assert_eq!(reveal_span(150.0, 80.0, 100.0, 300.0), None, "shown");
        assert_eq!(
            reveal_span(60.0, 80.0, 100.0, 300.0),
            Some(60.0),
            "half hidden left"
        );
        assert_eq!(
            reveal_span(360.0, 80.0, 100.0, 300.0),
            Some(140.0),
            "half hidden right"
        );
        assert_eq!(
            reveal_span(10.0, 500.0, 100.0, 300.0),
            Some(10.0),
            "longer than the view"
        );
        assert_eq!(
            reveal_span(0.0, 80.0, 100.0, 300.0),
            Some(0.0),
            "never negative"
        );
    }
}

/// How far one wheel line scrolls the strip.
pub(crate) const WHEEL_LINE_PX: f32 = 40.0;

impl App {
    /// The strip of window `wid`, from its size, its tabs and the buttons after it.
    pub(crate) fn strip_layout_of(&self, wid: window::Id) -> StripLayout {
        let count = self.windows.get(&wid).map_or(0, |w| w.tabs.len());
        let trailing = trailing_width(self.tunnels.up_count(), self.files_button(wid).is_some());
        strip_layout(
            Chrome::current(),
            self.fullscreen.contains(&wid),
            self.sizes.get(&wid).map(|&(w, _)| w),
            count,
            trailing,
        )
    }

    /// The window's strip offset as stored, kept within the strip as it is now.
    pub(crate) fn offset_of(&self, wid: window::Id) -> f32 {
        let stored = self.strip_offset.get(&wid).copied().unwrap_or(0.0);
        clamp_offset(stored, &self.strip_layout_of(wid))
    }

    /// Scroll the strip by `delta` logical pixels, within its ends.
    pub(crate) fn strip_scrolled_by(&mut self, wid: window::Id, delta: f32) {
        let next = clamp_offset(self.offset_of(wid) + delta, &self.strip_layout_of(wid));
        self.strip_offset.insert(wid, next);
    }

    /// A chevron: one tab toward `dir` (`-1` earlier tabs, `1` later).
    pub(crate) fn strip_stepped(&mut self, wid: window::Id, dir: i8) {
        let step = self.strip_layout_of(wid).tab_w + TAB_SPACING;
        self.strip_scrolled_by(wid, f32::from(dir) * step);
    }

    /// After every update, with tabs on top: a window whose active tab, its place among the
    /// tabs or its width changed since the strip last showed the active tab scrolls just enough
    /// to show it; one whose tabs fit goes back to the start. Any other change (a tab opening,
    /// the buttons' width) only clamps the offset, and a scroll by hand stays.
    pub(crate) fn reveal_strips(&mut self) {
        let changed: Vec<(window::Id, StripSeen, StripLayout)> = self
            .windows
            .iter()
            .filter_map(|(wid, w)| {
                let tab = w.active_tab()?;
                let seen = StripSeen {
                    tab: tab.id,
                    index: w.active,
                    width: self.sizes.get(wid).map(|&(w, _)| w),
                    fullscreen: self.fullscreen.contains(wid),
                };
                (self.strip_seen.get(wid) != Some(&seen))
                    .then(|| (*wid, seen, self.strip_layout_of(*wid)))
            })
            .collect();
        for (wid, seen, layout) in changed {
            self.strip_seen.insert(wid, seen);
            let start = seen.index as f32 * (layout.tab_w + TAB_SPACING);
            let offset = self.offset_of(wid);
            let next = if layout.overflow {
                reveal_span(start, layout.tab_w, offset, layout.strip_w).unwrap_or(offset)
            } else {
                0.0
            };
            self.strip_offset.insert(wid, clamp_offset(next, &layout));
        }
    }
}

/// What the reveal of a window's active tab was last done for.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct StripSeen {
    tab: TabId,
    index: usize,
    width: Option<f32>,
    fullscreen: bool,
}

#[cfg(test)]
mod app_tests {
    use fbterm_config::keys::Action;
    use iced::window;

    use super::*;
    use crate::app::{App, Message, TabBarMsg};
    use crate::restore_tests::{paths_with, profile};
    use crate::tabs::{Pane, Tab, WindowState};

    fn app_with_tabs(n: usize, width: f32) -> (App, window::Id) {
        let profiles: Vec<_> = (0..n).map(|i| profile(&format!("t{i}"))).collect();
        let (mut app, _) = App::build(paths_with(&profiles), true);
        app.state_saving_enabled = false;
        let wid = window::Id::unique();
        let mut w = WindowState::new();
        for p in &profiles {
            w.push(Tab::single(Pane::for_profile(p)));
        }
        w.active = 0;
        app.windows.clear();
        app.windows.insert(wid, w);
        app.sizes.insert(wid, (width, 600.0));
        (app, wid)
    }

    fn max(app: &App, wid: window::Id) -> f32 {
        let l = app.strip_layout_of(wid);
        l.content_w - l.strip_w
    }

    #[test]
    fn the_strip_offset_is_clamped() {
        let (mut app, wid) = app_with_tabs(40, 800.0);
        assert!(app.strip_layout_of(wid).overflow);
        app.strip_scrolled_by(wid, 1e6);
        assert_eq!(app.strip_offset[&wid], max(&app, wid));
        app.strip_scrolled_by(wid, -1e6);
        assert_eq!(app.strip_offset[&wid], 0.0);
        // Closed tabs shrink the strip: the stored offset is read back within it.
        app.strip_scrolled_by(wid, 1e6);
        for _ in 0..35 {
            let _ = app.close_tab(wid, 0);
        }
        assert_eq!(app.offset_of(wid), 0.0, "five tabs fit: nothing to scroll");
    }

    #[test]
    fn a_chevron_step_moves_one_tab() {
        let (mut app, wid) = app_with_tabs(40, 800.0);
        let step = app.strip_layout_of(wid).tab_w + TAB_SPACING;
        app.strip_stepped(wid, 1);
        assert_eq!(app.strip_offset[&wid], step);
        app.strip_stepped(wid, -1);
        app.strip_stepped(wid, -1);
        assert_eq!(app.strip_offset[&wid], 0.0, "never before the first tab");
    }

    #[test]
    fn selecting_a_hidden_tab_scrolls_it_into_view() {
        let (mut app, wid) = app_with_tabs(40, 800.0);
        let _ = app.update(Message::SelectTab(0));
        let _ = app.update(Message::SelectTab(39));
        let l = app.strip_layout_of(wid);
        let start = 39.0 * (l.tab_w + TAB_SPACING);
        let offset = app.offset_of(wid);
        assert!(offset <= start, "{offset} {start}");
        assert!(
            start + l.tab_w <= offset + l.strip_w + 0.5,
            "{offset} {start} {l:?}"
        );
        let _ = app.update(Message::SelectTab(0));
        assert_eq!(app.offset_of(wid), 0.0);
    }

    #[test]
    fn a_manual_scroll_is_kept_until_something_changes() {
        let (mut app, wid) = app_with_tabs(40, 800.0);
        let _ = app.update(Message::SelectTab(20));
        let at = app.offset_of(wid);
        app.strip_scrolled_by(wid, -100.0);
        let moved = app.offset_of(wid);
        assert_ne!(at, moved);
        // An unrelated update with the same active tab and layout leaves it.
        let _ = app.update(Message::TabBar(wid, TabBarMsg::StripScroll(0.0)));
        assert_eq!(app.offset_of(wid), moved);
        // A resized window shows the active tab again.
        app.sizes.insert(wid, (700.0, 600.0));
        let _ = app.update(Message::TabBar(wid, TabBarMsg::StripScroll(0.0)));
        let l = app.strip_layout_of(wid);
        let start = 20.0 * (l.tab_w + TAB_SPACING);
        let offset = app.offset_of(wid);
        assert!(offset <= start && start + l.tab_w <= offset + l.strip_w + 0.5);
    }

    #[test]
    fn tabs_on_the_left_leave_the_strip_alone() {
        let (mut app, wid) = app_with_tabs(40, 800.0);
        let _ = app.update(Message::SelectTab(30));
        assert!(app.strip_offset[&wid] > 0.0);
        let _ = app.update(Message::Shortcut(wid, Action::VerticalTabs));
        assert!(app.settings.tabs_left());
        assert!(!app.strip_offset.contains_key(&wid));
        assert!(!app.strip_seen.contains_key(&wid));
        let _ = app.update(Message::SelectTab(10));
        assert!(!app.strip_offset.contains_key(&wid), "no strip to scroll");
    }

    #[test]
    fn closing_the_window_forgets_the_strip() {
        let (mut app, wid) = app_with_tabs(40, 800.0);
        let _ = app.update(Message::SelectTab(30));
        assert!(app.strip_offset.contains_key(&wid) && app.strip_seen.contains_key(&wid));
        let _ = app.update(Message::WindowClosed(wid));
        assert!(!app.strip_offset.contains_key(&wid));
        assert!(!app.strip_seen.contains_key(&wid));
    }

    #[test]
    fn a_scroll_by_hand_survives_changes_that_are_not_the_active_tab_or_the_width() {
        let (mut app, wid) = app_with_tabs(40, 800.0);
        let _ = app.update(Message::SelectTab(20));
        app.strip_scrolled_by(wid, -1000.0);
        let moved = app.offset_of(wid);
        // Another tab opens at the end: the layout changes, the active tab does not.
        let extra = profile("extra");
        {
            let w = app.windows.get_mut(&wid).unwrap();
            w.push(Tab::single(Pane::for_profile(&extra)));
            // `push` shows the new tab; the active one stays here.
            w.active = 20;
        }
        let _ = app.update(Message::TabBar(wid, TabBarMsg::StripScroll(0.0)));
        assert_eq!(app.offset_of(wid), moved, "left where it was put");
    }

    #[test]
    fn moving_the_active_tab_reveals_it_again() {
        let (mut app, wid) = app_with_tabs(40, 800.0);
        let _ = app.update(Message::SelectTab(0));
        assert_eq!(app.offset_of(wid), 0.0);
        // The active tab is reordered to the end: same tab, new index.
        {
            let w = app.windows.get_mut(&wid).unwrap();
            w.tabs.swap(0, 39);
            w.active = 39;
        }
        let _ = app.update(Message::TabBar(wid, TabBarMsg::StripScroll(0.0)));
        let l = app.strip_layout_of(wid);
        let start = 39.0 * (l.tab_w + TAB_SPACING);
        let offset = app.offset_of(wid);
        assert!(
            offset <= start && start + l.tab_w <= offset + l.strip_w + 0.5,
            "{offset}"
        );
    }
}
