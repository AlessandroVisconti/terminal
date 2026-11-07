# Vertical tabs, tab menu and Welcome tab: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Tabs listed down the left side of the window behind a `tab_bar` setting, a right-click menu on tabs with `Copy password` and `Close tab`, and a Welcome page tab that asks the first-launch questions (tab layout, update checks) instead of the update popup.

**Architecture:** The tab bar's press, drag and gap logic moves into a small axis-generic helper (`tab_drag.rs`) that both the existing bar and the new sidebar widget use; the sidebar is a scrollable column of rows beside the content in `window.rs`. The terminal's right-click menu (`context_menu.rs`) is generalized to a target (pane or tab) and an item list so the tab menu reuses its layer, placement and keyboard driving; the password copy lives in `tab_menu.rs` on the blocking pool. The Welcome tab is a third `Page` rendered like the Settings page, writing settings through one shared `App::set_setting`.

**Tech Stack:** Rust 2024, iced (custom widgets, `scrollable`, `clipboard`), serde/toml, `zeroize`, the workspace's `fbterm-config` and `fbterm-secrets` crates.

**Spec:** `docs/superpowers/specs/2026-11-07-vertical-tabs-and-tab-menu-design.md`

## Global Constraints

- Never name or compare to another terminal application anywhere committed (code, comments, docs, commits).
- Builds and tests on macOS, Linux and Windows; platform APIs only behind `cfg` with an alternative.
- Product copy: no emoji, no em-dashes; Graphite tokens (`crate::theme::Tokens`), Lucide icons via `ui::icon`, `UI_FONT` for labels.
- Never hardcode a chord label: tooltips, palette and docs read `Bindings` / `keys.rs`.
- Secrets travel as `Zeroizing<String>` through `Delivered`, are never logged, never shown.
- Nothing a remote program sends opens a menu, copies a password, changes a setting or opens the Welcome tab.
- No periodic subscription or redraw loop; the clipboard clear is one `Task` with a sleep.
- Conventional Commits, no `Co-Authored-By` or session links. Gate before the PR: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo deny --all-features check`.
- Run `source "$HOME/.cargo/env"` first if cargo is not on PATH. All commands below run from the repo root (`terminal/`).

## Review Focus

1. A tab whose profile was deleted (or a local or key profile) right-clicked: the menu shows `Close tab` only, never a `Copy password` that would fail. Test in Task 5.
2. The tab menu open on a tab that is then closed, merged or carried to another window: the menu closes rather than acting on a stale tab. Test in Task 5.
3. A window at the 360 px minimum with tabs on the left: the terminal keeps 200 px and the sidebar yields; with a panel docked too, nothing goes negative. Test in Task 7.
4. A second `Copy password` within the minute: the first clear check finds a different value and leaves it. Test in Task 6.
5. The `Vertical tabs` chord pressed while the Welcome tab is the active tab: the setting flips and the Welcome tab's own cards show the new choice. Test in Task 8.

---

### Task 1: The `tab_bar` setting

**Files:**
- Modify: `crates/fbterm-config/src/settings.rs` (enum next to `WindowDecorations`, field in `Settings`, template after the `window_decorations` block)
- Modify: `crates/fbterm-config/src/settings_edit.rs` (`SettingChange`, `key`, `apply`, `value`)
- Modify: `crates/fbterm-config/src/lib.rs` (re-export `TabBar` beside `WindowDecorations`)
- Test: both files' `mod tests`

**Interfaces:**
- Produces: `pub enum TabBar { Top, Left }` (`Clone, Copy, PartialEq, Eq, Debug, Default` with `#[default] Top`, `serde(rename_all = "snake_case")`), `TabBar::as_str(self) -> &'static str` (`"top"` / `"left"`); `Settings.tab_bar: Option<TabBar>` with `#[serde(skip_serializing_if = "Option::is_none")]`, `None` by default; `Settings::tabs_left(&self) -> bool` (`Some(Left)`); `SettingChange::TabBar(TabBar)` with key `"tab_bar"`, applying as `Some(v)`, writing the string value.

- [ ] **Step 1: Write the failing tests**

In `settings.rs` tests:

```rust
#[test]
fn tab_bar_is_absent_until_answered_and_parses_both_sides() {
    assert_eq!(Settings::default().tab_bar, None);
    assert!(!Settings::default().tabs_left());
    let s: Settings = toml::from_str("tab_bar = \"left\"\n").unwrap();
    assert_eq!(s.tab_bar, Some(TabBar::Left));
    assert!(s.tabs_left());
    let s: Settings = toml::from_str("tab_bar = \"top\"\n").unwrap();
    assert_eq!(s.tab_bar, Some(TabBar::Top));
    assert!(toml::from_str::<Settings>("tab_bar = \"right\"\n").is_err());
    assert!(SETTINGS_TEMPLATE.contains("#tab_bar = \"top\"\n"));
}
```

In `settings_edit.rs` tests, modelled on `window_decorations_defaults_and_writes`:

```rust
#[test]
fn tab_bar_change_writes_under_its_template_line() {
    for (v, name) in [(TabBar::Left, "left"), (TabBar::Top, "top")] {
        let change = SettingChange::TabBar(v);
        assert_eq!(change.key(), "tab_bar");
        let after = edit_text(SETTINGS_TEMPLATE, &change).unwrap();
        assert!(after.contains(&format!("#tab_bar = \"top\"\ntab_bar = \"{name}\"\n")), "{after}");
        let mut s = Settings::default();
        change.apply(&mut s);
        assert_eq!(s.tab_bar, Some(v));
    }
}
```

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p fbterm-config tab_bar`
Expected: compile errors (`TabBar`, `tabs_left`, `SettingChange::TabBar` missing).

- [ ] **Step 3: Implement**

Template text (three comment lines then the key), placed right after the `window_decorations` block:

```
# Where open sessions are listed: "top" (a row of tabs along the top of the window) or
# "left" (a column down the left side, scrolling when it overflows). Left out, the Welcome
# tab asks once.
#tab_bar = "top"
```

- [ ] **Step 4: Run the crate's tests**

Run: `cargo test -p fbterm-config`
Expected: all pass (an existing test may assert the full template or the `Settings` field list; update it).

- [ ] **Step 5: Commit**

```bash
git add crates/fbterm-config
git commit -m "feat(config): tab_bar setting, absent until answered"
```

---

### Task 2: The `Vertical tabs` action

**Files:**
- Modify: `crates/fbterm-config/src/keys.rs` (`Action` enum, `ALL` (45 to 46), `name`, `from_name` if it is a separate table, `title`, `group`, `default_chord`)
- Modify: `crates/fbterm/src/palette.rs` (`actions()`: `(Action::VerticalTabs, "Vertical tabs")` after `FilePanel`)
- Modify: `crates/fbterm/src/shortcuts.rs` (`Mode::Settings` and `Mode::Editor` arms list `Action::VerticalTabs` with the `Tunnels` group)
- Modify: `crates/fbterm/src/actions.rs` (`Action::VerticalTabs => self.toggle_vertical_tabs()`, which Task 3 defines; until then return `Task::none()` with a `// Task 3` note, or do Tasks 2 and 3 in one commit)
- Test: `keys.rs` tests (modelled on the `tunnels` test near line 1620), `palette.rs` tests, `shortcuts.rs` tests

