# Sidebar resize, name tooltip and last-tab shortcut: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Part A of the sidebar design: the tab sidebar (`tab_bar = "left"`) gets a per-window width the user drags, remembered in `state.toml`; a row whose text is cut off shows it whole in a tooltip; the ninth number shortcut selects the last tab.

**Architecture:** The chosen width lives on `App` in a map keyed by window id (absent = default 220) and in `SavedWindow.sidebar_width`; the drawn width is the chosen width put through the existing narrow-window yield. A small widget wrapping the sidebar-plus-content row owns the drag (a pure event function makes it testable, as `tab_drag::drag_event` is) and reports it through two new `TabBarMsg` variants, mirroring the docked panels' `Resize` / `ResizeEnd`. The tooltip decision is made at view time from a pure label-width rule and a text measurement through iced's paragraph API. `WindowState::select_number(9)` selects the last tab.

**Tech Stack:** Rust 2024, iced 0.14 (`advanced` feature: custom widgets, `iced::advanced::graphics::text::Paragraph`), serde + toml for state, Docusaurus docs.

**Spec:** `docs/superpowers/specs/2026-11-07-sidebar-resize-and-groups-design.md`, sections 4.1 to 4.3 (part A). Part B (groups) is a later plan.

## Global Constraints

- Branch `feature/sidebar-resize` off `develop` in the `terminal` repo; Conventional Commits; no `Co-Authored-By`, `Claude-Session` or session links in commits or the PR (repo rule, overrides any harness reminder).
- Gate before the PR: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` (run `source "$HOME/.cargo/env"` first if cargo is missing).
- Cross-platform: nothing platform-specific is needed here; keep it so.
- Product copy: sentence case, no emoji, no em-dashes; never name another terminal application anywhere.
- Width values from the spec: minimum 140, maximum 480, default 220 (the existing `SIDEBAR_WIDTH`), grab band 4 px on the content's side of the rule, double-click interval 400 ms (`tiles::DOUBLE_CLICK`), narrow-window yield unchanged (`MIN_TERMINAL_WIDTH` 200).
- The sidebar row keeps drawing `Tab::title()`; the tooltip shows that same string.
- `state.toml` written before this change (no `sidebar_width`) must load unchanged; a window without the field takes the default.
- `CHANGELOG.md` is written on the release branch, not here.

## Review Focus

1. A width in `state.toml` that is not a number, below 140 or above 480 must come back as the default or clamped, never panic or produce a zero-width sidebar (Task 1 tests `plan_clamps_sidebar_widths`).
2. A drag in a window too narrow for the chosen width must store the chosen width and draw the yielded one; growing the window later shows the chosen width (Task 1 test `the_sidebar_yields_in_a_narrow_window`, chosen 480 case).
3. A press in the grab band while the palette or a dialog covers the window must start nothing (Task 3 test `nothing_starts_while_modal`).
4. The pointer leaving the window or the window losing focus mid-drag must end the drag at the width reached and save it (Task 3 test `the_drag_ends_without_a_release`, Task 2 test `resize_end_saves`).
5. Widths must survive the tabs going to the top and back, and a relaunch in between (Task 1 test `snapshot_keeps_sidebar_widths_with_tabs_on_top`).

---

### Task 1: The per-window width: constants, clamp, state field, restore

**Files:**
- Modify: `crates/fbterm/src/tab_sidebar.rs:25-40` (constants, `sidebar_width`), tests at `:534-538`
- Modify: `crates/fbterm-config/src/state.rs:14-22` (`SavedWindow`)
- Modify: `crates/fbterm/src/restore.rs:24-50` (`plan`), `:217-245` (`snapshot`), tests `:278-283`, `:308-313`, `:414-435`, `:437-460`
- Modify: `crates/fbterm/src/app.rs:276-278` (fields), `:689-690` (init), `:1213-1216` (`WindowClosed` cleanup)
- Modify: `crates/fbterm/src/windows.rs:84-105` (`restore_windows`), `:321-333` (`save_state`)
- Modify: `crates/fbterm/src/window.rs:176-180` (drawn width from the chosen width)

**Interfaces:**
- Produces, in `tab_sidebar.rs`: `pub(crate) const MIN_SIDEBAR_WIDTH: f32 = 140.0;`, `pub(crate) const MAX_SIDEBAR_WIDTH: f32 = 480.0;` (keep `SIDEBAR_WIDTH = 220.0` as the default), `pub(crate) fn clamp_sidebar_width(width: f32) -> f32` (non-finite → `SIDEBAR_WIDTH`, else clamped to the range), and `pub(crate) fn sidebar_width(chosen: f32, total: f32, hairline: f32) -> f32` (the old function with `chosen` in place of the constant).
- Produces, in `state.rs`: `pub sidebar_width: Option<f32>` on `SavedWindow`, `#[serde(default, skip_serializing_if = "Option::is_none")]`.
- Produces, on `App`: `pub(crate) sidebar_widths: HashMap<window::Id, f32>` (chosen widths; absent = default) and `pub(crate) fn sidebar_width_of(&self, id: window::Id) -> f32` (the chosen width, `SIDEBAR_WIDTH` when absent).
- Produces, in `restore.rs`: `snapshot(windows, sizes, sidebar_widths: &HashMap<window::Id, f32>, recency)`; `plan` clamps each `sidebar_width` through `clamp_sidebar_width` and keeps `None` as `None`.
- Consumed by Tasks 2 to 4.

