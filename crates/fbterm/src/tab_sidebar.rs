//! Tabs listed down the left side of the window (`tab_bar = "left"`): the sidebar beside
//! the content, and switching sides.

use fbterm_config::keys::{Action, Bindings};
use fbterm_config::{SettingChange, TabBar};
use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Operation, Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell, overlay, renderer};
use iced::widget::{button, column, container, row, scrollable, text};
use iced::{
    Alignment, Background, Border, Color, Element, Event, Length, Padding, Rectangle, Size, Task,
    Vector, mouse, window,
};

use crate::app::{App, Message, TabBarMsg};
use crate::tab_drag::{Axis, DragState, Strip, drag_event};
use crate::tab_look::{Surroundings, look};
use crate::tabs::{Tab, TabId};
use crate::theme::{Tokens, UI_FONT};
use crate::tooltip::{tip, tooltip_text};
use crate::ui::{ICON_X, icon};
use crate::window::{MIN_TERMINAL_WIDTH, dot, page_icon, snap_rect, state_color};
use crate::windows::Carry;

/// The sidebar's width when the user never changed it, in logical pixels.
pub(crate) const SIDEBAR_WIDTH: f32 = 220.0;
/// The narrowest and widest the user can make the sidebar.
pub(crate) const MIN_SIDEBAR_WIDTH: f32 = 140.0;
pub(crate) const MAX_SIDEBAR_WIDTH: f32 = 480.0;

/// `width` within [`MIN_SIDEBAR_WIDTH`]..=[`MAX_SIDEBAR_WIDTH`]; [`SIDEBAR_WIDTH`] when it is
/// not a number.
pub(crate) fn clamp_sidebar_width(width: f32) -> f32 {
    if width.is_finite() {
        width.clamp(MIN_SIDEBAR_WIDTH, MAX_SIDEBAR_WIDTH)
    } else {
        SIDEBAR_WIDTH
    }
}
/// A row's height.
pub(crate) const ROW_HEIGHT: f32 = 32.0;
/// Space between rows.
pub(crate) const ROW_SPACING: f32 = 2.0;
/// Space around the list.
pub(crate) const LIST_PADDING: f32 = 6.0;

/// How wide the sidebar is drawn in a window `total` wide whose rule is `hairline` wide, when
/// the user chose `chosen`: that width while the terminal keeps [`MIN_TERMINAL_WIDTH`], less
/// in a narrower window, down to nothing.
pub(crate) fn sidebar_width(chosen: f32, total: f32, hairline: f32) -> f32 {
    chosen.min(total - MIN_TERMINAL_WIDTH - hairline).max(0.0)
}

/// Where row `index` starts, from the top of the list's content.
pub(crate) fn row_top(index: usize) -> f32 {
    LIST_PADDING + index as f32 * (ROW_HEIGHT + ROW_SPACING)
}

/// The scroll offset that shows row `index` when it is not fully in view (`offset` and
/// `viewport` are the list's current offset and visible height): the row with the list's
/// padding beside it on the side it was hidden, never above the top. `None` when it shows.
pub(crate) fn reveal(index: usize, offset: f32, viewport: f32) -> Option<f32> {
    let top = row_top(index);
    let bottom = top + ROW_HEIGHT;
    if top < offset {
        Some((top - LIST_PADDING).max(0.0))
    } else if bottom > offset + viewport {
        Some((bottom + LIST_PADDING - viewport).max(0.0))
    } else {
        None
    }
}

/// The list's scrollable id in window `window`.
pub(crate) fn scroll_id(window: window::Id) -> iced::widget::Id {
    iced::widget::Id::from(format!("tabs-{window:?}"))
}

/// How a window's sidebar is drawn.
#[derive(Clone, Copy)]
pub(crate) struct Sidebar<'b> {
    pub window: window::Id,
    pub tokens: &'static Tokens,
    /// What the rows are painted against (the active row takes the terminal's color).
    pub around: Surroundings,
    /// The sidebar's background: the bar's.
    pub bar_bg: Color,
    pub bindings: &'b Bindings,
    /// A tab or tile being dragged in this window.
    pub carry: Option<Carry>,
    /// The palette or a dialog covers the window: no drags, no menu.
    pub modal: bool,
    /// Physical pixels per logical pixel.
    pub scale: f32,
    /// The sidebar's width (see [`sidebar_width`]).
    pub width: f32,
}

