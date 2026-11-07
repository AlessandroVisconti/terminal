//! Resizing the tab sidebar: the rule at its right edge is dragged to set the window's
//! chosen width (see `tab_sidebar.rs`), a double-click on it puts the default back.
//!
//! The grab zone is the rule plus [`GRAB`] on the content's side, as a pane divider's grab
//! room lies over the panes next to it; nothing of it lies over the sidebar, whose rightmost
//! pixels are the list's floating scrollbar. The decisions are one pure function
//! ([`resize_event`]) over the widget's state, so they are tested without a window; the
//! widget ([`resizable`]) wraps the sidebar-plus-content row, turns the function's answers
//! into messages and draws the rule in the accent color while a drag is on.

use std::time::Instant;

use iced::advanced::layout::{self, Layout};
use iced::advanced::widget::{Operation, Tree, Widget, tree};
use iced::advanced::{Clipboard, Shell, overlay, renderer};
use iced::{Border, Element, Event, Length, Rectangle, Size, Vector, mouse, window};

use crate::app::{Message, TabBarMsg};
use crate::tab_sidebar::SIDEBAR_WIDTH;
use crate::theme::Tokens;
use crate::tiles::{DOUBLE_CLICK, cancels_drag};

/// How far the grab zone reaches past the rule, over the content.
pub(crate) const GRAB: f32 = 4.0;

/// The drag's state, kept by the widget between events.
#[derive(Default)]
pub(crate) struct ResizeState {
    /// A drag is on: the rule follows the pointer.
    pub(crate) dragging: bool,
    /// When the grab zone was last pressed, for the double-click.
    last_press: Option<Instant>,
}

/// What an event asks the app for.
#[derive(Debug, PartialEq)]
pub(crate) enum Out {
    /// The chosen width is now this (window logical pixels, unclamped).
    Resize(f32),
    /// The drag ended.
    End,
    /// A double-click on the rule: back to the default width.
    Reset,
}

/// What `event` means for the resize, with the grab zone at `grab` (window coordinates), the
/// sidebar's left edge at `origin_x` and the pointer at `cursor`: a press in the zone starts a
/// drag (a second press within [`DOUBLE_CLICK`] is a `Reset` instead and starts none), moves
/// while dragging set the width to the pointer's distance from `origin_x`, the release or
/// anything that cancels a drag ends it. Nothing starts while `modal` (the palette, a dialog
/// or a menu covers the window).
pub(crate) fn resize_event(
    state: &mut ResizeState,
    event: &Event,
    grab: Rectangle,
    origin_x: f32,
    modal: bool,
    cursor: mouse::Cursor,
    now: Instant,
) -> Option<Out> {
    match event {
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
            if !modal && cursor.is_over(grab) =>
        {
            let double = state
                .last_press
                .is_some_and(|at| now.duration_since(at) < DOUBLE_CLICK);
            if double {
                state.last_press = None;
                state.dragging = false;
                Some(Out::Reset)
            } else {
                state.last_press = Some(now);
                state.dragging = true;
                None
            }
        }
        Event::Mouse(mouse::Event::CursorMoved { position }) if state.dragging => {
            Some(Out::Resize(position.x - origin_x))
        }
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) if state.dragging => {
            state.dragging = false;
            Some(Out::End)
        }
        e if state.dragging && cancels_drag(e) => {
            state.dragging = false;
            Some(Out::End)
        }
        _ => None,
    }
}

/// Whether the content under the grab zone still sees `event` after the resize answered
/// `out` (`started`: the event began a drag): everything the resize did not take, and a
/// cancel that ended a drag, so the terminal can clear its hover state.
pub(crate) fn forwards(out: &Option<Out>, started: bool, event: &Event) -> bool {
    match out {
        Some(Out::End) => cancels_drag(event),
        Some(_) => false,
        None => !started,
    }
}

/// `content` (the sidebar, its rule and the window's content side by side) with the rule
/// draggable: the sidebar is drawn `width` wide and its rule `hairline` wide, so the grab zone
/// starts `width` in from the left edge.
pub(crate) fn resizable<'a>(
    window: window::Id,
    content: Element<'a, Message>,
    width: f32,
    hairline: f32,
    modal: bool,
    tokens: &'static Tokens,
) -> Element<'a, Message> {
    Element::new(Resizable {
        window,
        content,
        width,
        hairline,
        modal,
        tokens,
    })
}