- [ ] **Step 1: Write the failing tests**

In `tab_sidebar.rs` tests, replace `the_sidebar_yields_in_a_narrow_window` and add the clamp test:

```rust
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
```

In `restore.rs` tests (the `saved` and `split_window` helpers gain `sidebar_width: None`):

```rust
#[test]
fn plan_clamps_sidebar_widths() {
    let a = ProfileId::new();
    let mut w = saved(vec![a], 0);
    w.sidebar_width = Some(f32::NAN);
    let state = SavedState {
        windows: vec![
            w,
            SavedWindow { sidebar_width: Some(10.0), ..saved(vec![a], 0) },
            SavedWindow { sidebar_width: Some(5000.0), ..saved(vec![a], 0) },
            SavedWindow { sidebar_width: Some(300.0), ..saved(vec![a], 0) },
            saved(vec![a], 0),
        ],
        ..Default::default()
    };
    let plan = plan(&state, &store(&[a]));
    assert_eq!(plan[0].sidebar_width, Some(SIDEBAR_WIDTH));
    assert_eq!(plan[1].sidebar_width, Some(MIN_SIDEBAR_WIDTH));
    assert_eq!(plan[2].sidebar_width, Some(MAX_SIDEBAR_WIDTH));
    assert_eq!(plan[3].sidebar_width, Some(300.0));
    assert_eq!(plan[4].sidebar_width, None, "an old file has no width");
}

#[test]
fn snapshot_keeps_sidebar_widths_with_tabs_on_top() {
    // Extend `snapshot_records_windows_tabs_active_sizes_and_recency`'s setup: w1 has a
    // width, w2 none. `snapshot` has no idea where the tabs are, so a width is written
    // whatever `tab_bar` says.
    let widths = HashMap::from([(w1, 300.0)]);
    let state = snapshot(&windows, &sizes, &widths, &recency);
    assert_eq!(state.windows[0].sidebar_width, Some(300.0));
    assert_eq!(state.windows[1].sidebar_width, None, "the default is not written");
}
```

In `restore.rs` tests, a TOML round trip through `SavedState` (serde): a saved window with `sidebar_width = 300.0` loads with `Some(300.0)`; the 0.7.0 shape without the key loads with `None`; a window with `None` serializes without the key (assert the string does not contain `sidebar_width`).