**Interfaces:**
- Produces: `Action::VerticalTabs`, name `"vertical_tabs"`, title `"Vertical tabs"`, `ActionGroup::Windows`, default chord `('b', true)` (Cmd+Shift+B on macOS, Ctrl+Shift+B elsewhere, through the existing tuple mapping).

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn vertical_tabs_action_has_a_name_a_group_and_a_free_chord() {
    let a = Action::VerticalTabs;
    assert_eq!(a.name(), "vertical_tabs");
    assert_eq!(Action::from_name("vertical_tabs"), Some(a));
    assert_eq!(a.title(), "Vertical tabs");
    assert_eq!(a.group(), ActionGroup::Windows);
    assert_eq!(default_chord(true, a), Some(c("cmd+shift+b")));
    assert_eq!(default_chord(false, a), Some(c("ctrl+shift+b")));
    assert!(Action::ALL.contains(&a));
}
```

(`c(..)` is the chord-parsing test helper the file already uses; adapt its name.) In `palette.rs` tests: the actions list contains an `Action::VerticalTabs` row labelled `"Vertical tabs"`. In `shortcuts.rs` tests, next to `the_settings_and_editor_windows_close_on_close_tab_and_reach_each_other`: `press(Mode::Settings, "b", true)` and `press(Mode::Editor, "b", true)` give `Some(Some(Message::Shortcut(_, Action::VerticalTabs)))`.

- [ ] **Step 2: Run them to see them fail**

Run: `cargo test -p fbterm-config vertical_tabs && cargo test -p fbterm vertical_tabs`
Expected: compile errors on `Action::VerticalTabs`.

- [ ] **Step 3: Implement**, including any existing test that counts `Action::ALL` or lists every default chord (the "no two actions share a default chord" test must still pass: Cmd+Shift+B and Ctrl+Shift+B are unused today).

- [ ] **Step 4: Run the tests**

Run: `cargo test -p fbterm-config && cargo test -p fbterm keys && cargo test -p fbterm palette && cargo test -p fbterm shortcuts`
Expected: pass.

- [ ] **Step 5: Commit** (with Task 3 if the dispatch arm needs it)

```bash
git commit -am "feat(keys): Vertical tabs action with Cmd+Shift+B / Ctrl+Shift+B"
```

---

### Task 3: Changing the setting from the action and the Settings tab

**Files:**
- Modify: `crates/fbterm/src/settings_window/glue.rs` (new `App::set_setting`, extracted from `update.rs::answer_update_question`)
- Modify: `crates/fbterm/src/update.rs` (`answer_update_question` calls `set_setting`; it is deleted in Task 8)
- Create: `crates/fbterm/src/tab_sidebar.rs` (for now only `App::toggle_vertical_tabs`; Task 7 fills it), registered in `main.rs`/`lib.rs` module list
- Modify: `crates/fbterm/src/settings_window/mod.rs` (`SettingsMsg::TabBar(TabBar)` mapped to `SettingChange::TabBar`), `settings_window/view.rs` (`Tabs` row in `appearance`, after the blur row, before the Title bar row, on every system)
- Test: `crates/fbterm/src/settings_window/app_tests.rs`, `settings_window/tests.rs`

**Interfaces:**
- Produces: `App::set_setting(&mut self, change: SettingChange) -> Task<Message>`: writes the field to `settings.toml` (`Settings::write_field`, warn on error), applies the change to the open Settings tab's `values`, applies to the app (`apply_settings(next, SettingsSource::Window)`), and if the file changed under us meanwhile (the `settings_seen` comparison) chains `settings_file_changed`; otherwise refreshes `settings_seen`. `App::toggle_vertical_tabs(&mut self) -> Task<Message>`: `set_setting(SettingChange::TabBar(if self.settings.tabs_left() { Top } else { Left }))`. `SettingsMsg::TabBar(TabBar)`.

- [ ] **Step 1: Write the failing tests**

In `app_tests.rs`:

```rust
#[test]
fn the_tabs_row_and_the_action_flip_the_side_and_write_it() {
    let (mut app, paths) = app();
    send(&mut app, SettingsMsg::TabBar(TabBar::Left));
    assert!(app.settings.tabs_left());
    assert!(read(&paths).unwrap().contains("#tab_bar = \"top\"\ntab_bar = \"left\"\n"));
    let _ = app.action(None, Action::VerticalTabs);
    assert_eq!(app.settings.tab_bar, Some(TabBar::Top));
    assert!(read(&paths).unwrap().contains("\ntab_bar = \"top\"\n"));
    // The open Settings tab shows the new value.
    assert_eq!(app.settings_window().unwrap().values.tab_bar, Some(TabBar::Top));
    let _ = app.action(None, Action::VerticalTabs);
    assert_eq!(app.settings.tab_bar, Some(TabBar::Left));
}
```

In `settings_window/tests.rs`, like the `WindowDecorations` pending test at line 389: `state.update(SettingsMsg::TabBar(TabBar::Left))` leaves `[SettingChange::TabBar(TabBar::Left)]` pending with `Write::Now`.

- [ ] **Step 2: Run to see them fail**: `cargo test -p fbterm the_tabs_row` (compile error).

- [ ] **Step 3: Implement.** The row: `setting(t, "Tabs", None, false, well-wrapped row of segment(t, "Top", !v.tabs_left(), ..) and segment(t, "Left", v.tabs_left(), ..))`, exactly like the Title bar row's shape.

- [ ] **Step 4: Run** `cargo test -p fbterm settings_window && cargo test -p fbterm update` — pass, including the update module's tests that still go through `answer_update_question`.

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(settings): Tabs row and Vertical tabs toggle through one set_setting path"
```

