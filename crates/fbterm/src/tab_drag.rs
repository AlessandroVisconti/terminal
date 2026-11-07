//! Pressing, dragging and dropping on a strip of tabs, shared by the tab bar (tabs along the
//! top) and the sidebar (tabs down the left): the same drag protocol ([`DragMsg`]) along
//! either axis, and the right-click that asks for a tab's menu.

use iced::advanced::Shell;
use iced::{Event, Point, Rectangle, mouse, window};

use crate::app::{Message, TabBarMsg};
use crate::tabs::TabId;
use crate::windows::{Carry, DragMsg};

/// How far a press travels before it carries the tab instead of clicking it.
pub(crate) const DRAG_THRESHOLD: f32 = 6.0;

/// Along which axis the strip lays its tabs out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Axis {
    /// Side by side (the tab bar).
    X,
    /// One under the other (the sidebar).
    Y,
}

impl Axis {
    fn along(self, p: Point) -> f32 {
        match self {
            Axis::X => p.x,
            Axis::Y => p.y,
        }
    }

    fn center(self, r: &Rectangle) -> f32 {
        match self {
            Axis::X => r.center_x(),
            Axis::Y => r.center_y(),
        }
    }
}

/// A press on a tab, until its release. The tab is remembered by id, not by index: tabs can
/// come and go during a drag, and a release must act on the tab that was pressed, or on none.
pub(crate) struct Press {
    pub id: TabId,
    pub origin: Point,
    /// Travelled past the threshold: the tab is carried, the release is no click.
    pub dragging: bool,
}

#[derive(Default)]
pub(crate) struct DragState {
    pub press: Option<Press>,
    /// The tab a carried tab hovers, as last reported.
    pub over: Option<usize>,
    /// Where a carried tile would become a tab, or a carried tab of this strip would move
    /// to, as last drawn.
    pub gap: Option<usize>,
}

/// What a strip of tabs is while an event is handled.
pub(crate) struct Strip<'a> {
    pub window: window::Id,
    /// Each tab's id, in strip order.
    pub ids: &'a [TabId],
    pub carry: Option<Carry>,
    /// The palette or a dialog covers the window: no tab drags, no menu.
    pub modal: bool,
    /// The active tab's index: a tile dropped on it goes nowhere.
    pub hole: Option<usize>,
    pub axis: Axis,
    /// How far the strip's coordinates are shifted from the window's: the sidebar's scroll
    /// offset (its rows are measured inside the scrollable), 0 for the bar.
    pub scroll: f32,
}

/// Where something dropped at `p` lands between the tabs: before the tab whose first half
/// (along the axis) is under `p`, after the one whose second half is, else the nearest gap.
/// Over its own tab (`own`): nowhere.
pub(crate) fn gap_at(
    rects: &[Rectangle],
    own: Option<usize>,
    p: Point,
    axis: Axis,
) -> Option<usize> {
    // A strip listing no tabs (the bar while tabs are on the left) takes no drop.
    if rects.is_empty() {
        return None;
    }
    if let Some(i) = rects.iter().position(|r| r.contains(p)) {
        if Some(i) == own {
            return None;
        }
        return Some(if axis.along(p) < axis.center(&rects[i]) {
            i
        } else {
            i + 1
        });
    }
    Some(
        rects
            .iter()
            .position(|r| axis.along(p) < axis.center(r))
            .unwrap_or(rects.len()),
    )
}