/// The sidebar: the rows in a scrollable list, with a rule on the right.
pub(crate) fn view<'a>(s: Sidebar<'_>, tabs: &'a [Tab], active: usize) -> Element<'a, Message> {
    let t = s.tokens;
    let window = s.window;
    let rows: Vec<Element<'a, Message>> = tabs
        .iter()
        .enumerate()
        .map(|(i, tab)| row_view(&s, tab, i, i == active))
        .collect();
    let list = column(rows)
        .spacing(ROW_SPACING)
        .padding(LIST_PADDING)
        .width(Length::Fill);
    let list = Element::new(Rows {
        window,
        ids: tabs.iter().map(|t| t.id).collect(),
        carry: s.carry,
        modal: s.modal,
        hole: (active < tabs.len()).then_some(active),
        tokens: t,
        scale: s.scale,
        content: list.into(),
    });
    let list = scrollable(list)
        .id(scroll_id(window))
        .on_scroll(move |v: scrollable::Viewport| {
            Message::TabBar(
                window,
                TabBarMsg::Scrolled {
                    offset: v.absolute_offset().y,
                    height: v.bounds().height,
                },
            )
        })
        .direction(scrollable::Direction::Vertical(crate::ui::edge_scrollbar(
            false,
        )))
        .style(move |theme, status| crate::ui::scroll_style(t, theme, status))
        .width(Length::Fill)
        .height(Length::Fill);
    let bar_bg = s.bar_bg;
    let list = container(list)
        .width(s.width)
        .height(Length::Fill)
        .style(move |_| container::Style {
            background: Some(bar_bg.into()),
            ..container::Style::default()
        });
    row![list, crate::ui::vertical_rule(t, s.scale)]
        .height(Length::Fill)
        .into()
}

/// One row: the accent stripe, the state dot, the trouble dot, the title and the close
/// button, as a tab on the bar shows them.
fn row_view<'a>(s: &Sidebar<'_>, tab: &'a Tab, index: usize, active: bool) -> Element<'a, Message> {
    let t = s.tokens;
    let window = s.window;
    let (accent, accent_style) = (tab.accent(), tab.accent_style());
    let idle = look(t, accent, accent_style, active, false, s.around);
    let hover = look(t, accent, accent_style, active, true, s.around);
    let stripe = container(text(""))
        .width(3)
        .height(14)
        .style(move |_| container::Style {
            background: idle.stripe.map(Background::Color),
            border: Border {
                radius: 1.5.into(),
                ..Border::default()
            },
            ..container::Style::default()
        });
    let label = text(tab.title())
        .font(UI_FONT)
        .size(12.5)
        .wrapping(text::Wrapping::None)
        .color(idle.label);
    let close = button(icon(ICON_X, 12.0, idle.icon))
        .on_press(Message::TabBar(window, TabBarMsg::Close(index)))
        .padding(3)
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            button::Style {
                background: hovered.then_some(Background::Color(t.hover)),
                text_color: t.text_secondary,
                border: Border {
                    radius: 5.0.into(),
                    ..Border::default()
                },
                ..button::Style::default()
            }
        });
    let close = tip(
        t,
        close,
        if active && tab.panes.len() == 1 {
            tooltip_text("Close tab", Action::CloseTab, s.bindings)
        } else {
            "Close tab".to_owned()
        },
        false,
    );
    let mut inner = row![stripe];
    // A page tab has no session to show: it carries its page's icon, as on the bar.
    inner = match tab.page {
        None => inner.push(dot(state_color(t, tab.state()))),
        Some(page) => inner.push(icon(page_icon(page), 14.0, idle.icon)),
    };
    if tab.trouble() {
        inner = inner.push(dot(t.warning));
    }
    let inner = inner
        .push(container(label).width(Length::Fill).clip(true))
        .push(close)
        .spacing(8)
        .height(Length::Fill)
        .align_y(Alignment::Center);
    button(inner)
        .on_press(Message::TabBar(window, TabBarMsg::Select(index)))
        .width(Length::Fill)
        .height(ROW_HEIGHT)
        .padding(Padding::from([0.0, 8.0]))
        .style(move |_, status| {
            let hovered = matches!(status, button::Status::Hovered | button::Status::Pressed);
            let background = if active {
                idle.background
            } else if hovered {
                hover.background.or(Some(Background::Color(t.hover)))
            } else {
                idle.background
            };
            button::Style {
                background,
                text_color: idle.label,
                border: Border {
                    radius: 5.0.into(),
                    ..Border::default()
                },
                snap: true,
                ..button::Style::default()
            }
        })
        .into()
}