---

### Task 4: Axis-generic tab drag and the right-click hook

**Files:**
- Create: `crates/fbterm/src/tab_drag.rs` (moved from `window.rs`: `TabPress` as `Press`, `BarState` as `DragState`, `tab_gap` as `gap_at`, `BarBackground::drag_event` as a free function)
- Modify: `crates/fbterm/src/window.rs` (`BarBackground` keeps `tab_rects`, calls `tab_drag::drag_event` with `Axis::X`)
- Modify: `crates/fbterm/src/app.rs` (`TabBarMsg::Menu { index: usize, at: Point }`, dispatched to `open_tab_menu` from Task 5; until then `Task::none()`)
- Test: `tab_drag.rs` tests

**Interfaces:**
- Produces: `pub(crate) enum Axis { X, Y }`; `pub(crate) struct Press { index, origin: Point, dragging: bool }`; `#[derive(Default)] pub(crate) struct DragState { press: Option<Press>, over: Option<usize>, gap: Option<usize> }`; `pub(crate) struct Strip<'a> { window: window::Id, ids: &'a [TabId], carry: Option<Carry>, modal: bool, hole: Option<usize>, axis: Axis, scroll: f32 }` (`scroll`: how far the strip's coordinates are shifted from the window's, the sidebar's scroll offset; 0 for the bar); `pub(crate) fn gap_at(rects: &[Rectangle], own: Option<usize>, p: Point, axis: Axis) -> Option<usize>` (the bar's rule along the axis: before the one whose first half is under `p`, after the one whose second half is, else the nearest gap; nowhere over `own`); `pub(crate) fn drag_event(strip: &Strip<'_>, state: &mut DragState, event: &Event, rects: &[Rectangle], bounds: Rectangle, cursor: mouse::Cursor, shell: &mut Shell<'_, Message>) -> bool` with today's behaviour plus: `ButtonPressed(Right)` over a tab while `!modal` publishes `Message::TabBar(window, TabBarMsg::Menu { index, at: Point::new(p.x, p.y - strip.scroll) })` and returns `true`.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn gap_at_works_along_either_axis() {
    let rows = [Rectangle::new(Point::new(0.0, 0.0), Size::new(100.0, 30.0)),
                Rectangle::new(Point::new(0.0, 34.0), Size::new(100.0, 30.0))];
    assert_eq!(gap_at(&rows, None, Point::new(10.0, 5.0), Axis::Y), Some(0));
    assert_eq!(gap_at(&rows, None, Point::new(10.0, 25.0), Axis::Y), Some(1));
    assert_eq!(gap_at(&rows, None, Point::new(10.0, 60.0), Axis::Y), Some(2));
    assert_eq!(gap_at(&rows, None, Point::new(10.0, 200.0), Axis::Y), Some(2));
    assert_eq!(gap_at(&rows, Some(1), Point::new(10.0, 40.0), Axis::Y), None);
    let tabs = [Rectangle::new(Point::new(0.0, 0.0), Size::new(100.0, 30.0)),
                Rectangle::new(Point::new(102.0, 0.0), Size::new(100.0, 30.0))];
    assert_eq!(gap_at(&tabs, None, Point::new(160.0, 5.0), Axis::X), Some(2));
    assert_eq!(gap_at(&tabs, None, Point::new(120.0, 5.0), Axis::X), Some(1));
}
```

- [ ] **Step 2: Run** `cargo test -p fbterm gap_at` (fails to compile).

- [ ] **Step 3: Move the code.** Behaviour of the bar is unchanged; the existing drag tests in `windows.rs`/`tiles.rs` tests still pass.

- [ ] **Step 4: Run** `cargo test -p fbterm` — pass.

- [ ] **Step 5: Commit**

```bash
git commit -am "refactor(tabs): tab drag logic shared by axis, right-click reported as TabBarMsg::Menu"
```

---

### Task 5: The tab menu (items and Close tab)

**Files:**
- Modify: `crates/fbterm/src/context_menu.rs` (`MenuItem` gains `CopyPassword` and `CloseTab`; `ContextMenu` gains a target and an item list; `MenuItem::enabled`/`ALL` replaced by the list; `menu_size` from `items.len()`; `view` iterates `items`; `App::menu_covered` extracted from `open_context_menu`; `sync_context_menu` handles a tab target)
- Create: `crates/fbterm/src/tab_menu.rs` (`open_tab_menu`, `tab_password_profile`; Task 6 adds the copy)
- Modify: `crates/fbterm/src/app.rs` (`TabBarMsg::Menu` dispatch), `sessions.rs:368` (constructor rename)
- Test: `context_menu.rs` and `tab_menu.rs` tests (app-level, `App::build(paths_with(..), true)` with `state_saving_enabled = false`, windows inserted by hand as in `sessions.rs` tests)

**Interfaces:**
- Produces: `pub(crate) enum MenuItem { Copy, Paste, CopyPassword, CloseTab }` with titles `"Copy"`, `"Paste"`, `"Copy password"`, `"Close tab"` (`chord_label` only for Copy and Paste); `pub(crate) enum MenuTarget { Pane(PaneId), Tab(TabId) }`; `pub(crate) struct ContextMenu { window, target: MenuTarget, at: Point, items: Vec<(MenuItem, bool)>, selected: MenuItem }` with `ContextMenu::for_pane(window, pane, at, can_copy)` (`[(Copy, can_copy), (Paste, true)]`, selected as today), `ContextMenu::for_tab(window, tab, at, password: bool)` (`[(CopyPassword, true)]` when `password`, then `(CloseTab, true)`; selected = first item), `ContextMenu::enabled(&self, item) -> bool`, `step` over the enabled items. `App::menu_covered(&self, wid) -> bool` (palette, find box, prompt or vault dialog over `wid`, or no such terminal window). `App::open_tab_menu(&mut self, wid, index: usize, at: Point) -> Task<Message>`: nothing when covered or no such tab; else sets `context_menu = ContextMenu::for_tab(wid, tab.id, at, self.tab_password_profile(wid, tab.id).is_some())`, no focus or selection change. `App::tab_password_profile(&self, wid, tab: TabId) -> Option<ProfileId>`: the tab's focused pane's profile when it exists in the store, is `ProfileKind::Ssh` and its auth is `AuthMethod::Password`. In `context_menu_msg`, `Choose(CloseTab)` on a tab target: the tab's current index in the window, then `close_tab_asking(wid, index)`; `Choose(CopyPassword)`: `Message::TabMenu(TabMenuMsg::Copy(wid, tab))` (Task 6). `sync_context_menu`: a tab target closes when no tab with that id is in `menu.window`.

- [ ] **Step 1: Write the failing tests** (in `tab_menu.rs` `mod tests`; `ssh_password_profile(name)` is a small local helper building a `Profile` with `ProfileKind::Ssh` and `AuthMethod::Password`, `profile(name)` is `restore_tests::profile`, a local profile)

```rust
#[test]
fn the_menu_offers_copy_password_only_for_password_ssh_tabs() {
    let (ssh, local) = (ssh_password_profile("prod"), profile("sh"));
    let (mut app, _) = App::build(paths_with(&[ssh.clone(), local.clone()]), true);
    app.state_saving_enabled = false;
    let wid = window::Id::unique();
    let mut w = WindowState::new();
    w.push(Tab::single(Pane::for_profile(&ssh)));
    w.push(Tab::single(Pane::for_profile(&local)));
    w.push(Tab::page(Page::Settings));
    app.windows.clear();
    app.windows.insert(wid, w);
    let at = Point::new(40.0, 10.0);
    let _ = app.open_tab_menu(wid, 0, at);
    let m = app.context_menu.clone().unwrap();
    assert_eq!(m.items, vec![(MenuItem::CopyPassword, true), (MenuItem::CloseTab, true)]);
    assert_eq!(m.selected, MenuItem::CopyPassword);
    assert_eq!(m.at, at);
    assert_eq!(app.windows[&wid].active, 0, "a right-click does not select the tab");
    let _ = app.open_tab_menu(wid, 1, at);
    assert_eq!(app.context_menu.clone().unwrap().items, vec![(MenuItem::CloseTab, true)]);
    let _ = app.open_tab_menu(wid, 2, at);
    assert_eq!(app.context_menu.clone().unwrap().items, vec![(MenuItem::CloseTab, true)]);
    let _ = app.open_tab_menu(wid, 7, at);
    assert!(app.context_menu.is_some(), "an unknown index changes nothing");
}