In `windows.rs` tests (next to `reorder_tab_moves_it_to_the_gap_and_shows_it`): `restore_windows_sets_sidebar_widths`: build an `App` as the restore tests do, call `restore_windows` with one `SavedWindow` carrying `sidebar_width: Some(300.0)` and one with `None`; assert `app.sidebar_width_of(first) == 300.0` and `app.sidebar_width_of(second) == SIDEBAR_WIDTH`, and `app.sidebar_widths.len() == 1`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p fbterm sidebar_width -- --nocapture` and `cargo test -p fbterm restore::tests`
Expected: compile errors (`clamp_sidebar_width`, `sidebar_width` arity, `sidebar_width` field, `snapshot` arity).

- [ ] **Step 3: Implement**

- `tab_sidebar.rs`: the two constants, `clamp_sidebar_width`, `sidebar_width(chosen, total, hairline)` as `chosen.min(total - MIN_TERMINAL_WIDTH - hairline).max(0.0)`. Update the module doc line about the width.
- `state.rs`: the field with its doc comment (`The sidebar's chosen width in logical pixels, when the user changed it.`).
- `restore.rs`: `plan` maps `sidebar_width.map(clamp_sidebar_width)`; `snapshot` takes `sidebar_widths` and writes `sidebar_widths.get(id).copied().filter(|w| *w != SIDEBAR_WIDTH)`.
- `app.rs`: the field, its init, `sidebar_width_of`, and `self.sidebar_widths.remove(&id)` beside the `sidebar_scroll.remove` in `WindowClosed`.
- `windows.rs`: `restore_windows` inserts `saved.sidebar_width` into the map when `Some`; `save_state` passes `&self.sidebar_widths`.
- `window.rs:178-179`: `crate::tab_sidebar::sidebar_width(app.sidebar_width_of(id), total, hairline)`; the `map_or(SIDEBAR_WIDTH, ...)` fallback for a window without a size stays.
- Fix every other caller (`grep -rn "sidebar_width(\|snapshot(" crates/fbterm/src`).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p fbterm` and `cargo test -p fbterm-config`
Expected: all pass, including the existing sidebar and restore tests.

- [ ] **Step 5: Commit**

```bash
git add crates/fbterm/src/tab_sidebar.rs crates/fbterm-config/src/state.rs crates/fbterm/src/restore.rs crates/fbterm/src/app.rs crates/fbterm/src/windows.rs crates/fbterm/src/window.rs
git commit -m "feat(sidebar): keep a chosen width per window in state"
```

---

### Task 2: The resize messages

**Files:**
- Modify: `crates/fbterm/src/app.rs:48-68` (`TabBarMsg`), `:1183-1199` (handling)
- Modify: `crates/fbterm/src/tab_sidebar.rs` (an `impl App` block next to `sidebar_scrolled`, tests next to `app_with_tabs`)

**Interfaces:**
- Consumes: `clamp_sidebar_width`, `App::sidebar_widths`, `App::sidebar_width_of` (Task 1).
- Produces: `TabBarMsg::SidebarResize(f32)` (a drag's new chosen width, window logical pixels, unclamped) and `TabBarMsg::SidebarResizeEnd` (the drag ended, or a double-click reset was issued); `pub(crate) sidebar_resizing: HashSet<window::Id>` on `App` (windows with a drag on, read by the tooltip rule in Task 4); `App::sidebar_resize(&mut self, wid, width: f32)` and `App::sidebar_resize_end(&mut self, wid) -> Task<Message>`.
- Consumed by Tasks 3 and 4.

- [ ] **Step 1: Write the failing tests**

In `tab_sidebar.rs` tests, using `app_with_tabs`:

```rust
#[test]
fn resizing_sets_the_chosen_width_clamped_per_window() {
    let (mut app, wid) = app_with_tabs(&["a"]);
    let other = window::Id::unique();
    let _ = app.update(Message::TabBar(wid, TabBarMsg::SidebarResize(300.0)));
    assert_eq!(app.sidebar_width_of(wid), 300.0);
    assert_eq!(app.sidebar_width_of(other), SIDEBAR_WIDTH, "another window keeps its own");
    assert!(app.sidebar_resizing.contains(&wid));
    let _ = app.update(Message::TabBar(wid, TabBarMsg::SidebarResize(5000.0)));
    assert_eq!(app.sidebar_width_of(wid), MAX_SIDEBAR_WIDTH);
    let _ = app.update(Message::TabBar(wid, TabBarMsg::SidebarResize(1.0)));
    assert_eq!(app.sidebar_width_of(wid), MIN_SIDEBAR_WIDTH);
}