/// The list of rows inside the scrollable: it handles the drag protocol and the right-click
/// over the rows (the column's children), and draws the gap where a carried tile would
/// become a tab or a carried tab would move to.
struct Rows<'a> {
    window: window::Id,
    ids: Vec<TabId>,
    carry: Option<Carry>,
    modal: bool,
    hole: Option<usize>,
    tokens: &'static Tokens,
    scale: f32,
    content: Element<'a, Message>,
}

impl Rows<'_> {
    /// The strip as of one event. The scrollable hands its content a viewport shifted by
    /// the scroll translation and a layout that is not: their difference is the offset the
    /// rows and the pointer are measured at, read here rather than kept (iced reports no
    /// scroll when the list comes to fit, and a rebuilt list starts at 0).
    fn strip(&self, layout: Layout<'_>, viewport: &Rectangle) -> Strip<'_> {
        Strip {
            window: self.window,
            ids: &self.ids,
            carry: self.carry,
            modal: self.modal,
            hole: self.hole,
            axis: Axis::Y,
            scroll: viewport.y - layout.bounds().y,
        }
    }
}

/// The rows' bounds: the column's children.
fn row_rects(layout: Layout<'_>) -> Vec<Rectangle> {
    layout.children().map(|c| c.bounds()).collect()
}

impl Widget<Message, iced::Theme, iced::Renderer> for Rows<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<DragState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(DragState::default())
    }

    fn children(&self) -> Vec<Tree> {
        vec![Tree::new(&self.content)]
    }

    fn diff(&self, tree: &mut Tree) {
        tree.diff_children(std::slice::from_ref(&self.content));
    }

    fn size(&self) -> Size<Length> {
        self.content.as_widget().size()
    }

    fn layout(
        &mut self,
        tree: &mut Tree,
        renderer: &iced::Renderer,
        limits: &layout::Limits,
    ) -> layout::Node {
        self.content
            .as_widget_mut()
            .layout(&mut tree.children[0], renderer, limits)
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout<'_>,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .as_widget_mut()
            .operate(&mut tree.children[0], layout, renderer, operation);
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
        let state = tree.state.downcast_mut::<DragState>();
        let rects = row_rects(layout);
        let strip = self.strip(layout, viewport);
        // The row pressed at the start of a drag is let go without a click: it sees the
        // release with no pointer over it.
        let cursor = if drag_event(&strip, state, event, &rects, layout.bounds(), cursor, shell) {
            mouse::Cursor::Unavailable
        } else {
            cursor
        };
        self.content.as_widget_mut().update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            clipboard,
            shell,
            viewport,
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
        self.content.as_widget().mouse_interaction(
            &tree.children[0],
            layout,
            cursor,
            viewport,
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
        self.content.as_widget().draw(
            &tree.children[0],
            renderer,
            theme,
            style,
            layout,
            cursor,
            viewport,
        );
        // A carried tile or tab: a violet line in the gap where it would land.
        let state = tree.state.downcast_ref::<DragState>();
        if let (Some(Carry::Tile(_) | Carry::Tab(_)), Some(gap)) = (self.carry, state.gap) {
            let bounds = layout.bounds();
            let rows = row_rects(layout);
            let y = match rows.get(gap) {
                Some(next) => next.y - 2.0,
                None => rows.last().map_or(bounds.y, |r| r.y + r.height),
            };
            let marker = snap_rect(
                Rectangle {
                    x: bounds.x + LIST_PADDING,
                    y,
                    width: bounds.width - 2.0 * LIST_PADDING,
                    height: 2.0,
                },
                self.scale,
            );
            renderer.with_layer(bounds, |renderer| {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: marker,
                        border: Border::default(),
                        shadow: Default::default(),
                        snap: true,
                    },
                    self.tokens.accent,
                );
            });
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout<'b>,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
    ) -> Option<overlay::Element<'b, Message, iced::Theme, iced::Renderer>> {
        self.content.as_widget_mut().overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
        )
    }
}

impl App {
    /// The sidebar width the user chose for window `id`: [`SIDEBAR_WIDTH`] until changed.
    pub(crate) fn sidebar_width_of(&self, id: window::Id) -> f32 {
        self.sidebar_widths
            .get(&id)
            .copied()
            .unwrap_or(SIDEBAR_WIDTH)
    }

    /// The list scrolled (or was laid out): remember its offset and visible height, for the
    /// drag's coordinates and for scrolling a selected tab into view.
    pub(crate) fn sidebar_scrolled(&mut self, wid: window::Id, offset: f32, height: f32) {
        self.sidebar_scroll.insert(wid, (offset, height));
    }