#[test]
fn a_deleted_profile_offers_no_copy_password() {
    // Review focus 1: the pane keeps a ProfileId the store no longer has.
    let ssh = ssh_password_profile("gone");
    let (mut app, _) = App::build(paths_with(&[]), true);
    app.state_saving_enabled = false;
    let wid = window::Id::unique();
    let mut w = WindowState::new();
    w.push(Tab::single(Pane::for_profile(&ssh)));
    app.windows.clear();
    app.windows.insert(wid, w);
    let _ = app.open_tab_menu(wid, 0, Point::ORIGIN);
    assert_eq!(app.context_menu.clone().unwrap().items, vec![(MenuItem::CloseTab, true)]);
}

#[test]
fn close_tab_closes_the_right_clicked_tab_and_the_menu_follows_its_tab() {
    // Two local tabs, active 0; right-click tab 1, choose Close tab: tab 1 goes, active stays 0.
    // Then open the menu on tab 0 and close that tab through close_tab: Review focus 2, the
    // menu is gone after update's sync.
    ...
    let _ = app.update(Message::ContextMenu(wid, MenuMsg::Choose(MenuItem::CloseTab)));
    assert_eq!(app.windows[&wid].tabs.len(), 1);
    assert!(app.context_menu.is_none());
    let _ = app.open_tab_menu(wid, 0, Point::ORIGIN);
    let _ = app.update(Message::TabBar(wid, TabBarMsg::Close(0)));
    assert!(app.context_menu.is_none());
}

#[test]
fn the_tab_menu_does_not_open_under_the_palette() {
    // open_palette(wid, ..) then open_tab_menu: context_menu stays None.
}
```

Also adapt the existing `context_menu.rs` tests to `for_pane` and the item list.

- [ ] **Step 2: Run** `cargo test -p fbterm tab_menu` (compile errors).

- [ ] **Step 3: Implement.** Keep the layer widget, `menu_origin`, `item_view` and the key mapping as they are; the only `view` change is iterating `menu.items`.

- [ ] **Step 4: Run** `cargo test -p fbterm context_menu && cargo test -p fbterm tab_menu` — pass.

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(tabs): right-click menu on a tab with Close tab and a Copy password item"
```

---

### Task 6: Copy password

**Files:**
- Modify: `crates/fbterm/src/tab_menu.rs` (the copy flow), `crates/fbterm/src/app.rs` (`Message::TabMenu(TabMenuMsg)`, `App::show_notice`), `context_menu.rs` (`Choose(CopyPassword)` sends `TabMenuMsg::Copy`)
- Test: `tab_menu.rs` tests