#[test]
fn resize_end_saves() {
    // `app_with_tabs` turns state saving off; point `app.paths` at a temp dir, turn
    // `state_saving_enabled` on (as `file_panel/tests.rs` does around its Resize tests),
    // resize to 300, send SidebarResizeEnd, then load the state file:
    // `SavedState::load_or_default(&app.paths.state()).windows[0].sidebar_width == Some(300.0)`
    // and `!app.sidebar_resizing.contains(&wid)`.
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p fbterm tab_sidebar::tests::resiz`
Expected: compile errors on the new variants.

- [ ] **Step 3: Implement**

- `TabBarMsg::SidebarResize(f32)` and `TabBarMsg::SidebarResizeEnd` with doc comments; in the `Message::TabBar` arm: `SidebarResize(w) => { self.sidebar_resize(wid, w); Task::none() }`, `SidebarResizeEnd => self.sidebar_resize_end(wid)`.
- `sidebar_resize`: insert `clamp_sidebar_width(width)` into `sidebar_widths`, insert `wid` into `sidebar_resizing`.
- `sidebar_resize_end`: remove from `sidebar_resizing`, call `self.save_state()` (as `FilePanelMsg::ResizeEnd` does), return `Task::none()`.
- `WindowClosed` also removes the id from `sidebar_resizing`.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p fbterm tab_sidebar`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/fbterm/src/app.rs crates/fbterm/src/tab_sidebar.rs
git commit -m "feat(sidebar): resize messages with clamp and save on release"
```

---

### Task 3: The drag handle

**Files:**
- Create: `crates/fbterm/src/sidebar_resize.rs` (the pure event function, its state and the wrapping widget; `mod sidebar_resize;` in `main.rs` beside `mod tab_sidebar;`)
- Modify: `crates/fbterm/src/window.rs:301-323` (wrap the sidebar-plus-content row)
- Modify: `crates/fbterm/src/tiles.rs:32` (`DOUBLE_CLICK` becomes `pub(crate)`)

**Interfaces:**
- Consumes: `TabBarMsg::SidebarResize`, `TabBarMsg::SidebarResizeEnd` (Task 2); `tiles::cancels_drag`, `tiles::DOUBLE_CLICK`; `Tokens::accent_edge`.
- Produces, in `sidebar_resize.rs`:
  - `pub(crate) const GRAB: f32 = 4.0;`
  - `#[derive(Default)] pub(crate) struct ResizeState { dragging: bool, last_press: Option<Instant> }`
  - `#[derive(Debug, PartialEq)] pub(crate) enum Out { Resize(f32), End, Reset }` (`Reset` = double-click: the caller publishes `SidebarResize(SIDEBAR_WIDTH)` then `SidebarResizeEnd`)
  - `pub(crate) fn resize_event(state: &mut ResizeState, event: &Event, grab: Rectangle, origin_x: f32, modal: bool, cursor: mouse::Cursor, now: Instant) -> Option<Out>` where `grab` is the handle's rectangle in window coordinates and `origin_x` the sidebar's left edge (the chosen width is `pointer.x - origin_x`).
  - `pub(crate) fn resizable<'a>(window: window::Id, content: Element<'a, Message>, drawn_width: f32, hairline: f32, modal: bool, tokens: &'static Tokens) -> Element<'a, Message>`: a widget with one child (`content`, the `row![sidebar, docked_area]`), whose grab rectangle is `x = bounds.x + drawn_width`, `width = hairline + GRAB`, full height; it publishes the messages, shows `mouse::Interaction::ResizingHorizontally` over the grab zone or while dragging, and while dragging draws a quad of `tokens.accent_edge` over the rule (`x = bounds.x + drawn_width`, `width = hairline`). It captures the press, moves and release it consumes so the tiles under the band never see them. Pass-through of `children`, `diff`, `layout`, `operate`, `draw`, `overlay` as `Rows` in `tab_sidebar.rs` does.

- [ ] **Step 1: Write the failing tests** (in `sidebar_resize.rs`, `#[cfg(test)]`)

Helpers: `grab()` = `Rectangle { x: 220.0, y: 0.0, width: 5.0, height: 600.0 }`, `origin_x = 0.0`, `at(x)` = `mouse::Cursor::Available(Point::new(x, 100.0))`, `press()` / `release()` = left button events, `moved(x)` = `CursorMoved { position: Point::new(x, 100.0) }`, `t0 = Instant::now()`.

```rust
#[test]
fn a_press_in_the_grab_zone_starts_a_drag_and_moves_resize() {
    let mut s = ResizeState::default();
    assert_eq!(resize_event(&mut s, &press(), grab(), 0.0, false, at(222.0), t0), None);
    assert!(s.dragging);
    assert_eq!(resize_event(&mut s, &moved(300.0), grab(), 0.0, false, at(300.0), t0), Some(Out::Resize(300.0)));
    assert_eq!(resize_event(&mut s, &release(), grab(), 0.0, false, at(300.0), t0), Some(Out::End));
    assert!(!s.dragging);
}

#[test]
fn the_width_is_measured_from_the_sidebars_left_edge() {
    // origin_x = 50.0, pointer at 300.0: Resize(250.0)
}

#[test]
fn a_press_outside_the_zone_does_nothing() {
    // at(100.0) and at(240.0): None, !dragging; a later move: None
}

#[test]
fn nothing_starts_while_modal() {
    // modal = true, press at 222.0: None, !dragging
}

#[test]
fn the_drag_ends_without_a_release() {
    // press, move to 300 (Resize), then Event::Mouse(CursorLeft): Some(Out::End), !dragging;
    // again with Event::Window(window::Event::Unfocused)
}

#[test]
fn a_double_click_resets() {
    // press at t0 (None), release at t0 (End), press at t0 + 200 ms: Some(Out::Reset), !dragging;
    // a press at t0 + 500 ms after a first press: None (a plain new drag)
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p fbterm sidebar_resize`
Expected: compile error, module missing.

- [ ] **Step 3: Implement `resize_event` and `resizable`**

`resize_event` decisions: a left press with the cursor over `grab` and not `modal`: if `last_press` is within `DOUBLE_CLICK`, clear it and return `Reset` without starting a drag; else record `now`, set `dragging`, return `None`. `CursorMoved` while dragging: `Resize(position.x - origin_x)`. Left release while dragging: clear, `End`. `cancels_drag(event)` while dragging: clear, `End`. Everything else `None`.

The widget: `update` calls `resize_event` with `grab` from its layout bounds and the drawn width, `origin_x = bounds.x`, publishes (`Reset` → `SidebarResize(SIDEBAR_WIDTH)` then `SidebarResizeEnd`), calls `shell.capture_event()` when it returned `Some` or started a drag, and otherwise forwards the event to the child. `mouse_interaction` and `draw` as in Interfaces. In `window.rs`, the `Some(width)` arm wraps `row![...]` with `resizable(id, row.into(), width, hairline, modal, t)`.

- [ ] **Step 4: Run the tests and the app**

Run: `cargo test -p fbterm sidebar_resize` then `cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS, no warnings.

Run the app with the sidebar on, never touching real config: `FBTERM_CONFIG_DIR=$(mktemp -d) FBTERM_SECRETS=memory FBTERM_TAB_BAR=left cargo run -p fbterm`. Check by hand: the cursor changes over the rule, a drag resizes live and the terminal reflows, the rule is violet while dragging, the list's scrollbar still drags, a double-click on the rule snaps back to 220, dragging past 480 or below 140 stops there, and shrinking the window below the chosen width yields as before.

- [ ] **Step 5: Commit**

```bash
git add crates/fbterm/src/sidebar_resize.rs crates/fbterm/src/main.rs crates/fbterm/src/window.rs crates/fbterm/src/tiles.rs
git commit -m "feat(sidebar): drag the rule to resize, double-click to reset"
```

---

### Task 4: The tooltip on a clipped row

**Files:**
- Modify: `crates/fbterm/src/tab_sidebar.rs:143-230` (`row_view`), `:71-88` (`Sidebar` gains `tips: bool`), tests
- Modify: `crates/fbterm/src/window.rs:303-316` (fill `tips`)

**Interfaces:**
- Consumes: `tooltip::tip(t, content, label, force)`; `App::sidebar_resizing` (Task 2); `theme::UI_FONT`.
- Produces, in `tab_sidebar.rs`:
  - `pub(crate) fn label_width(sidebar_width: f32, page: bool, trouble: bool) -> f32`: the room the row's label gets, from the row's parts as `row_view` lays them out: `sidebar_width - 2 * LIST_PADDING - 2 * 8 (row padding) - 3 (stripe) - 8 - (14 if page else 7) - 8 - (7 + 8 if trouble else 0) - 18 (close: 12 icon + 3 padding each side) - 8`, never below 0.
  - `pub(crate) fn text_width(s: &str) -> f32`: the laid-out width of `s` as the row draws it, through `<iced::advanced::graphics::text::Paragraph as iced::advanced::text::Paragraph>::with_text(Text { content: s, bounds: Size::INFINITY, size: Pixels(12.5), line_height: LineHeight::default(), font: UI_FONT, align_x: Alignment::Default, align_y: Vertical::Top, shaping: Shaping::Basic, wrapping: Wrapping::None })` then `.min_width()`.
  - `pub(crate) fn clipped(text_width: f32, label_width: f32) -> bool` = `text_width > label_width`.
  - `Sidebar.tips: bool`: tooltips allowed (`carry.is_none() && !modal && !app.sidebar_resizing.contains(&id)`).

- [ ] **Step 1: Write the failing tests** (in `tab_sidebar.rs` tests)

```rust
#[test]
fn the_label_gets_what_the_rows_parts_leave() {
    assert_eq!(label_width(220.0, false, false), 220.0 - 12.0 - 16.0 - 3.0 - 8.0 - 7.0 - 8.0 - 18.0 - 8.0);
    assert_eq!(label_width(220.0, true, false), label_width(220.0, false, false) - 7.0);
    assert_eq!(label_width(220.0, false, true), label_width(220.0, false, false) - 15.0);
    assert_eq!(label_width(10.0, false, false), 0.0);
}

#[test]
fn a_row_is_clipped_when_its_text_is_wider_than_its_label() {
    assert!(clipped(150.0, 140.0));
    assert!(!clipped(140.0, 140.0));
    assert!(text_width("prod-eu-west-1-bastion-with-a-long-name") > text_width("zsh"));
    assert_eq!(text_width(""), 0.0);
}
```

And a view test using `lay_out`: `a_long_title_gets_a_tooltip_and_a_short_one_does_not` is not observable from the layout tree without digging into iced's tooltip widget; instead test the decision the view makes by extracting it into `fn row_tip(s: &Sidebar<'_>, title: &str, page: bool, trouble: bool) -> Option<String>` (returns `Some(title)` when `s.tips && clipped(text_width(title), label_width(s.width, page, trouble))`) and assert: a 40-character title at width 140 → `Some`, `"zsh"` at 220 → `None`, the long one with `tips: false` → `None`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p fbterm tab_sidebar::tests`
Expected: compile errors on the new functions.

- [ ] **Step 3: Implement**

`label_width`, `text_width`, `clipped`, `row_tip`; in `row_view`, after building the row `button`, wrap it: `match row_tip(s, tab.title(), tab.page.is_some(), tab.trouble()) { Some(label) => tip(t, button, label, false), None => button.into() }`. `window.rs` fills `tips`. The module doc of `tab_sidebar.rs` gets one line on the tooltip rule.

- [ ] **Step 4: Run the tests and the app**

Run: `cargo test -p fbterm tab_sidebar` and `cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS.

Run the app as in Task 3 with a profile whose name is longer than the sidebar (add one to `$FBTERM_CONFIG_DIR/profiles.toml` or use `FBTERM_OPEN_TABS`), hover its row: the full title shows in the usual tooltip; a short row shows none; narrow the sidebar until a short name clips and it gains a tooltip; no tooltip appears while dragging the rule or carrying a tab.

- [ ] **Step 5: Commit**

```bash
git add crates/fbterm/src/tab_sidebar.rs crates/fbterm/src/window.rs
git commit -m "feat(sidebar): show a clipped row's whole title as a tooltip"
```

---

### Task 5: The ninth number shortcut selects the last tab

**Files:**
- Modify: `crates/fbterm/src/tabs.rs:473-479` (`select_number`), test `:905-918`
- Modify: `crates/fbterm-config/src/keys.rs:247` (title), tests near `:1600`
- Modify: `website/docs/keyboard-shortcuts.md:44`, `:205`

**Interfaces:**
- Produces: `WindowState::select_number(9)` selects the last tab (returns `false` only with no tabs); `Action::SelectTab(9).title() == "Last tab"`; names unchanged.

- [ ] **Step 1: Write the failing tests**

In `tabs.rs`, extend `next_prev_wrap_and_numbers_select`: replace the `"no ninth tab"` assertion with `assert!(w.select_number(9)); assert_eq!(w.active, 2, "nine is the last tab");` and add `let mut many = window(&["1","2","3","4","5","6","7","8","9","10","11","12","13"], 0); assert!(many.select_number(9)); assert_eq!(many.active, 12); assert!(many.select_number(8)); assert_eq!(many.active, 7); assert!(!WindowState::new().select_number(9));`.

In `keys.rs` tests (next to the `Close tab or pane` title assertion): `assert_eq!(Action::SelectTab(9).title(), "Last tab"); assert_eq!(Action::SelectTab(8).title(), "Tab 8"); assert_eq!(Action::SelectTab(9).name(), "select_tab_9");` (use the accessor the existing name test uses).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p fbterm next_prev_wrap_and_numbers_select` and `cargo test -p fbterm-config title`
Expected: FAIL on the assertions above.

- [ ] **Step 3: Implement**

`select_number`: `9` → `self.tabs.len().checked_sub(1).map_or(false, |i| self.select(i))`; `1..=8` as before; `0` → `false`. Title: `Self::SelectTab(9) => "Last tab".into()` before the `format!("Tab {n}")` arm. Doc comment on `select_number` updated. Docs: `keyboard-shortcuts.md` row 44 becomes two rows, `Select tab 1 to 8` (`Cmd+1` to `Cmd+8` / `Ctrl+Shift+1` to `Ctrl+Shift+8`) and `Last tab` (`Cmd+9` / `Ctrl+Shift+9`); row 205 likewise (`select_tab_1` to `select_tab_8`, and `select_tab_9` as `Last tab`).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --workspace`
Expected: PASS. Then `cd website && npm run build` builds (link check).

- [ ] **Step 5: Commit**

```bash
git add crates/fbterm/src/tabs.rs crates/fbterm-config/src/keys.rs website/docs/keyboard-shortcuts.md
git commit -m "feat(tabs): the ninth number shortcut selects the last tab"
```

---

### Task 6: Docs, CLAUDE.md and the gate

**Files:**
- Modify: `website/docs/tabs.md:19` (the sidebar paragraph)
- Modify: `CLAUDE.md:96` (the sidebar gotcha), the `## Status` paragraph's last sentence
- Modify: `crates/fbterm/src/tab_sidebar.rs:1-2` (module doc) if Task 4 did not already

- [ ] **Step 1: Write the docs**

`tabs.md`, after the sentence "The column scrolls when the tabs overflow...": a short paragraph: drag the column's right edge to make it narrower or wider (140 to 480 pixels), each window remembers its own width, double-click the edge to go back to the default, and a tab whose name is cut off shows it whole when you hover it. Keep the narrow-window sentence.

`CLAUDE.md:96`: in the sidebar gotcha, replace `220 px, yielding...` with: the chosen width per window (`App::sidebar_widths`, default `SIDEBAR_WIDTH` 220, clamped 140..=480, saved as `SavedWindow.sidebar_width`, dragged at the rule through `sidebar_resize.rs` and `TabBarMsg::SidebarResize` / `SidebarResizeEnd`, double-click resets), yielding in a narrow window as before; and one clause: a row whose title is wider than `label_width` gets `tip` with the title. Status: one sentence that the sidebar width per window, the clipped-title tooltip and `Last tab` (`select_tab_9`) are on `develop`, pointing at the spec.

- [ ] **Step 2: Run the full gate**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny --all-features check
(cd website && npm run build)
```
Expected: all clean.

- [ ] **Step 3: Commit**

```bash
git add website/docs/tabs.md CLAUDE.md crates/fbterm/src/tab_sidebar.rs
git commit -m "docs: sidebar width, clipped-title tooltip and the last-tab shortcut"
```

- [ ] **Step 4: Open the PR** to `develop` titled `feat(sidebar): per-window width, title tooltip, last-tab shortcut`, body listing the three behaviours and the spec path, no attribution footer.