struct Resizable<'a> {
    window: window::Id,
    content: Element<'a, Message>,
    width: f32,
    hairline: f32,
    modal: bool,
    tokens: &'static Tokens,
}

impl Resizable<'_> {
    /// The rule's rectangle, in window coordinates.
    fn rule(&self, bounds: Rectangle) -> Rectangle {
        Rectangle {
            x: bounds.x + self.width,
            width: self.hairline,
            ..bounds
        }
    }

    /// The grab zone: the rule and [`GRAB`] past it over the content.
    fn grab(&self, bounds: Rectangle) -> Rectangle {
        let rule = self.rule(bounds);
        Rectangle {
            width: rule.width + GRAB,
            ..rule
        }
    }
}

impl Widget<Message, iced::Theme, iced::Renderer> for Resizable<'_> {
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<ResizeState>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(ResizeState::default())
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
        let bounds = layout.bounds();
        let state = tree.state.downcast_mut::<ResizeState>();
        let was_dragging = state.dragging;
        let out = resize_event(
            state,
            event,
            self.grab(bounds),
            bounds.x,
            self.modal,
            cursor,
            Instant::now(),
        );
        let started = state.dragging && !was_dragging;
        match out {
            Some(Out::Resize(width)) => {
                shell.publish(Message::TabBar(
                    self.window,
                    TabBarMsg::SidebarResize(width),
                ));
            }
            Some(Out::End) => {
                shell.publish(Message::TabBar(self.window, TabBarMsg::SidebarResizeEnd));
            }
            Some(Out::Reset) => {
                shell.publish(Message::TabBar(
                    self.window,
                    TabBarMsg::SidebarResize(SIDEBAR_WIDTH),
                ));
                shell.publish(Message::TabBar(self.window, TabBarMsg::SidebarResizeEnd));
            }
            None => {}
        }
        // What the resize consumed never reaches the content under the grab zone.
        if !forwards(&out, started, event) {
            shell.capture_event();
            return;
        }
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
        let dragging = tree.state.downcast_ref::<ResizeState>().dragging;
        if dragging || (!self.modal && cursor.is_over(self.grab(layout.bounds()))) {
            return mouse::Interaction::ResizingHorizontally;
        }
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
        // While dragging, the rule shows the accent.
        if tree.state.downcast_ref::<ResizeState>().dragging {
            let bounds = layout.bounds();
            renderer.with_layer(bounds, |renderer| {
                renderer.fill_quad(
                    renderer::Quad {
                        bounds: self.rule(bounds),
                        border: Border::default(),
                        shadow: Default::default(),
                        snap: true,
                    },
                    self.tokens.accent_edge,
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

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use iced::{Event, Point, Rectangle, mouse, window};

    use super::{Out, ResizeState, forwards, resize_event};

    /// What the content under the grab zone still sees: a cancel that ends a drag (so the
    /// terminal can clear its hover), and anything the resize did not take.
    #[test]
    fn cancels_that_end_a_drag_still_reach_the_content() {
        let left = Event::Mouse(mouse::Event::CursorLeft);
        let unfocused = Event::Window(window::Event::Unfocused);
        assert!(forwards(&Some(Out::End), false, &left));
        assert!(forwards(&Some(Out::End), false, &unfocused));
        assert!(
            !forwards(&Some(Out::End), false, &release()),
            "the release is the resize's"
        );
        assert!(!forwards(&Some(Out::Resize(300.0)), false, &moved(300.0)));
        assert!(!forwards(&Some(Out::Reset), false, &press()));
        assert!(
            !forwards(&None, true, &press()),
            "the press that starts a drag"
        );
        assert!(forwards(&None, false, &moved(10.0)));
    }

    fn grab() -> Rectangle {
        Rectangle {
            x: 220.0,
            y: 0.0,
            width: 5.0,
            height: 600.0,
        }
    }

    fn at(x: f32) -> mouse::Cursor {
        mouse::Cursor::Available(Point::new(x, 100.0))
    }

    fn press() -> Event {
        Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left))
    }

    fn release() -> Event {
        Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left))
    }

    fn moved(x: f32) -> Event {
        Event::Mouse(mouse::Event::CursorMoved {
            position: Point::new(x, 100.0),
        })
    }

    #[test]
    fn a_press_in_the_grab_zone_starts_a_drag_and_moves_resize() {
        let t0 = Instant::now();
        let mut s = ResizeState::default();
        assert_eq!(
            resize_event(&mut s, &press(), grab(), 0.0, false, at(222.0), t0),
            None
        );
        assert!(s.dragging);
        assert_eq!(
            resize_event(&mut s, &moved(300.0), grab(), 0.0, false, at(300.0), t0),
            Some(Out::Resize(300.0))
        );
        assert_eq!(
            resize_event(&mut s, &release(), grab(), 0.0, false, at(300.0), t0),
            Some(Out::End)
        );
        assert!(!s.dragging);
    }

    #[test]
    fn the_width_is_measured_from_the_sidebars_left_edge() {
        let t0 = Instant::now();
        let mut s = ResizeState::default();
        let g = Rectangle { x: 270.0, ..grab() };
        assert_eq!(
            resize_event(&mut s, &press(), g, 50.0, false, at(272.0), t0),
            None
        );
        assert_eq!(
            resize_event(&mut s, &moved(300.0), g, 50.0, false, at(300.0), t0),
            Some(Out::Resize(250.0))
        );
    }

    #[test]
    fn a_press_outside_the_zone_does_nothing() {
        let t0 = Instant::now();
        let mut s = ResizeState::default();
        for x in [100.0, 240.0] {
            assert_eq!(
                resize_event(&mut s, &press(), grab(), 0.0, false, at(x), t0),
                None
            );
            assert!(!s.dragging);
        }
        assert_eq!(
            resize_event(&mut s, &moved(300.0), grab(), 0.0, false, at(300.0), t0),
            None
        );
    }

    #[test]
    fn nothing_starts_while_modal() {
        let t0 = Instant::now();
        let mut s = ResizeState::default();
        assert_eq!(
            resize_event(&mut s, &press(), grab(), 0.0, true, at(222.0), t0),
            None
        );
        assert!(!s.dragging);
    }

    #[test]
    fn the_drag_ends_without_a_release() {
        let t0 = Instant::now();
        for end in [
            Event::Mouse(mouse::Event::CursorLeft),
            Event::Window(window::Event::Unfocused),
        ] {
            let mut s = ResizeState::default();
            let _ = resize_event(&mut s, &press(), grab(), 0.0, false, at(222.0), t0);
            assert_eq!(
                resize_event(&mut s, &moved(300.0), grab(), 0.0, false, at(300.0), t0),
                Some(Out::Resize(300.0))
            );
            assert_eq!(
                resize_event(&mut s, &end, grab(), 0.0, false, at(300.0), t0),
                Some(Out::End)
            );
            assert!(!s.dragging);
        }
    }

    #[test]
    fn a_double_click_resets() {
        let t0 = Instant::now();
        let mut s = ResizeState::default();
        assert_eq!(
            resize_event(&mut s, &press(), grab(), 0.0, false, at(222.0), t0),
            None
        );
        assert_eq!(
            resize_event(&mut s, &release(), grab(), 0.0, false, at(222.0), t0),
            Some(Out::End)
        );
        let t1 = t0 + Duration::from_millis(200);
        assert_eq!(
            resize_event(&mut s, &press(), grab(), 0.0, false, at(222.0), t1),
            Some(Out::Reset)
        );
        assert!(!s.dragging);
        // Too late for a double-click: a plain new drag.
        let mut s = ResizeState::default();
        let _ = resize_event(&mut s, &press(), grab(), 0.0, false, at(222.0), t0);
        let _ = resize_event(&mut s, &release(), grab(), 0.0, false, at(222.0), t0);
        let t2 = t0 + Duration::from_millis(500);
        assert_eq!(
            resize_event(&mut s, &press(), grab(), 0.0, false, at(222.0), t2),
            None
        );
        assert!(s.dragging);
    }
}