**Interfaces:**
- Produces: `pub(crate) const CLEAR_AFTER: Duration = Duration::from_secs(60)`; `pub(crate) enum TabMenuMsg { Copy(window::Id, TabId), Read(window::Id, ProfileId, Delivered<Result<Option<Zeroizing<String>>, SecretError>>), ClearDue(Delivered<Zeroizing<String>>), ClearCheck(Delivered<Zeroizing<String>>, Option<String>) }`; `pub(crate) fn should_clear(copied: &str, clipboard: Option<&str>) -> bool` (`clipboard == Some(copied)`, so `None` and `""` never clear); `App::tab_menu_msg(&mut self, msg) -> Task<Message>`; `App::copy_password(&mut self, wid, tab: TabId) -> Task<Message>`: `tab_password_profile` else nothing; else `run_blocking(move || secrets.get(&id, SecretKind::Password).map(|o| o.map(Zeroizing::new)), |e| Err(SecretError::Backend(e)), move |r| Message::TabMenu(TabMenuMsg::Read(wid, id, Delivered::new(r))))` with `secrets = self.secrets.clone()`; `App::password_read(&mut self, wid, id, result) -> Task<Message>`: `Ok(Some(pw))` batches `iced::clipboard::write(pw.to_string())` with `Task::perform(tokio::time::sleep(CLEAR_AFTER), move |()| Message::TabMenu(TabMenuMsg::ClearDue(Delivered::new(pw))))`; `Ok(None)` shows the notice `No saved password for {name}.` (the profile's name, or the id when the profile is gone); `Err(SecretError::Locked)` asks `ask_main_password(wid, need, Message::TabMenu(TabMenuMsg::Copy(wid, tab)), None)` when the backend reports a need, else shows `Saved secrets are locked.`; other errors show `e.to_string()`. `ClearDue(pw)` returns `iced::clipboard::read().map(move |t| Message::TabMenu(TabMenuMsg::ClearCheck(pw, t)))` (clone the `Delivered` into the closure); `ClearCheck(pw, t)` writes `String::new()` to the clipboard only when `should_clear`. `App::show_notice(&mut self, line: String)` in `app.rs`: push to `notices` unless already there, `notices_dismissed = false`.
- Consumes: `App::run_blocking` (inline in tests, `blocking_inline`), `App::ask_main_password`, `Message::Vault` flow, `App::tab_password_profile` (Task 5).

Note: `Copy(wid, tab)` carries the tab, not the profile, so a retry after an unlock reads the tab's profile again (a tab closed meanwhile copies nothing).

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn should_clear_only_an_exact_match() {
    assert!(should_clear("s3cret", Some("s3cret")));
    assert!(!should_clear("s3cret", Some("s3cret ")));
    assert!(!should_clear("s3cret", Some("")));
    assert!(!should_clear("s3cret", None));
    // Review focus 4: the first copy's check after a second copy finds the second value.
    assert!(!should_clear("first", Some("second")));
}

#[test]
fn a_missing_password_says_so_in_the_banner_and_a_saved_one_does_not() {
    let ssh = ssh_password_profile("prod");
    let (mut app, _) = App::build(paths_with(std::slice::from_ref(&ssh)), true);
    app.state_saving_enabled = false;
    app.blocking_inline = true;
    // window with one tab of `ssh` as in Task 5's tests
    let tab = app.windows[&wid].tabs[0].id;
    let _ = app.update(Message::TabMenu(TabMenuMsg::Copy(wid, tab)));
    assert!(app.visible_notices().contains(&"No saved password for prod.".to_string()));
    app.secrets.set(&ssh.id, SecretKind::Password, "s3cret").unwrap();
    app.notices.clear();
    let _ = app.update(Message::TabMenu(TabMenuMsg::Copy(wid, tab)));
    assert!(app.visible_notices().is_empty());
}

#[test]
fn choosing_copy_password_from_the_menu_closes_it_and_runs_the_copy() {
    // open_tab_menu on the ssh tab, then Message::ContextMenu(wid, MenuMsg::Choose(MenuItem::CopyPassword)):
    // context_menu is None and, with no saved password, the banner line shows.
}

#[test]
fn a_locked_vault_asks_for_the_main_password_then_copies() {
    // Use App::build with the in-memory backend and FBTERM_DEMO_VAULT's path
    // (`SecretBackend::in_memory(SecretStorage::Vault)` or the test helper `debug.rs` uses for
    // `setpassword`), set a main password, lock it, then TabMenuMsg::Copy: app.vault.dialog is
    // Some with Purpose::Then whose `then` is Message::TabMenu(TabMenuMsg::Copy(wid, tab)).
}
```

- [ ] **Step 2: Run** `cargo test -p fbterm tab_menu` (compile errors).

- [ ] **Step 3: Implement.** `tracing` must never see the password; the `Debug` of `TabMenuMsg` must not print it (derive `Debug` only on variants without the secret, or implement `Debug` by hand printing `Read(..)`/`ClearDue(..)`).

- [ ] **Step 4: Run** `cargo test -p fbterm tab_menu && cargo test -p fbterm saved_secrets` — pass.

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(tabs): Copy password from the tab menu, cleared from the clipboard a minute later"
```

---

### Task 7: The sidebar

**Files:**
- Modify: `crates/fbterm/src/tab_sidebar.rs` (the view, geometry and scroll bookkeeping)
- Modify: `crates/fbterm/src/window.rs` (`TabBar.tabs_in_bar: bool`, `TabBar.outline_start: f32`; `tab_bar` skips the tab buttons when `!tabs_in_bar`; `BarBackground.outline_start` passed as `outline`'s `start`; `window_view` composes the sidebar; `outline_end`'s dock total reduced by the sidebar)
- Modify: `crates/fbterm/src/app.rs` (fields `sidebar_scroll: HashMap<window::Id, (f32, f32)>` and `sidebar_shown: HashMap<window::Id, TabId>`; `TabBarMsg::Scrolled(scrollable::Viewport)`; `reveal_active_tabs` batched in `update` next to `sync_context_menu`; both maps dropped with the window in `WindowClosed`)
- Test: `tab_sidebar.rs` tests, `window.rs` tests (a view smoke test like `tunnels_tests.rs:1460`)

**Interfaces:**
- Produces: `pub(crate) const SIDEBAR_WIDTH: f32 = 220.0; ROW_HEIGHT: f32 = 32.0; ROW_SPACING: f32 = 2.0; LIST_PADDING: f32 = 6.0;` `pub(crate) fn sidebar_width(total: f32, hairline: f32) -> f32` = `SIDEBAR_WIDTH.min(total - MIN_TERMINAL_WIDTH - hairline).max(0.0)`; `pub(crate) fn row_top(index: usize) -> f32` = `LIST_PADDING + index as f32 * (ROW_HEIGHT + ROW_SPACING)`; `pub(crate) fn reveal(index: usize, offset: f32, viewport: f32) -> Option<f32>`: `None` when `row_top(index) >= offset && row_top(index) + ROW_HEIGHT <= offset + viewport`, else the offset that shows the row with `LIST_PADDING` of margin on the side it was hidden (`row_top - LIST_PADDING` above, `row_top + ROW_HEIGHT + LIST_PADDING - viewport` below), never below 0; `pub(crate) fn scroll_id(window: window::Id) -> iced::widget::Id` (`format!("tabs-{window:?}")`); `pub(crate) struct Sidebar<'b> { window, tokens: &'static Tokens, around: Surroundings, bar_bg: Color, bindings: &'b Bindings, carry: Option<Carry>, modal: bool, scale: f32, scroll: f32, width: f32 }`; `pub(crate) fn view<'a>(s: Sidebar<'_>, tabs: &'a [Tab], active: usize) -> Element<'a, Message>`: a `row![list, ui::vertical_rule(t, scale)]` of width `s.width` plus the rule, full height, background `bar_bg`; `list` is `scrollable(Element::new(Rows { .. }))` with `.id(scroll_id)`, `.on_scroll(|v| Message::TabBar(window, TabBarMsg::Scrolled(v)))`, the app scrollbar (`ui::edge_scrollbar(false)` and `ui::scroll_style`); `Rows` is the sidebar's drag widget: it wraps the `column` of rows (spacing `ROW_SPACING`, padding `LIST_PADDING`), its `update` calls `tab_drag::drag_event` with `Axis::Y`, `scroll: s.scroll`, rects = the column's children bounds, and draws the tile gap as a 2 px accent line across the sidebar at the gap's y (the bar draws its gap in `BarBackground::draw`; match its colour). A row: `button` of `row![stripe, state dot (not for page tabs), trouble dot, title (clipped), close button]` sized `ROW_HEIGHT`, `Padding::from([0.0, 8.0])`, `width(Length::Fill)`, 5 px radius, `on_press(TabBarMsg::Select(i))`, background from `tab_look::look(t, accent, style, active, hovered, around)` (the active row's `background`, inactive hovered rows `t.hover`), close button and tooltip as `tab_button` builds them (lift the stripe, dots, label and close into small shared helpers in `window.rs` or `tab_look.rs` rather than copying them). `App::sidebar_scrolled(&mut self, wid, v: scrollable::Viewport)` stores `(v.absolute_offset().y, v.bounds().height)`. `App::reveal_active_tabs(&mut self) -> Task<Message>`: for each terminal window while `settings.tabs_left()`, when the active tab's id differs from `sidebar_shown[wid]`, record it and, when `reveal(active, offset, viewport)` (defaults `(0, f32::MAX)` before the first `Scrolled`) is `Some(y)`, `scrollable::scroll_to(scroll_id(wid), AbsoluteOffset { x: 0.0, y })`.
- Consumes: Task 4's `tab_drag`, `tab_look::look`, `ui::vertical_rule`, `MIN_TERMINAL_WIDTH`.