    /// After every update: a window whose active tab changed scrolls its sidebar just enough
    /// to show the new active row, when it is not fully in view; `None` when nothing is to be
    /// scrolled (the common case, so the update's task is passed through untouched). Nothing
    /// while tabs are on top, where the lists do not exist: what was known about them is
    /// forgotten, so a list built afresh is read afresh.
    pub(crate) fn reveal_active_tabs(&mut self) -> Option<Task<Message>> {
        if !self.settings.tabs_left() {
            self.sidebar_shown.clear();
            self.sidebar_scroll.clear();
            return None;
        }
        let bar = crate::chrome::Chrome::current().bar_height();
        let mut tasks = Vec::new();
        for (wid, w) in &self.windows {
            let Some(tab) = w.active_tab() else {
                continue;
            };
            if self.sidebar_shown.get(wid) == Some(&tab.id) {
                continue;
            }
            self.sidebar_shown.insert(*wid, tab.id);
            let (offset, height) = match self.sidebar_scroll.get(wid) {
                Some(&v) => v,
                None => (0.0, self.sizes.get(wid).map_or(f32::MAX, |(_, h)| h - bar)),
            };
            if let Some(y) = reveal(w.active, offset, height) {
                self.sidebar_scroll.insert(*wid, (y, height));
                tasks.push(iced::widget::operation::scroll_to(
                    scroll_id(*wid),
                    iced::widget::operation::AbsoluteOffset { x: 0.0, y },
                ));
            }
        }
        (!tasks.is_empty()).then(|| Task::batch(tasks))
    }

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

#[cfg(test)]
mod tests {
    use fbterm_config::keys::Action;
    use iced::advanced::layout::Limits;
    use iced::advanced::widget::Tree;
    use iced::{Pixels, Size, window};

    use super::{
        LIST_PADDING, MAX_SIDEBAR_WIDTH, MIN_SIDEBAR_WIDTH, ROW_HEIGHT, SIDEBAR_WIDTH,
        clamp_sidebar_width, reveal, row_top, sidebar_width,
    };
    use crate::app::{App, Message, TabBarMsg};
    use crate::restore_tests::{paths_with, profile};
    use crate::tabs::{Pane, Tab, WindowState};

    fn app_with_tabs(names: &[&str]) -> (App, window::Id) {
        let profiles: Vec<_> = names.iter().map(|n| profile(n)).collect();
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
        app.sizes.insert(wid, (900.0, 600.0));
        (app, wid)
    }

    /// Build and lay the window's view out, as a frame would: a widget that cannot lay out
    /// panics here.
    fn lay_out(app: &App, wid: window::Id) {
        let renderer = iced::Renderer::Secondary(iced_tiny_skia::Renderer::new(
            iced::Font::DEFAULT,
            Pixels(16.0),
        ));
        let mut element = crate::window::view(app, wid);
        let mut tree = Tree::new(&element);
        let _ = element.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &Limits::new(Size::ZERO, Size::new(900.0, 600.0)),
        );
    }

    #[test]
    fn the_sidebar_yields_in_a_narrow_window() {
        assert_eq!(sidebar_width(220.0, 1000.0, 1.0), 220.0);
        assert_eq!(sidebar_width(220.0, 360.0, 1.0), 159.0);
        assert_eq!(sidebar_width(220.0, 150.0, 1.0), 0.0);
        // A chosen width larger than the room: the yield applies to the chosen width.
        assert_eq!(sidebar_width(480.0, 1000.0, 1.0), 480.0);
        assert_eq!(sidebar_width(480.0, 500.0, 1.0), 299.0);
    }

    #[test]
    fn the_chosen_width_is_clamped_and_defaults() {
        assert_eq!(clamp_sidebar_width(10.0), MIN_SIDEBAR_WIDTH);
        assert_eq!(clamp_sidebar_width(5000.0), MAX_SIDEBAR_WIDTH);
        assert_eq!(clamp_sidebar_width(300.0), 300.0);
        assert_eq!(clamp_sidebar_width(f32::NAN), SIDEBAR_WIDTH);
        assert_eq!(clamp_sidebar_width(f32::INFINITY), SIDEBAR_WIDTH);
    }

    #[test]
    fn rows_are_revealed_only_when_hidden() {
        assert_eq!(row_top(0), LIST_PADDING);
        assert_eq!(row_top(3), LIST_PADDING + 3.0 * (ROW_HEIGHT + 2.0));
        assert_eq!(reveal(0, 0.0, 300.0), None);
        assert_eq!(reveal(2, 0.0, 300.0), None);
        assert_eq!(
            reveal(10, 0.0, 300.0),
            Some(row_top(10) + ROW_HEIGHT + LIST_PADDING - 300.0)
        );
        assert_eq!(reveal(0, 100.0, 300.0), Some(0.0));
        assert_eq!(reveal(5, 400.0, 300.0), Some(row_top(5) - LIST_PADDING));
    }

