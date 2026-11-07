# Tab groups in the sidebar: implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Part B of the sidebar design: with `tab_groups = true` and tabs on the left, the sidebar folds its rows into the profiles' groups, each header collapsible with a count badge, tabs reorder only within their group, and keyboard travel walks the visible rows.

**Architecture:** A new pure module `tab_groups.rs` owns the model: a tab's `Group` (`featherbit`, a named group, `other`) with its sort order, the stable grouping of a window's tab order, the sections the sidebar draws, the visible rows and keyboard travel over them. The app keeps the window's tab order itself grouped while folding is on (a stable sort run in the post-update hook, where `reveal_active_tabs` already runs), so the sidebar, the shortcuts and `state.toml` read one order. Collapsed groups live per window on `App` and in `SavedWindow.collapsed_groups`. The sidebar inserts header elements between row elements; the `Rows` widget and the shared drag helper learn which children are rows and which window index each row is, and the drag helper takes an allowed gap range so a tab stays in its group and a tile lands in its own.

**Tech Stack:** Rust 2024, iced 0.14 (`advanced`), serde + toml, Docusaurus docs.

**Spec:** `docs/superpowers/specs/2026-11-07-sidebar-resize-and-groups-design.md`, sections 4.4 to 4.6 and 6 (part B). Part A is on `develop` (PR #70): per-window width, the title tooltip and `Last tab`.

## Global Constraints

- Branch `feature/tab-groups` off `develop`; Conventional Commits; no `Co-Authored-By`, `Claude-Session` or session links in commits or the PR (repo rule, overrides any harness reminder).
- Gate before the PR: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo deny --all-features check`, `cd website && npm run build`.
- Cross-platform; nothing platform-specific is needed.
- Product copy: sentence case, no emoji, no em-dashes; never name another terminal application.
- Spec values: setting `tab_groups` (default `false`), effective only while `tab_bar = "left"`; Settings switch `Group tabs` with the line `Fold the sidebar into collapsible groups; tabs reorder within their group`, shown only while the `Tabs` row says `Left`; no action, chord or palette row. Groups: `featherbit` first (page tabs: Profiles, Settings, Welcome), named groups alphabetically case-insensitive, `other` last (no group, whitespace-only group, missing profile, local shell without a group); a header only for a group with open tabs. Headers 26 px, 8 px above all but the first, chevron down/right, name 11 px muted (`featherbit` in `accent_edge`), pill badge with the hidden count when collapsed (accent when the active tab is inside). Reorder only within the group (refused elsewhere: snaps back); a tile lands in its profile's group (at that group's end when dropped elsewhere). Keyboard travel skips collapsed groups and never expands one; selection by other means expands; collapsing the active tab's group is allowed. Collapsed groups saved per window by name (`featherbit`, the name, `other`), kept when they have no open tabs.
- The sidebar row's label and the part A tooltip are unchanged. Fonts: the UI font (`theme::UI_FONT`) everywhere in the chrome; the badge count uses it at 11 px like the bar's tunnel count (there is no separate mono UI font in the app: a plan call).
- `state.toml` from earlier releases loads unchanged; a window without `collapsed_groups` has none.
- `CHANGELOG.md` is written on the release branch, not here.

## Review Focus

1. A profile whose group is only spaces, or whose profile was deleted while its tab is open, must fall under `other` and stay reachable, never vanish or panic (Task 1 test `ungrouped_blank_and_orphaned_tabs_are_other`).
2. Two profiles whose groups differ only by case (`Prod`, `prod`) must share one header, as the Profiles page lists them (Task 1 test `group_names_differing_by_case_are_one_group`).
3. With every group collapsed, next and previous tab must do nothing and the number shortcuts must select nothing, rather than selecting a hidden tab or panicking on an empty list (Task 1 test `travel_over_no_rows_goes_nowhere`).
4. A collapsed name whose group has no open tabs must survive a save and a relaunch, and an old `state.toml` without the field must load (Task 3 tests `collapsed_groups_round_trip_and_default`, `a_collapsed_name_without_tabs_is_kept`).
5. Turning folding on with tabs on top must change nothing visible and must not reorder tabs until the side is switched to left; turning it off must keep the grouped order (Task 4 tests `folding_on_with_tabs_on_top_changes_nothing`, `folding_off_keeps_the_order`).

---

### Task 1: The grouping model

**Files:**
- Create: `crates/fbterm/src/tab_groups.rs` (`mod tab_groups;` in `main.rs` beside `mod tab_sidebar;`)

**Interfaces:**
- Consumes: `fbterm_config::{Profile, ProfileStore}`, `crate::tabs::{Tab, Page}`.
- Produces (all `pub(crate)`):
  - `enum Group { Featherbit, Named(String), Other }` with `PartialEq, Eq, Clone, Debug`, and `Ord`/`PartialOrd` implemented by hand: `Featherbit < Named < Other`; two `Named` compare by `name.to_lowercase()` then by `name`.
  - `impl Group { fn title(&self) -> &str }` (`"featherbit"`, the name as given, `"other"`); `fn key(&self) -> String` (same text; the name saved in state); `fn from_key(key: &str) -> Group` (`"featherbit"` → `Featherbit`, `"other"` → `Other`, else `Named`; a user group literally named `other` or `featherbit` shares the privileged group's key: accepted, documented in the module doc).
  - `fn group_of(tab: &Tab, profiles: &ProfileStore) -> Group`: `Featherbit` when `tab.page.is_some()`; else the focused pane's profile's `group`, trimmed, non-empty → `Named(trimmed.to_owned())`, otherwise `Other` (also when the profile is missing).
  - `fn grouped_order(groups: &[Group]) -> Vec<usize>`: the indices `0..groups.len()` stably sorted by group, so equal groups keep their relative order; two `Named` whose names differ only by case count as equal for this sort (compare lowercase only, then stability).
  - `struct Section { group: Group, rows: Vec<usize>, collapsed: bool }` and `fn sections(groups: &[Group], collapsed: &BTreeSet<String>) -> Vec<Section>`: one section per run of equal (case-insensitive) groups in the given order, titled by the first tab's group, `collapsed` when `collapsed` contains `group.key()`. On an already grouped order this is one section per group.
  - `fn visible_rows(sections: &[Section]) -> Vec<usize>`: the rows of the sections that are not collapsed, in order.
  - `enum Step { Next, Prev, Number(u8) }` and `fn travel(rows: &[usize], active: usize, step: Step) -> Option<usize>`: the window index to select. `Next`: the first row after `active` in window-index order, wrapping to the first row; `Prev`: the last row before `active`, wrapping to the last; `Number(1..=8)`: `rows[n - 1]`; `Number(9)`: the last row; `Number(0)` or no rows: `None`. A result equal to `active` is still `Some` (the caller's `select` returns false then).
  - Consumed by Tasks 3 to 7.

- [ ] **Step 1: Write the failing tests** (in `tab_groups.rs`, `#[cfg(test)]`; a `tab(group: Option<&str>)` helper builds a `Tab::single(Pane::for_profile(&profile))` with a profile in a `ProfileStore`, a `page()` helper builds `Tab::page(Page::Settings)`)

```rust
#[test]
fn groups_sort_featherbit_first_named_by_name_other_last() {
    let mut v = vec![Group::Other, Group::Named("staging".into()), Group::Featherbit, Group::Named("Production".into())];
    v.sort();
    assert_eq!(v, vec![Group::Featherbit, Group::Named("Production".into()), Group::Named("staging".into()), Group::Other]);
}

#[test]
fn keys_round_trip() {
    for g in [Group::Featherbit, Group::Named("prod".into()), Group::Other] {
        assert_eq!(Group::from_key(&g.key()), g);
    }
    assert_eq!(Group::Featherbit.title(), "featherbit");
    assert_eq!(Group::Other.title(), "other");
}

#[test]
fn ungrouped_blank_and_orphaned_tabs_are_other() {
    // tab(None), tab(Some("   ")), a tab whose profile is not in the store, a local shell
    // profile without a group: all Group::Other; tab(Some(" prod ")) is Named("prod");
    // page() is Featherbit.
}

#[test]
fn group_names_differing_by_case_are_one_group() {
    let groups = [Group::Named("Prod".into()), Group::Other, Group::Named("prod".into())];
    assert_eq!(grouped_order(&groups), vec![0, 2, 1]);
    let order: Vec<Group> = grouped_order(&groups).into_iter().map(|i| groups[i].clone()).collect();
    let s = sections(&order, &BTreeSet::new());
    assert_eq!(s.len(), 2);
    assert_eq!(s[0].group, Group::Named("Prod".into()), "titled by the first tab's spelling");
    assert_eq!(s[0].rows, vec![0, 1]);
}

#[test]
fn grouping_is_stable_within_a_group() {
    let groups = [Group::Named("b".into()), Group::Other, Group::Named("a".into()), Group::Named("b".into()), Group::Featherbit];
    assert_eq!(grouped_order(&groups), vec![4, 2, 0, 3, 1]);
}

#[test]
fn sections_mark_collapsed_and_visible_rows_skip_them() {
    let groups = [Group::Featherbit, Group::Named("a".into()), Group::Named("a".into()), Group::Other];
    let collapsed = BTreeSet::from(["a".to_string()]);
    let s = sections(&groups, &collapsed);
    assert_eq!(s.iter().map(|s| (s.rows.clone(), s.collapsed)).collect::<Vec<_>>(),
        vec![(vec![0], false), (vec![1, 2], true), (vec![3], false)]);
    assert_eq!(visible_rows(&s), vec![0, 3]);
}

#[test]
fn travel_walks_visible_rows_and_wraps() {
    let rows = [0, 3, 4];           // 1 and 2 are hidden
    assert_eq!(travel(&rows, 0, Step::Next), Some(3));
    assert_eq!(travel(&rows, 4, Step::Next), Some(0));
    assert_eq!(travel(&rows, 0, Step::Prev), Some(4));
    assert_eq!(travel(&rows, 3, Step::Prev), Some(0));
    assert_eq!(travel(&rows, 1, Step::Next), Some(3), "from a hidden tab: the next visible");
    assert_eq!(travel(&rows, 2, Step::Prev), Some(0));
    assert_eq!(travel(&rows, 0, Step::Number(2)), Some(3));
    assert_eq!(travel(&rows, 0, Step::Number(9)), Some(4));
    assert_eq!(travel(&rows, 0, Step::Number(3)), Some(4));
    assert_eq!(travel(&rows, 0, Step::Number(4)), None);
    assert_eq!(travel(&rows, 0, Step::Number(0)), None);
}

#[test]
fn travel_over_no_rows_goes_nowhere() {
    for step in [Step::Next, Step::Prev, Step::Number(1), Step::Number(9)] {
        assert_eq!(travel(&[], 0, step), None);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p fbterm tab_groups`
Expected: compile error, module missing.

- [ ] **Step 3: Implement the module** as in Interfaces, with a module doc stating the order rule, the key collision for user groups named `other` or `featherbit`, and that `sections` expects a grouped order.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p fbterm tab_groups` then `cargo clippy --workspace --all-targets -- -D warnings` (expect dead-code errors for items not yet used: allow them for this commit with `#[allow(dead_code)]` on the module line in `main.rs`, removed in Task 6).
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/fbterm/src/tab_groups.rs crates/fbterm/src/main.rs
git commit -m "feat(sidebar): tab group model: order, sections, visible rows, travel"
```

---

### Task 2: The `tab_groups` setting

**Files:**
- Modify: `crates/fbterm-config/src/settings.rs:183-186` (field after `tab_bar`), `:213` (default), sample after the `tab_bar` block (`:436-441`), `tabs_left` (`:471-474`, add `tab_groups_on`)
- Modify: `crates/fbterm-config/src/settings_edit.rs:26` (variant), `:51`, `:72`, `:101`, test `every_change` (`:364-380`)
- Modify: `crates/fbterm/src/settings_window/mod.rs:59-61` (`SettingsMsg::TabGroups(bool)`), `:472` (handling); `crates/fbterm/src/settings_window/view.rs:352-375` (the row); `crates/fbterm/src/settings_window/tests.rs:392-400`
- Modify: `website/docs/files.md:69` (the settings table: a `tab_groups` row after `tab_bar`)

**Interfaces:**
- Produces: `Settings.tab_groups: bool` (`#[serde(default)]`, default `false`); `Settings::tab_groups_on(&self) -> bool` = `self.tab_groups && self.tabs_left()`; `SettingChange::TabGroups(bool)` with key `"tab_groups"`; `SettingsMsg::TabGroups(bool)`.
- Consumed by Tasks 3 to 7.

- [ ] **Step 1: Write the failing tests**

In `settings.rs` tests (next to `tab_bar_is_absent_until_answered_and_parses_both_sides` in `settings_edit.rs:704`, or the settings tests module): `tab_groups_defaults_off_and_counts_only_with_tabs_left`: `Settings::default().tab_groups == false`; `toml::from_str("tab_groups = true\n")` gives `tab_groups == true` and `tab_groups_on() == false`; with `tab_bar = "left"` as well, `tab_groups_on() == true`; the sample template parses with the field commented out.

In `settings_edit.rs` `every_change`: add `SettingChange::TabGroups(true)` (the existing round-trip test then covers key, apply and value).

In `settings_window/tests.rs`: `the_group_tabs_switch_changes_the_setting_at_once`: `s.update(SettingsMsg::TabGroups(true))`; `s.values.tab_groups == true`; `s.take_pending() == [SettingChange::TabGroups(true)]`. And in the labels test near `:402` (`labels(&s)`): with `tab_bar = Left` the labels contain `"Group tabs"`, with `Top` they do not.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p fbterm-config tab_groups` and `cargo test -p fbterm settings_window::tests::the_group_tabs`
Expected: compile errors on the field, the variant and the message.

- [ ] **Step 3: Implement**

The field with doc `Fold the sidebar's rows into the profiles' groups (only with tabs on the left).`; the sample block:

```
# Fold the sidebar into the profiles' groups when tabs are on the left: a collapsible header
# per group, tabs reorder within their group. Ignored with tabs on top.
#tab_groups = false
```

`SettingChange::TabGroups(v)` → key `"tab_groups"`, apply `settings.tab_groups = *v`, value `Value::from(*v)`. In `view.rs`, after `rows.push(setting(t, "Tabs", None, false, tabs));`: `if v.tabs_left() { rows.push(setting(t, "Group tabs", Some("Fold the sidebar into collapsible groups; tabs reorder within their group"), false, switch(t, v.tab_groups, editable, false, SettingsMsg::TabGroups))); }`. `files.md`: a row `| \`tab_groups\` | \`true\` or \`false\` | \`false\` | Fold the sidebar into the profiles' groups when tabs are on the left (see [Tab groups](tabs#tab-groups)). In Settings this is **Group tabs**. |` (the anchor exists after Task 8).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p fbterm-config` and `cargo test -p fbterm settings_window`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add crates/fbterm-config/src/settings.rs crates/fbterm-config/src/settings_edit.rs crates/fbterm/src/settings_window website/docs/files.md
git commit -m "feat(settings): tab_groups setting with its Group tabs switch"
```

---

### Task 3: Collapsed groups per window, saved and restored

**Files:**
- Modify: `crates/fbterm-config/src/state.rs:14-26` (`SavedWindow.collapsed_groups`), its tests
- Modify: `crates/fbterm/src/app.rs` (field `collapsed`, init, `WindowClosed` cleanup, `TabBarMsg::ToggleGroup(String)` and its arm)
- Modify: `crates/fbterm/src/restore.rs` (`plan` passes the names through; `snapshot` takes `collapsed: &HashMap<window::Id, BTreeSet<String>>`), its tests and helpers
- Modify: `crates/fbterm/src/windows.rs:84-105` (`restore_windows` inserts), `:321-333` (`save_state` passes the map), `:50-67` (`new_window` inserts nothing)
- Modify: `crates/fbterm/src/restore_tests.rs` (`saved` helper, a relaunch test)
- Create: an `impl App` block in `tab_groups.rs` for the app-side helpers

**Interfaces:**
- Consumes: `Group`, `group_of`, `sections`, `visible_rows` (Task 1); `Settings::tab_groups_on` (Task 2).
- Produces: `SavedWindow.collapsed_groups: Vec<String>` (`#[serde(default, skip_serializing_if = "Vec::is_empty")]`); `App.collapsed: HashMap<window::Id, BTreeSet<String>>`; `TabBarMsg::ToggleGroup(String)` (a header click, by key) handled by `App::toggle_group(&mut self, wid, key: String) -> Task<Message>` (insert or remove, then `save_state`, `Task::none()`); `App::groups_on(&self) -> bool` (= `self.settings.tab_groups_on()`); `App::tab_groups(&self, wid) -> Vec<Group>` (one per tab, window order); `App::sections_of(&self, wid) -> Vec<Section>` (from `tab_groups` and the window's collapsed set; with groups off: one section `Other` holding every row, never collapsed, so callers need no special case); `App::visible_rows(&self, wid) -> Vec<usize>`; `restore::snapshot(windows, sizes, sidebar_widths, collapsed, recency)`.
- Consumed by Tasks 4 to 7.

- [ ] **Step 1: Write the failing tests**

`state.rs` tests: `collapsed_groups_round_trip_and_default`: a window with `collapsed_groups: vec!["staging".into(), "other".into()]` serializes with the key once and reads back equal; a window with an empty vec serializes without the key; the pre-existing TOML text (no key) loads with an empty vec.

`restore.rs` tests: `snapshot_writes_collapsed_groups_per_window` (w1 with `{"staging"}` → `["staging"]`, w2 with nothing → empty) and `plan_keeps_collapsed_names_as_they_are` (`["a", "b"]` in → `["a", "b"]` out).

`restore_tests.rs`: `a_collapsed_name_without_tabs_is_kept`: save a state whose one window has `collapsed_groups: ["staging"]` while its only tab's profile has no group; build the app; `app.collapsed[&wid] == {"staging"}`; call `app.save_state()` with saving enabled and read the file back: `collapsed_groups == ["staging"]`.

`tab_groups.rs` app tests (using `app_with_tabs` from `tab_sidebar.rs` tests, made `pub(crate)`, plus a variant that sets profile groups: `app_with_groups(&[("a", Some("prod")), ("b", None)])`): `toggling_a_group_collapses_it_and_saves`: with `tab_groups = true` and `tab_bar = Left` set on `app.settings`, `app.update(Message::TabBar(wid, TabBarMsg::ToggleGroup("prod".into())))` → `app.collapsed[&wid]` contains `"prod"`, `app.visible_rows(wid)` omits the `prod` tab's index; toggling again removes it. `sections_without_folding_are_one_open_section`: with `tab_groups = false`, `app.sections_of(wid).len() == 1` and `visible_rows == 0..n`.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p fbterm-config state::tests` and `cargo test -p fbterm -- tab_groups restore collapsed`
Expected: compile errors on the field, the message and the helpers.

- [ ] **Step 3: Implement** as in Interfaces; `WindowClosed` removes the window's entry; `save_state` passes `&self.collapsed`; `restore_windows` inserts `saved.collapsed_groups` as a `BTreeSet` when non-empty.

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --workspace`
Expected: PASS (every existing `snapshot` caller updated).

- [ ] **Step 5: Commit**

```bash
git add crates/fbterm-config/src/state.rs crates/fbterm/src
git commit -m "feat(sidebar): collapsed groups per window, saved in state"
```

---

### Task 4: Keeping the tab order grouped

**Files:**
- Modify: `crates/fbterm/src/tab_groups.rs` (`impl App`: `regroup`, `regroup_all`, `expand_active`)
- Modify: `crates/fbterm/src/app.rs:1049-1051` (the post-update hook: `self.regroup_all()` before `reveal_active_tabs`)
- Modify: `crates/fbterm/src/tab_sidebar.rs` `reveal_active_tabs` (`:570-600` area): when a window's active tab changed, call `expand_active` first

**Interfaces:**
- Consumes: Task 1 and Task 3.
- Produces: `App::regroup(&mut self, wid) -> bool` (while `groups_on()`: reorder `window.tabs` by `grouped_order`, keeping the active tab by id; returns whether anything moved; nothing while off); `App::regroup_all(&mut self)` (every window; saves state once when something moved); `App::expand_active(&mut self, wid) -> bool` (removes the active tab's group key from the window's collapsed set when present, saving state; returns whether it did).
- Consumed by Tasks 5 to 7.

- [ ] **Step 1: Write the failing tests** (app tests in `tab_groups.rs`; `groups_on` set by writing `app.settings.tab_groups = true; app.settings.tab_bar = Some(TabBar::Left)`; names of a window: `app.windows[&wid].tabs.iter().map(Tab::name)`)

```rust
#[test]
fn switching_folding_on_sorts_tabs_stably_and_keeps_the_active_tab() {
    // tabs: b(staging) a(prod) p(page Settings) c(none) d(prod), active = d
    // after app.regroup_all(): names == [p, a, d, b, c], active index == 2 (d)
}

#[test]
fn folding_off_keeps_the_order() {
    // same window, groups on, regroup_all; then tab_groups = false; push tab e(prod) at the end;
    // regroup_all again: order unchanged (e stays last, not moved into prod)
}

#[test]
fn folding_on_with_tabs_on_top_changes_nothing() {
    // tab_groups = true, tab_bar = Top: regroup_all leaves [b, a, p, c, d] as is
}

#[test]
fn a_new_tab_lands_at_the_end_of_its_group() {
    // groups on, grouped order [p, a, d, b, c]; app.open_tab(wid, profile of group prod)
    // then regroup_all (as the post-update hook would): the new tab sits right after d
}

#[test]
fn the_post_update_hook_regroups() {
    // groups on, ungrouped order; send any message through app.update (TabBarMsg::Select(0));
    // the order is grouped afterwards
}

#[test]
fn selecting_a_hidden_tab_expands_its_group_but_collapsing_the_active_one_keeps_it() {
    // groups on, collapse "prod" (ToggleGroup) while active is in "staging";
    // app.update(TabBarMsg::Select(index of a)): collapsed no longer has "prod", active == a;
    // then ToggleGroup("prod") again: collapsed has "prod" and active is still a
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p fbterm tab_groups`
Expected: compile errors on the new methods, then assertion failures.

- [ ] **Step 3: Implement.** `regroup`: compute `grouped_order`; if it is `0..n`, return false; else permute `tabs` (remember the active tab's id first, then `active = find(id)`). `regroup_all` loops the window ids; `expand_active` reads `group_of(active_tab)` and removes its key. In `reveal_active_tabs`, inside the branch that detects a changed active tab (`sidebar_shown.get(wid) != Some(&tab.id)`), call `self.expand_active(*wid)` before computing the reveal offset (the borrow of `self.windows` must end first: collect the ids and active tab ids into a `Vec` up front).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --workspace`
Expected: PASS, including the existing drag, restore and sidebar tests (they run with folding off).

- [ ] **Step 5: Commit**

```bash
git add crates/fbterm/src/tab_groups.rs crates/fbterm/src/app.rs crates/fbterm/src/tab_sidebar.rs
git commit -m "feat(sidebar): keep the tab order grouped while folding is on"
```

---

### Task 5: Keyboard travel over the visible rows

**Files:**
- Modify: `crates/fbterm/src/actions.rs:61-63` (`NextTab`, `PrevTab`, `SelectTab`)
- Modify: `crates/fbterm/src/tab_groups.rs` (`impl App::travel_tabs`), tests
- Modify: `crates/fbterm/src/windows.rs` (closing the active tab: no change needed, `expand_active` covers it; add the test)

**Interfaces:**
- Consumes: `travel`, `Step`, `App::visible_rows` (Tasks 1, 3), `App::expand_active` (Task 4).
- Produces: `App::travel_tabs(&mut self, wid: Option<window::Id>, step: Step) -> Task<Message>`: for the focused window (as `with_focused` resolves it), `rows = self.visible_rows(wid)`, `travel(&rows, w.active, step)` then `w.select(i)`; with folding off the rows are every tab, so the behaviour equals `next`/`prev`/`select_number` today. `WindowState::next`, `prev`, `select_number` stay (used by tests and the flat path).

- [ ] **Step 1: Write the failing tests** (app tests in `tab_groups.rs`, groups on, order `[p(featherbit), a(prod), d(prod), b(staging), c(other)]`, `staging` collapsed)

```rust
#[test]
fn next_and_prev_skip_collapsed_groups_and_never_expand_them() {
    // active a: NextTab → d; NextTab → c (b skipped); NextTab → p (wraps); PrevTab → c;
    // collapsed still has "staging"
}

#[test]
fn number_shortcuts_count_visible_rows() {
    // SelectTab(3) → c (p, a, d, c visible); SelectTab(9) → c; SelectTab(5) → nothing
}

#[test]
fn with_every_group_collapsed_travel_does_nothing() {
    // collapse featherbit, prod, staging, other; active a; NextTab, PrevTab, SelectTab(1): active stays a
}

#[test]
fn closing_the_active_tab_into_a_collapsed_group_expands_it() {
    // order [p, a, d, b, c], staging collapsed, active d; close d (TabBarMsg::Close(2)):
    // the flat rule selects b (the tab that took its place); after the update "staging" is expanded
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p fbterm tab_groups`
Expected: assertion failures (travel still flat).

- [ ] **Step 3: Implement** `travel_tabs` and route the three actions through it (`Action::SelectTab(n) => self.travel_tabs(wid, Step::Number(n))`, same for `Next`/`Prev`).

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --workspace`
Expected: PASS, including `tabs::tests::next_prev_wrap_and_numbers_select` and `nine_selects_the_last_tab_past_the_ninth` (unchanged).

- [ ] **Step 5: Commit**

```bash
git add crates/fbterm/src/actions.rs crates/fbterm/src/tab_groups.rs
git commit -m "feat(sidebar): keyboard travel walks the visible rows"
```

---

### Task 6: Headers in the sidebar

**Files:**
- Modify: `crates/fbterm/src/tab_sidebar.rs`: `view` (`:168-230`), `Rows` (`:330-345`, `row_rects`, `draw`), `row_top`/`reveal` (`:55-72`), `reveal_active_tabs`, tests
- Modify: `crates/fbterm/src/ui.rs:30-31` (add `ICON_RIGHT`)
- Modify: `crates/fbterm/src/main.rs` (drop the `#[allow(dead_code)]` from Task 1)

**Interfaces:**
- Consumes: `App::sections_of`, `TabBarMsg::ToggleGroup` (Task 3), `Group` (Task 1).
- Produces, in `tab_sidebar.rs`:
  - `pub(crate) const HEADER_HEIGHT: f32 = 26.0;`, `pub(crate) const HEADER_GAP: f32 = 8.0;`
  - `pub(crate) enum Item { Header { group: Group, hidden: usize, holds_active: bool }, Row(usize) }` (`Row` carries the window index) and `pub(crate) fn items(sections: &[Section], active: usize, folding: bool) -> Vec<Item>`: with `folding` false, one `Row` per tab and no headers; with it true, per section a `Header` (`hidden` = `rows.len()` when collapsed else 0; `holds_active` when collapsed and `rows` contains `active`) then its rows unless collapsed.
  - `pub(crate) fn item_tops(items: &[Item]) -> Vec<f32>`: each item's top from the list's content top: start at `LIST_PADDING`; a header adds `HEADER_GAP` before it unless it is the first item; items are `HEADER_HEIGHT` or `ROW_HEIGHT` tall and `ROW_SPACING` apart. With no headers this equals the old `row_top(i)` for every `i`.
  - `pub(crate) fn reveal(top: f32, height: f32, offset: f32, viewport: f32) -> Option<f32>` (the old `reveal` with the row's `top` and `height` passed in; `row_top` is removed).
  - `Sidebar` gains `pub items: Vec<Item>` built by `window.rs` from `app.sections_of(id)`, `win.active` and `app.groups_on()`.
  - `Rows` gains `is_row: Vec<bool>` (one per column child) so `row_rects` returns the rectangles of rows only, in order; `ids` lists the visible rows' tab ids and `hole` is the active tab's position among them (`None` when hidden).
  - `ICON_RIGHT: &str = "\u{e06f}"` (chevron-right; the font's chevrons sit at consecutive code points: down `e06d`, left `e06e`, right `e06f`, up `e070`; `bundled_icon_font_has_every_icon_codepoint` checks it exists).
  - `fn header_view(s: &Sidebar<'_>, group: &Group, hidden: usize, holds_active: bool) -> Element<'a, Message>`: a `button` `HEADER_HEIGHT` high, full width, `on_press(TabBarMsg::ToggleGroup(group.key()))`, transparent idle and `t.hover` on hover, 5 px corners, padding `[0, 6]`; content: `icon(ICON_DOWN or ICON_RIGHT, 12.0, t.text_muted)`, `text(group.title()).font(UI_FONT).size(11).color(if Featherbit { t.accent_edge } else { t.text_muted })` clipped on one line, `Space` filling, and when `hidden > 0` the badge: `container(text(hidden).font(UI_FONT).size(11).line_height(Absolute(16)))` with padding `[0, 6]`, background `t.raised`, border `t.border` 1 px radius 8; when `holds_active` the border and text take `t.accent_edge`. The header carries no tooltip and no menu (a right-click on it does nothing: it is not a row, so `drag_event` finds no tab under it).
  - The list: `column` of `items` mapped to `header_view` or `row_view`; the `HEADER_GAP` is a `Space` of that height before each header except the first (a separate child, with `is_row` false).

- [ ] **Step 1: Write the failing tests** (in `tab_sidebar.rs` tests)

```rust
#[test]
fn items_without_folding_are_rows_only_and_tops_match_the_flat_list() {
    let sections = vec![Section { group: Group::Other, rows: vec![0, 1, 2], collapsed: false }];
    let items = items(&sections, 1, false);
    assert!(items.iter().all(|i| matches!(i, Item::Row(_))));
    let tops = item_tops(&items);
    assert_eq!(tops, vec![6.0, 6.0 + 34.0, 6.0 + 68.0]);
}

#[test]
fn items_with_folding_carry_headers_counts_and_the_active_marker() {
    let sections = vec![
        Section { group: Group::Featherbit, rows: vec![0], collapsed: false },
        Section { group: Group::Named("prod".into()), rows: vec![1, 2], collapsed: true },
        Section { group: Group::Other, rows: vec![3], collapsed: false },
    ];
    let items = items(&sections, 2, true);
    // Header(featherbit, 0, false), Row(0), Header(prod, 2, true), Header(other, 0, false), Row(3)
    assert_eq!(items.len(), 5);
    assert!(matches!(&items[2], Item::Header { hidden: 2, holds_active: true, .. }));
    let tops = item_tops(&items);
    assert_eq!(tops[0], 6.0);
    assert_eq!(tops[1], 6.0 + 26.0 + 2.0);
    assert_eq!(tops[2], tops[1] + 32.0 + 2.0 + 8.0, "a gap above a later header");
}

#[test]
fn reveal_uses_the_items_top() {
    assert_eq!(reveal(100.0, 32.0, 0.0, 50.0), Some(100.0 + 32.0 + 6.0 - 50.0));
    assert_eq!(reveal(100.0, 32.0, 90.0, 300.0), None);
}
```

And a layout test `a_folded_sidebar_lays_out`: `app_with_groups`, groups on, one group collapsed, `lay_out(&app, wid)` does not panic; and `the_active_row_is_revealed_under_headers`: with three groups and a tall list, selecting the last tab yields a `scroll_to` task (reuse the pattern of `selecting_a_hidden_row_scrolls_the_sidebar_and_a_visible_one_does_not`, with the expected offset computed from `item_tops`).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p fbterm tab_sidebar`
Expected: compile errors on `Item`, `items`, `item_tops`, `reveal`'s arity.

- [ ] **Step 3: Implement** as in Interfaces. `reveal_active_tabs` finds the active tab's item index (`Item::Row(active)`), takes its top from `item_tops`, and calls `reveal(top, ROW_HEIGHT, offset, height)`; when the active row is hidden (no item), nothing scrolls. `window.rs` builds `items` and passes them in `Sidebar`. In `Rows::update`, the strip's `ids` and `hole` come from the items (visible rows); the per-row `TabBarMsg::Select`/`Close`/`Menu` built in `row_view` already carry the window index.

- [ ] **Step 4: Run the tests and the app**

Run: `cargo test --workspace` and `cargo clippy --workspace --all-targets -- -D warnings`
Expected: PASS, no warnings.

Run the app with groups on, with a throwaway config holding profiles in two groups (write a `profiles.toml` with `group = "prod"` on two local profiles and none on a third into `$FBTERM_CONFIG_DIR`), `FBTERM_TAB_BAR=left`, and `tab_groups = true` in its `settings.toml`; open the three profiles. Check by hand: headers `prod` and `other` (and `featherbit` once Settings is open, first), click a header to collapse it and see the badge with the count, collapse the active tab's group and see the badge in the accent, select a hidden tab from the palette and see its group expand, reorder within `prod` by dragging.

- [ ] **Step 5: Commit**

```bash
git add crates/fbterm/src/tab_sidebar.rs crates/fbterm/src/ui.rs crates/fbterm/src/window.rs crates/fbterm/src/main.rs
git commit -m "feat(sidebar): group headers with collapse and a hidden count"
```

---

### Task 7: Dragging within a group

**Files:**
- Modify: `crates/fbterm/src/tab_drag.rs` (`Strip`, `drag_event`, `gap_at` callers), tests
- Modify: `crates/fbterm/src/tab_sidebar.rs` (`Rows::strip`: `indices`, `range`, `clamp`)
- Modify: `crates/fbterm/src/window.rs` (the bar's `Strip`: `indices` = `0..n`, `range: None`, `clamp: false`)

**Interfaces:**
- Consumes: `items`, `Item`, `Rows` (Task 6), `App::sections_of` and `group_of` (Tasks 1, 3).
- Produces, in `tab_drag.rs`: `Strip` gains `pub indices: &'a [usize]` (the window index of each row, parallel to `ids`), `pub range: Option<(usize, usize)>` (the gap indices, in strip space and inclusive, the current carry may land in) and `pub clamp: bool` (a gap outside `range` is moved to its nearest end instead of refused). `drag_event`: the index in `TabBarMsg::Close`, `TabBarMsg::Menu`, `DragMsg::OverTab` is `indices[row]`; a gap `g` is published as `indices[g]` when `g < indices.len()`, else `indices.last() + 1` (0 for an empty strip); a gap outside `range` is `None` (no marker, and the release publishes only `DragMsg::End`, so the tab snaps back) unless `clamp`, when it becomes the nearer bound.
- In `tab_sidebar.rs`, `Rows` carries `indices: Vec<usize>` and computes `range`/`clamp` from the sidebar's sections: for `Carry::Tab(id)` the span of the section holding `id` (its first row's strip index to one past its last), `clamp: false`; for `Carry::Tile(pane)` the span of the section of that pane's profile group (looked up in `window.rs` and passed as `Sidebar.tile_group: Option<Group>`), or, when no section has that group, the single gap where it would sort (one past the last row of the last section whose group sorts before it, else 0), `clamp: true`; `None` with folding off.

- [ ] **Step 1: Write the failing tests** (in `tab_drag.rs` tests; build a `Strip` with `ids` of four tabs, `indices = [0, 1, 3, 4]` (window index 2 hidden), rows at y = 0, 34, 68, 102, 30 high)

```rust
#[test]
fn published_indices_are_window_indices() {
    // a right-click on row 2 publishes TabBarMsg::Menu { index: 3, .. };
    // a middle click on row 3 publishes Close(4)
}

#[test]
fn a_tab_dropped_outside_its_range_snaps_back() {
    // carry Tab(ids[1]), range Some((0, 2)), clamp false: a release at row 3's lower half
    // (gap 4) publishes only DragMsg::End; a release at gap 0 publishes ReorderTab { index: 0 } then End
}

#[test]
fn a_tile_dropped_outside_its_range_lands_at_its_groups_end() {
    // carry Tile(pane), range Some((2, 4)), clamp true: a release at gap 0 publishes
    // TileToTab { index: 3 } (strip gap 2 → window index indices[2] = 3)
}

#[test]
fn a_gap_past_the_last_row_maps_to_one_past_the_last_window_index() {
    // carry Tab(ids[0]), range None: a release below row 3 publishes ReorderTab { index: 5 }
}
```

Use the existing test helpers in `tab_drag.rs` tests (`a_tab_carried_down_the_sidebar_and_dropped_in_a_gap_asks_to_move_there` shows how events are fed and messages collected).

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p fbterm tab_drag`
Expected: compile errors on the new fields.

- [ ] **Step 3: Implement** in `drag_event` (the mapping in one helper `fn window_gap(indices: &[usize], gap: usize) -> usize` and the range rule in the `gap` closure), then the two `Strip` construction sites.

- [ ] **Step 4: Run the tests and the app**

Run: `cargo test --workspace`
Expected: PASS, including every existing drag test (bar and sidebar, folding off).

In the app (Task 6's setup): drag a `prod` tab above the `other` header and release: it snaps back; drag it within `prod`: it moves; split a pane and drag the tile onto the sidebar over another group: the gap shows at `prod`'s end and the drop lands there.

- [ ] **Step 5: Commit**

```bash
git add crates/fbterm/src/tab_drag.rs crates/fbterm/src/tab_sidebar.rs crates/fbterm/src/window.rs
git commit -m "feat(sidebar): tabs reorder within their group, tiles land in theirs"
```

---

### Task 8: Docs, CLAUDE.md and the gate

**Files:**
- Modify: `website/docs/tabs.md` (a `## Tab groups` section after the sidebar paragraphs, before `## The tab menu`)
- Modify: `CLAUDE.md` (the sidebar gotcha gains the groups sentence; the `## Status` sentence from part A now says part B is on `develop`)
- Modify: `crates/fbterm/src/tab_sidebar.rs:1-3` (module doc mentions groups)

- [ ] **Step 1: Write the docs.** `tabs.md`, section `## Tab groups`: with tabs on the left, turn on **Group tabs** in Settings (Appearance), or `tab_groups = true` in `settings.toml`; the column folds into the groups profiles carry (the `group` field in `profiles.toml`, edited in the Profiles page): `featherbit` first for the Profiles, Settings and Welcome tabs, then the groups by name, then `other` for tabs without one; a header shows only while one of its tabs is open; click a header to fold or unfold its group, a folded header shows how many tabs it hides, in violet when the selected tab is among them; tabs can be dragged only within their group; next and previous tab and the number shortcuts walk the tabs you can see, skipping folded groups, while choosing a tab any other way (the palette, a click in the Profiles page) unfolds its group; which groups are folded is remembered per window. With tabs on top the setting does nothing.

`CLAUDE.md` gotcha: one sentence after the tooltip clause: with `tab_groups` on (`Settings::tab_groups_on`), `tab_groups.rs` keeps each window's tab order grouped (`App::regroup_all` in the post-update hook, stable by `Group` order: `Featherbit`, named case-insensitive, `Other`), draws headers between rows (`Item`, `items`, `item_tops`), keeps collapsed group keys per window (`App::collapsed`, `SavedWindow.collapsed_groups`, `TabBarMsg::ToggleGroup`), expands the active tab's group when the active tab changes (`expand_active`, so collapsing the active group by hand sticks), routes `NextTab`/`PrevTab`/`SelectTab` through `App::travel_tabs` over `visible_rows`, and restricts drags through `Strip::range`/`clamp` with `Strip::indices` mapping rows to window indices. Status: part B on `develop`.

- [ ] **Step 2: Run the full gate**

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo deny --all-features check
(cd website && npm run build)
```
Expected: all clean (the `files.md` link to `tabs#tab-groups` resolves now).

- [ ] **Step 3: Commit**

```bash
git add website/docs/tabs.md CLAUDE.md crates/fbterm/src/tab_sidebar.rs
git commit -m "docs: tab groups in the sidebar"
```

- [ ] **Step 4: Open the PR** to `develop` titled `feat(sidebar): fold tabs into profile groups`, body listing the behaviours and the spec path, no attribution footer.