Window composition in `window_view`: `let sidebar = app.settings.tabs_left().then(|| sidebar_width(total, hairline))` where `total` is the window's width from `app.sizes` (fall back to `SIDEBAR_WIDTH` when unknown); `TabBar { tabs_in_bar: sidebar.is_none(), outline_start: sidebar.map_or(0.0, |w| w + hairline), .. }`; `outline_end` for a docked panel uses `total - sidebar - hairline` as the width the dock lays out in; the body pushes `row![tab_sidebar::view(..), dock(terminal_area, panel)]` when `sidebar` is `Some`, else `dock(..)` as today. `BarBackground` with `hole: None` must draw no corner pieces and no flares and start the top outline at `outline_start` (read `outline`'s `start` use before changing it: today it is the bar's left edge).

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn the_sidebar_yields_in_a_narrow_window() {
    // Review focus 3
    assert_eq!(sidebar_width(1000.0, 1.0), 220.0);
    assert_eq!(sidebar_width(360.0, 1.0), 159.0);
    assert_eq!(sidebar_width(150.0, 1.0), 0.0);
}

#[test]
fn rows_are_revealed_only_when_hidden() {
    assert_eq!(row_top(0), 6.0);
    assert_eq!(row_top(3), 6.0 + 3.0 * 34.0);
    assert_eq!(reveal(0, 0.0, 300.0), None);
    assert_eq!(reveal(2, 0.0, 300.0), None);
    assert_eq!(reveal(10, 0.0, 300.0), Some(row_top(10) + 32.0 + 6.0 - 300.0));
    assert_eq!(reveal(0, 100.0, 300.0), Some(0.0));
    assert_eq!(reveal(5, 400.0, 300.0), Some(row_top(5) - 6.0));
}

#[test]
fn switching_sides_keeps_tabs_order_and_the_active_tab_and_builds_the_view() {
    // three local tabs, active 1, state_saving_enabled false, app.sizes[wid] = (900.0, 600.0)
    let _ = app.action(None, Action::VerticalTabs);
    assert!(app.settings.tabs_left());
    assert_eq!(app.windows[&wid].active, 1);
    assert_eq!(app.windows[&wid].tabs.iter().map(|t| t.title().to_owned()).collect::<Vec<_>>(), vec!["a", "b", "c"]);
    let _element = crate::window::view(&app, wid);   // builds without panicking
    let _ = app.action(None, Action::VerticalTabs);
    let _element = crate::window::view(&app, wid);
}