    #[test]
    fn switching_sides_keeps_tabs_order_and_the_active_tab_and_lays_out() {
        let (mut app, wid) = app_with_tabs(&["a", "b", "c"]);
        app.windows.get_mut(&wid).unwrap().active = 1;
        let titles = |app: &App| -> Vec<String> {
            app.windows[&wid]
                .tabs
                .iter()
                .map(|t| t.title().to_owned())
                .collect()
        };
        let _ = app.action(None, Action::VerticalTabs);
        assert!(app.settings.tabs_left());
        assert_eq!(app.windows[&wid].active, 1);
        assert_eq!(titles(&app), ["a", "b", "c"]);
        lay_out(&app, wid);
        let _ = app.action(None, Action::VerticalTabs);
        assert!(!app.settings.tabs_left());
        assert_eq!(titles(&app), ["a", "b", "c"]);
        lay_out(&app, wid);
    }

    #[test]
    fn a_narrow_window_with_a_panel_still_lays_out() {
        let (mut app, wid) = app_with_tabs(&["a"]);
        app.sizes.insert(wid, (360.0, 300.0));
        let _ = app.action(None, Action::VerticalTabs);
        let _ = app.open_tunnels_panel(wid);
        lay_out(&app, wid);
    }

    #[test]
    fn nothing_to_scroll_means_no_task_and_tabs_on_top_forget_the_scroll() {
        let (mut app, wid) = app_with_tabs(&["a", "b", "c"]);
        assert!(app.reveal_active_tabs().is_none(), "tabs on top");
        let _ = app.update(Message::Shortcut(wid, Action::VerticalTabs));
        assert!(app.reveal_active_tabs().is_none(), "already shown");
        let scrolled = TabBarMsg::Scrolled {
            offset: 400.0,
            height: 200.0,
        };
        let _ = app.update(Message::TabBar(wid, scrolled));
        assert!(app.sidebar_scroll.contains_key(&wid));
        // Back on top: the scrollable is gone, and so is what we knew about it.
        let _ = app.update(Message::Shortcut(wid, Action::VerticalTabs));
        assert!(!app.sidebar_scroll.contains_key(&wid));
        assert!(!app.sidebar_shown.contains_key(&wid));
    }

    #[test]
    fn switching_back_to_the_left_reveals_the_active_tab_afresh() {
        let names: Vec<String> = (0..20).map(|i| format!("t{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let (mut app, wid) = app_with_tabs(&refs);
        app.windows.get_mut(&wid).unwrap().active = 19;
        let _ = app.update(Message::Shortcut(wid, Action::VerticalTabs));
        // Revealed against the window's height: the list scrolled down to row 19.
        let (offset, _) = app.sidebar_scroll[&wid];
        assert!(offset > 0.0, "{offset}");
        // Scrolled back up by hand, then top and left again: revealed again, from 0.
        let _ = app.update(Message::TabBar(
            wid,
            TabBarMsg::Scrolled {
                offset: 0.0,
                height: 200.0,
            },
        ));
        let _ = app.update(Message::Shortcut(wid, Action::VerticalTabs));
        let _ = app.update(Message::Shortcut(wid, Action::VerticalTabs));
        let (offset, _) = app.sidebar_scroll[&wid];
        assert!(offset > 0.0, "{offset}");
    }

    #[test]
    fn selecting_a_hidden_row_scrolls_the_sidebar_and_a_visible_one_does_not() {
        let names: Vec<String> = (0..20).map(|i| format!("t{i}")).collect();
        let refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let (mut app, wid) = app_with_tabs(&refs);
        let _ = app.update(Message::Shortcut(wid, Action::VerticalTabs));
        let first = app.windows[&wid].tabs[0].id;
        assert_eq!(app.sidebar_shown.get(&wid), Some(&first));
        let scrolled = TabBarMsg::Scrolled {
            offset: 0.0,
            height: 200.0,
        };
        let _ = app.update(Message::TabBar(wid, scrolled));
        assert_eq!(app.sidebar_scroll.get(&wid), Some(&(0.0, 200.0)));
        let _ = app.update(Message::SelectTab(19));
        let last = app.windows[&wid].tabs[19].id;
        assert_eq!(app.sidebar_shown.get(&wid), Some(&last));
        let _ = app.update(Message::SelectTab(18));
        assert_eq!(
            app.sidebar_shown.get(&wid),
            Some(&app.windows[&wid].tabs[18].id)
        );
    }
}