/// The tab-drag and right-click side of `event` over a strip whose tabs sit at `rects`
/// inside `bounds`: a press that travels [`DRAG_THRESHOLD`] carries its tab, a carried tab
/// of this strip dropped in a gap asks to move there ([`DragMsg::ReorderTab`]), a carried
/// tile dropped in a gap becomes a tab there. Returns `true` when the content beneath must
/// not see where the pointer is (the release that ends a tab drag is no click on a button; a
/// right-click is the menu's, not the button's).
pub(crate) fn drag_event(
    strip: &Strip<'_>,
    state: &mut DragState,
    event: &Event,
    rects: &[Rectangle],
    bounds: Rectangle,
    cursor: mouse::Cursor,
    shell: &mut Shell<'_, Message>,
) -> bool {
    let publish = |shell: &mut Shell<'_, Message>, msg: DragMsg| {
        shell.publish(Message::Drag(strip.window, msg));
    };
    let under = |p: Point| rects.iter().position(|r| r.contains(p));
    let index_of = |id: TabId| strip.ids.iter().position(|i| *i == id);
    let tile = matches!(strip.carry, Some(Carry::Tile(_)));
    // The index of the tab this strip is carrying, if any: it can be dropped back on the
    // strip, in another gap.
    let own = state
        .press
        .as_ref()
        .filter(|p| p.dragging && matches!(strip.carry, Some(Carry::Tab(id)) if id == p.id))
        .and_then(|p| index_of(p.id));
    // A drag that ended elsewhere (Escape) leaves no gap behind.
    if !tile && own.is_none() && state.gap.take().is_some() {
        shell.request_redraw();
    }
    let gap = |p: Option<Point>| {
        let p = p.filter(|_| tile || own.is_some())?;
        let gap = gap_at(rects, if tile { strip.hole } else { own }, p, strip.axis)?;
        // Right before or after the carried tab: no move.
        (own != Some(gap) && own.map(|o| o + 1) != Some(gap)).then_some(gap)
    };
    match event {
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
            state.press = cursor
                .position_over(bounds)
                .filter(|_| !strip.modal)
                .and_then(|p| {
                    let id = *strip.ids.get(under(p)?)?;
                    Some(Press {
                        id,
                        origin: p,
                        dragging: false,
                    })
                });
        }
        // A wheel click on a tab closes it, as in a browser.
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Middle)) if !strip.modal => {
            if let Some(index) = cursor.position_over(bounds).and_then(under) {
                shell.publish(Message::TabBar(strip.window, TabBarMsg::Close(index)));
            }
        }
        // A right-click on a tab asks for its menu, at the pointer in window coordinates.
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) if !strip.modal => {
            if let Some(p) = cursor.position_over(bounds)
                && let Some(index) = under(p)
            {
                let at = Point::new(p.x, p.y - strip.scroll);
                shell.publish(Message::TabBar(strip.window, TabBarMsg::Menu { index, at }));
                return true;
            }
        }
        Event::Mouse(mouse::Event::CursorMoved { .. }) => {
            if let (Some(press), Some(p)) = (&mut state.press, cursor.position()) {
                if !press.dragging && p.distance(press.origin) >= DRAG_THRESHOLD {
                    press.dragging = true;
                    if strip.ids.contains(&press.id) {
                        shell.publish(Message::Drag(strip.window, DragMsg::StartTab(press.id)));
                    }
                }
                if press.dragging && matches!(strip.carry, Some(Carry::Tab(_))) {
                    let mine = index_of(press.id);
                    let over = cursor
                        .position_over(bounds)
                        .and_then(under)
                        .filter(|i| Some(*i) != mine);
                    if over != state.over {
                        state.over = over;
                        publish(shell, DragMsg::OverTab(over));
                    }
                }
            }
            let at = gap(cursor.position_over(bounds));
            if at != state.gap {
                state.gap = at;
                shell.request_redraw();
            }
        }
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
            let at = gap(cursor.position_over(bounds));
            if state.gap.take().is_some() {
                shell.request_redraw();
            }
            if let Some(Press { dragging: true, .. }) = state.press.take() {
                state.over = None;
                // This strip's own tab let go in a gap: it moves there, then the drag ends.
                if let (Some(index), Some(tab)) =
                    (at.filter(|_| !tile), own.and_then(|o| strip.ids.get(o)))
                {
                    publish(shell, DragMsg::ReorderTab { tab: *tab, index });
                }
                publish(shell, DragMsg::End);
                return true;
            }
            if let (Some(Carry::Tile(pane)), Some(index)) =
                (strip.carry, gap(cursor.position_over(bounds)))
            {
                publish(shell, DragMsg::TileToTab { pane, index });
            }
        }
        Event::Mouse(mouse::Event::CursorLeft) | Event::Window(window::Event::Unfocused) => {
            if let Some(Press { dragging: true, .. }) = state.press.take() {
                publish(shell, DragMsg::End);
            }
            state.over = None;
            if state.gap.take().is_some() {
                shell.request_redraw();
            }
        }
        _ => {}
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::{Point, Rectangle, Size};

    #[test]
    fn gap_at_works_along_either_axis() {
        let rows = [
            Rectangle::new(Point::new(0.0, 0.0), Size::new(100.0, 30.0)),
            Rectangle::new(Point::new(0.0, 34.0), Size::new(100.0, 30.0)),
        ];
        assert_eq!(gap_at(&rows, None, Point::new(10.0, 5.0), Axis::Y), Some(0));
        assert_eq!(
            gap_at(&rows, None, Point::new(10.0, 25.0), Axis::Y),
            Some(1)
        );
        assert_eq!(
            gap_at(&rows, None, Point::new(10.0, 60.0), Axis::Y),
            Some(2)
        );
        assert_eq!(
            gap_at(&rows, None, Point::new(10.0, 200.0), Axis::Y),
            Some(2)
        );
        assert_eq!(
            gap_at(&rows, None, Point::new(10.0, 32.0), Axis::Y),
            Some(1)
        );
        assert_eq!(
            gap_at(&rows, Some(1), Point::new(10.0, 40.0), Axis::Y),
            None
        );
        let tabs = [
            Rectangle::new(Point::new(0.0, 0.0), Size::new(100.0, 30.0)),
            Rectangle::new(Point::new(102.0, 0.0), Size::new(100.0, 30.0)),
        ];
        assert_eq!(
            gap_at(&tabs, None, Point::new(160.0, 5.0), Axis::X),
            Some(2)
        );
        assert_eq!(
            gap_at(&tabs, None, Point::new(120.0, 5.0), Axis::X),
            Some(1)
        );
        assert_eq!(gap_at(&tabs, None, Point::new(20.0, 5.0), Axis::X), Some(0));
        assert_eq!(gap_at(&tabs, Some(0), Point::new(20.0, 5.0), Axis::X), None);
        // A strip with no tabs (the bar while tabs are on the left) takes no drop.
        assert_eq!(gap_at(&[], None, Point::new(20.0, 5.0), Axis::X), None);
    }

    /// Runs `events` (each with the pointer position, in strip coordinates) through a strip
    /// of `rows` down the left, scrolled by `scroll`, while `carry` is under way. Returns the
    /// drag messages published.
    fn drive_rows(
        rows: &[Rectangle],
        ids: &[TabId],
        carry: Option<Carry>,
        scroll: f32,
        events: &[(Event, Point)],
    ) -> Vec<DragMsg> {
        use iced::advanced::Shell;
        let strip = Strip {
            window: window::Id::unique(),
            ids,
            carry,
            modal: false,
            hole: Some(0),
            axis: Axis::Y,
            scroll,
        };
        let bounds = Rectangle::new(
            Point::new(0.0, rows[0].y),
            Size::new(100.0, rows.len() as f32 * 34.0),
        );
        let mut state = DragState::default();
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        for (event, at) in events {
            drag_event(
                &strip,
                &mut state,
                event,
                rows,
                bounds,
                mouse::Cursor::Available(*at),
                &mut shell,
            );
        }
        messages
            .into_iter()
            .filter_map(|m| match m {
                Message::Drag(_, d) => Some(d),
                _ => None,
            })
            .collect()
    }

    fn press() -> Event {
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
    }

    fn release() -> Event {
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
    }

    fn moved(p: Point) -> Event {
        Event::Mouse(mouse::Event::CursorMoved { position: p })
    }

    #[test]
    fn a_tab_carried_down_the_sidebar_and_dropped_in_a_gap_asks_to_move_there() {
        // Three rows inside a list scrolled by 400 (content coordinates).
        let rows = [
            Rectangle::new(Point::new(0.0, 400.0), Size::new(100.0, 30.0)),
            Rectangle::new(Point::new(0.0, 434.0), Size::new(100.0, 30.0)),
            Rectangle::new(Point::new(0.0, 468.0), Size::new(100.0, 30.0)),
        ];
        let ids = [TabId::next(), TabId::next(), TabId::next()];
        let at = rows[0].center();
        let away = Point::new(at.x, at.y + 10.0);
        // The lower half of the second row: the gap after it, index 2.
        let over = Point::new(at.x, rows[1].center_y() + 10.0);
        let drags = drive_rows(
            &rows,
            &ids,
            Some(Carry::Tab(ids[0])),
            400.0,
            &[
                (press(), at),
                (moved(away), away),
                (moved(over), over),
                (release(), over),
            ],
        );
        assert_eq!(
            drags
                .iter()
                .filter(|d| matches!(d, DragMsg::ReorderTab { .. }))
                .collect::<Vec<_>>(),
            [&DragMsg::ReorderTab {
                tab: ids[0],
                index: 2
            }],
            "{drags:?}"
        );
        assert_eq!(drags.last(), Some(&DragMsg::End));

        // The upper half of the second row is the gap right after itself: no move.
        let over = Point::new(at.x, rows[1].center_y() - 10.0);
        let drags = drive_rows(
            &rows,
            &ids,
            Some(Carry::Tab(ids[0])),
            400.0,
            &[
                (press(), at),
                (moved(away), away),
                (moved(over), over),
                (release(), over),
            ],
        );
        assert!(
            !drags
                .iter()
                .any(|d| matches!(d, DragMsg::ReorderTab { .. })),
            "{drags:?}"
        );
        assert_eq!(drags.last(), Some(&DragMsg::End));
    }

    #[test]
    fn a_tab_that_left_the_strip_during_its_drag_is_not_reordered() {
        let rows = [
            Rectangle::new(Point::new(0.0, 0.0), Size::new(100.0, 30.0)),
            Rectangle::new(Point::new(0.0, 34.0), Size::new(100.0, 30.0)),
        ];
        let ids = [TabId::next(), TabId::next()];
        // The carried tab is not one of this strip's rows any more (it moved to another
        // window): a release over a gap here asks for no move.
        let gone = TabId::next();
        let at = rows[0].center();
        let over = Point::new(at.x, rows[1].center_y() + 10.0);
        let drags = drive_rows(
            &rows,
            &ids,
            Some(Carry::Tab(gone)),
            0.0,
            &[(press(), at), (moved(over), over), (release(), over)],
        );
        assert!(
            !drags
                .iter()
                .any(|d| matches!(d, DragMsg::ReorderTab { .. })),
            "{drags:?}"
        );
    }

    #[test]
    fn a_right_click_reports_the_menu_at_the_pointer_in_window_coordinates() {
        use iced::advanced::Shell;
        // Content coordinates: the list is scrolled by 400, so its rows and the pointer the
        // scrollable hands down are 400 below the window's.
        let rows = [
            Rectangle::new(Point::new(0.0, 400.0), Size::new(100.0, 30.0)),
            Rectangle::new(Point::new(0.0, 434.0), Size::new(100.0, 30.0)),
        ];
        let ids = [TabId::next(), TabId::next()];
        let strip = Strip {
            window: window::Id::unique(),
            ids: &ids,
            carry: None,
            modal: false,
            hole: Some(0),
            axis: Axis::Y,
            // The rows are measured inside a scrollable scrolled by 400: a pointer at
            // content y 440 is at window y 40.
            scroll: 400.0,
        };
        let mut state = DragState::default();
        let mut messages = Vec::new();
        let mut shell = Shell::new(&mut messages);
        let taken = drag_event(
            &strip,
            &mut state,
            &Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)),
            &rows,
            Rectangle::new(Point::new(0.0, 400.0), Size::new(100.0, 64.0)),
            mouse::Cursor::Available(Point::new(10.0, 440.0)),
            &mut shell,
        );
        assert!(taken);
        assert!(matches!(
            messages.as_slice(),
            [Message::TabBar(_, TabBarMsg::Menu { index: 1, at })] if *at == Point::new(10.0, 40.0)
        ));
    }
}