#[test]
fn selecting_a_hidden_row_scrolls_the_sidebar_and_a_visible_one_does_not() {
    // 20 tabs, tabs_left, Scrolled viewport (offset 0, height 200); select tab 19 through
    // Message::SelectTab(19): sidebar_shown[wid] is tab 19's id (the reveal ran). Then
    // Scrolled (offset 500, height 200) and select tab 18: still shown-tracked, no panic.
    // (The scroll Task itself cannot be observed; the bookkeeping can.)
}
```

- [ ] **Step 2: Run** `cargo test -p fbterm tab_sidebar` (compile errors).

- [ ] **Step 3: Implement**, then look at it: `FBTERM_CONFIG_DIR=<scratch dir> FBTERM_SECRETS=memory FBTERM_TAB_BAR=left FBTERM_OPEN_TABS=<a local profile>,<same>,<same> FBTERM_SCREENSHOT=<scratch>/left.png cargo run -p fbterm` (the `FBTERM_TAB_BAR` aid is Task 9; add it now if it helps and keep it) and check: sidebar left, rule, rows, active row in the terminal's colour, bar empty of tabs with its buttons, outline from the rule to the right edge. Repeat with the file panel open (`FBTERM_DEMO_FILES`) and at 360 px width.

- [ ] **Step 4: Run** `cargo test -p fbterm` — pass (drag and restore tests unchanged).

- [ ] **Step 5: Commit**

```bash
git commit -am "feat(tabs): vertical tabs in a sidebar at the left edge"
```

---

### Task 8: The Welcome tab replacing the update popup

**Files:**
- Create: `crates/fbterm/src/welcome/mod.rs`, `crates/fbterm/src/welcome/view.rs`, `crates/fbterm/src/welcome/tests.rs`
- Modify: `crates/fbterm/src/tabs.rs` (`Page::Welcome`, title `"Welcome"`), `windows.rs` (`page_tab` becomes `pub(crate)` so the tests can find the tab), `window.rs` (`page_min_width(Welcome) => 520.0`, `page_view` arm, `key_mode` arm `(Some(Page::Welcome), _) => Mode::Welcome`), `shortcuts.rs` (`Mode::Welcome`: like `Mode::Settings` with `CloseTab` mapped to `Message::Welcome(WelcomeMsg::Close)`, plus `Action::Settings` and `Action::VerticalTabs`), `windows.rs` (`page_closed` returns `Task<Message>`; `Page::Welcome => self.welcome_closed()`), `app.rs` (`Message::Welcome(WelcomeMsg)`; `ask_welcome: bool` replaces `ask_updates`/`update_asked`; `update` no longer calls `ask_update_question`; `build` chains `open_welcome_if_due()` right after the restore and `open_debug_welcome()` with the other debug aids)
- Modify: `crates/fbterm/src/update.rs` (delete `QUESTION_TITLE`, `QUESTION_BODY` (moved), `update_question_due`, `ask_update_question`, `answer_update_question` and their tests; keep the check, dot and `demo_update`), `prompts.rs` (`Dialog::CheckUpdates`, `PromptMsg::AnswerUpdates` removed), `prompt_view.rs` (arm removed), `dialogs.rs` (two arms removed), `debug.rs` (`"updates"` removed from `FBTERM_DEMO_PROMPT`; `open_debug_welcome` for `FBTERM_OPEN_WELCOME`)
- Test: `welcome/tests.rs` (the update popup's tests move here, reworded), `shortcuts.rs` tests, `window.rs` `key_mode` test

**Interfaces:**
- Produces: `pub(crate) enum WelcomeMsg { Open, Close, ChooseTabs(TabBar), ChooseUpdates(bool) }`; `pub(crate) const TITLE: &str = "Welcome to fbterm"; INTRO: &str = "A couple of choices before you start. Everything here can be changed later in Settings."; TABS_QUESTION: &str = "Where should open sessions be listed?"; UPDATES_BODY: &str` (today's `update::QUESTION_BODY` text); `App::welcome_due(&self) -> bool`: `ask_welcome && state_saving_enabled && (settings.tab_bar.is_none() || settings.check_updates.is_none())` and (`!paths.settings().exists() || Settings::load(..).is_ok()`); `App::open_welcome_if_due(&mut self) -> Task<Message>`: when due, `ask_welcome = false` and `show_page(Page::Welcome).1`; `App::welcome_msg(&mut self, msg) -> Task<Message>`: `Open` → `show_page`, `Close` → `close_page(Page::Welcome)`, `ChooseTabs(v)` → `set_setting(SettingChange::TabBar(v))`, `ChooseUpdates(v)` → `set_setting(SettingChange::CheckUpdates(v))`; `App::welcome_closed(&mut self) -> Task<Message>`: batch of `set_setting` for each still-unanswered question (`TabBar(Top)`, `CheckUpdates(false)`). `welcome::view::view<'a>(t: &'static Tokens, settings: &Settings) -> Element<'a, Message>`: the Settings page's shape (title 18, intro 12.5 muted, sections through a local copy of `section`/`setting` or by making those two `pub(super)`-visible helpers in `settings_window::view` `pub(crate)`), section `Tabs` with `TABS_QUESTION` and `row![card(Top), card(Left)]`, section `Updates` with `UPDATES_BODY` and two `segment`s `Check automatically` / `Not now` (selected from `settings.check_updates`, none when `None`), a footer with a primary `Done` button sending `WelcomeMsg::Close`; `fn card<'a>(t, caption: &'static str, side: TabBar, selected: bool) -> Element<'a, Message>`: a `button` holding `preview(t, side)` over the caption, border `t.accent` when selected else `t.border`, 8 px radius, `on_press(WelcomeMsg::ChooseTabs(side))`; `fn preview<'a>(t, side: TabBar) -> Element<'a, Message>`: a 132 by 84 frame (`t.app` background, `t.border` 1 px, 6 px radius) with a 14 px `t.surface` bar; `Top`: three 26 by 8 pills in the bar, the first `t.accent`, the others `t.border`; `Left`: the bar empty and a 40 px `t.surface` column below it holding three 30 by 6 rows, the first `t.accent`, the others `t.border`.
- Consumes: `show_page`, `close_page`, `set_setting` (Task 3), `TabBar` (Task 1).

- [ ] **Step 1: Write the failing tests** (`welcome/tests.rs`, using `restore_tests::{paths_with, profile}`; `build_with_settings(text)` writes `text` to the temp config dir's `settings.toml` before `App::build`, as `update.rs` tests did)

```rust
#[test]
fn a_fresh_install_opens_the_welcome_tab_once_and_only_in_runs_that_keep_state() {
    let (mut app, _) = App::build(paths_with(&[profile("sh")]), true);
    app.ask_welcome = true;           // tests build with it off
    app.state_saving_enabled = true;
    let _ = app.open_welcome_if_due();
    let (wid, i) = app.page_tab(Page::Welcome).expect("welcome tab");
    assert_eq!(app.windows[&wid].active, i);
    assert!(!app.ask_welcome);
    let _ = app.open_welcome_if_due();
    assert_eq!(app.windows[&wid].tabs.iter().filter(|t| t.page == Some(Page::Welcome)).count(), 1);
    let (mut app, _) = App::build(paths_with(&[profile("sh")]), true);
    app.ask_welcome = true;
    app.state_saving_enabled = false;
    let _ = app.open_welcome_if_due();
    assert!(app.page_tab(Page::Welcome).is_none());
}

#[test]
fn answered_installs_and_unreadable_settings_do_not_get_the_tab() {
    // settings.toml with both `tab_bar = "top"` and `check_updates = false`: not due.
    // settings.toml with only one of them: due.
    // settings.toml that does not parse: not due.
}

#[test]
fn each_choice_is_written_as_chosen_and_every_window_follows() {
    // open the tab; Message::Welcome(WelcomeMsg::ChooseTabs(TabBar::Left)): app.settings.tabs_left(),
    // file contains `tab_bar = "left"`; ChooseUpdates(true): app.settings.check_updates == Some(true),
    // file contains `check_updates = true`; the tab is still open.
}

#[test]
fn closing_the_tab_writes_the_defaults_for_what_was_not_answered() {
    // open; ChooseTabs(Left) only; Message::Welcome(WelcomeMsg::Close): tab gone,
    // settings.check_updates == Some(false), settings.tab_bar == Some(Left), file has both.
    // Closing through the tab bar's close button (TabBarMsg::Close(i)) does the same.
}

#[test]
fn the_vertical_tabs_chord_acts_on_the_welcome_tab_and_its_cards_follow() {
    // Review focus 5: open; app.action(None, Action::VerticalTabs); settings.tabs_left();
    // crate::window::view(&app, wid) builds; shortcuts: press(Mode::Welcome, "b", true) is
    // Some(Some(Message::Shortcut(_, Action::VerticalTabs))) and press(Mode::Welcome, "w", false)
    // is Some(Some(Message::Welcome(WelcomeMsg::Close))).
}

#[test]
fn the_welcome_tab_is_not_restored_and_cannot_be_split() {
    // open; save_state(); the saved window's tabs hold no page tab (SavedTab count equals the
    // non-page tabs); split_pane on it leaves one pane (as sessions.rs page_tab_refuses_splits).
}
```

- [ ] **Step 2: Run** `cargo test -p fbterm welcome` (compile errors).

- [ ] **Step 3: Implement**, then look at it: `FBTERM_CONFIG_DIR=<scratch> FBTERM_SECRETS=memory FBTERM_OPEN_WELCOME=1 FBTERM_SCREENSHOT=<scratch>/welcome.png cargo run -p fbterm`; check both cards, the Updates choices, `Done`, and that it scrolls sideways under 520 px.

- [ ] **Step 4: Run** `cargo test -p fbterm` — pass; `cargo clippy --workspace --all-targets -- -D warnings` clean (dead `Dialog` arms removed).

- [ ] **Step 5: Commit**

```bash
git commit -am "feat: Welcome tab asks the first-launch questions in place of the update popup"
```

---

### Task 9: Debug aids, docs and CLAUDE.md

**Files:**
- Modify: `crates/fbterm/src/app.rs` or `debug.rs` (`FBTERM_TAB_BAR=left`: in `build`, after settings load and only through `debug_env`, `settings.tab_bar = Some(TabBar::Left)` without writing; `FBTERM_OPEN_WELCOME=1` from Task 8)
- Create: `website/docs/tabs.md` (sidebar position after `profiles` in `website/sidebars.ts`)
- Modify: `website/docs/keyboard-shortcuts.md` (`Windows and tabs` table: `Vertical tabs` with both chords; `Palette actions` table row), `website/docs/saved-secrets.md` (`## Copying a password`: the tab menu, what it copies, the sixty-second clear and its limits: only while fbterm runs, Wayland needs focus), `website/docs/files.md` (the `check_updates` paragraph: the question is on the Welcome tab), `website/docs/intro.md` (one line pointing to the tabs page), `CLAUDE.md` (debug aids: add `FBTERM_TAB_BAR`, `FBTERM_OPEN_WELCOME`, drop `updates` from `FBTERM_DEMO_PROMPT`; the update-check bullet: the question lives on the Welcome tab (`App::welcome_due`), `Page::Welcome` beside Settings and Profiles in the Runtime bullet and the Status paragraph; the tab bar bullet: tabs on the left are `tab_sidebar.rs`, the drag helper `tab_drag.rs`, the tab menu `tab_menu.rs`)
- Modify: the spec's status line (`Status: implemented on develop, <date>`) once the PR is merged is the owner's; leave `draft` with a note of the PR number.

`tabs.md` sections: `The tab bar` (what a tab shows: stripe or tint, state dot, trouble dot, close, wheel click, dragging to reorder, dropping on a tab's content to merge, the buttons on the right), `Tabs on the left` (the setting, Settings row, the `Vertical tabs` action with its chords written as `Cmd+Shift+B` / `Ctrl+Shift+B` as the other docs pages do, scrolling, narrow windows), `The tab menu` (`Copy password` for password SSH profiles linking to saved-secrets, `Close tab`), `The Welcome tab` (when it opens, what it asks, closing it keeps the defaults, how to change later).

- [ ] **Step 1: Write the docs and the aids.**
- [ ] **Step 2: Build the site**: `cd website && npm ci && npm run build` (fails on broken links). Expected: success.
- [ ] **Step 3: Screenshots** for the docs page if the other pages carry them (`website/static/img/`): `left.png` and `welcome.png` from Tasks 7 and 8, named like the existing ones.
- [ ] **Step 4: Commit**

```bash
git add website CLAUDE.md crates docs
git commit -m "docs: tabs page, Welcome tab, Copy password and the debug aids"
```

---

### Task 10: Gate and PR

- [ ] **Step 1:** `cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo deny --all-features check`. Expected: all clean.
- [ ] **Step 2:** `graphify update .` is for the gateway only; nothing here.
- [ ] **Step 3:** Push `feature/vertical-tabs-tab-menu` and open a PR to `develop` titled `feat: vertical tabs, tab menu with Copy password, Welcome tab`, body listing the three features, the spec and plan paths, the calls made without the owner (spec section 6) and the screenshots. No attribution lines.
