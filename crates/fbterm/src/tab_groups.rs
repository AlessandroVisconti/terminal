//! Tabs folded into their profiles' groups (`tab_groups = true`, with tabs on the left): which
//! group a tab belongs to, the order groups come in, the sections the sidebar draws, the rows
//! that can be seen and keyboard travel over them.
//!
//! Order: `featherbit` (the page tabs) first, then the named groups alphabetically, case
//! insensitive, then `other` (tabs without a group). Two names that differ only by case are
//! one group, titled by the first tab's spelling, as the Profiles page lists them. A group is
//! remembered as collapsed by its key, which is the name as given, `featherbit` or `other`:
//! a user group literally named `other` or `featherbit` therefore shares the privileged
//! group's collapsed state (accepted). [`sections`] expects an order that is already grouped
//! (the app keeps each window's tab order so while folding is on).

use std::cmp::Ordering;
use std::collections::BTreeSet;

use fbterm_config::ProfileStore;

use crate::tabs::Tab;

/// Which group a tab is listed under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Group {
    /// The page tabs (Profiles, Settings, Welcome): always first.
    Featherbit,
    /// A profile group, as the user named it.
    Named(String),
    /// Tabs without a group: always last.
    Other,
}

impl Group {
    /// The header's text.
    pub(crate) fn title(&self) -> &str {
        match self {
            Self::Featherbit => "featherbit",
            Self::Named(name) => name,
            Self::Other => "other",
        }
    }

    /// The name the group is remembered by (in `state.toml`).
    pub(crate) fn key(&self) -> String {
        self.title().to_owned()
    }

    /// How two groups compare for sorting and for being one group: the privileged ones by
    /// kind, named ones by name, case-insensitive.
    fn rank(&self) -> (u8, String) {
        match self {
            Self::Featherbit => (0, String::new()),
            Self::Named(name) => (1, name.to_lowercase()),
            Self::Other => (2, String::new()),
        }
    }

    /// One group with `other`, as the sidebar sections them.
    pub(crate) fn same(&self, other: &Self) -> bool {
        self.rank() == other.rank()
    }
}

/// The group of profile `id`: its `group`, trimmed; `other` without one or when the profile
/// is gone.
pub(crate) fn group_of_profile(id: fbterm_config::ProfileId, profiles: &ProfileStore) -> Group {
    profiles
        .get(id)
        .and_then(|p| p.group.as_deref())
        .map(str::trim)
        .filter(|g| !g.is_empty())
        .map_or(Group::Other, |g| Group::Named(g.to_owned()))
}

impl Ord for Group {
    fn cmp(&self, other: &Self) -> Ordering {
        self.rank()
            .cmp(&other.rank())
            .then_with(|| match (self, other) {
                (Self::Named(a), Self::Named(b)) => a.cmp(b),
                _ => Ordering::Equal,
            })
    }
}

impl PartialOrd for Group {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// The group of `tab`: `featherbit` for a page tab, else its focused pane's profile's group
/// (trimmed), `other` without one or when the profile is gone.
pub(crate) fn group_of(tab: &Tab, profiles: &ProfileStore) -> Group {
    if tab.page.is_some() {
        return Group::Featherbit;
    }
    group_of_profile(tab.focused_pane().profile, profiles)
}

/// The indices of `groups` in grouped order: stable, so tabs of one group keep their order,
/// and names differing only by case count as one group.
#[cfg(test)]
pub(crate) fn grouped_order(groups: &[Group]) -> Vec<usize> {
    grouped_order_moved(groups, &vec![false; groups.len()])
}

/// [`grouped_order`], with the tabs flagged in `moved` (their group just changed) after the
/// others of their group, so they land at its end.
pub(crate) fn grouped_order_moved(groups: &[Group], moved: &[bool]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..groups.len()).collect();
    order.sort_by_cached_key(|&i| (groups[i].rank(), moved.get(i).copied().unwrap_or(false)));
    order
}

/// One run of tabs of the same group, as the sidebar draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Section {
    pub group: Group,
    /// The tabs' window indices, in order.
    pub rows: Vec<usize>,
    pub collapsed: bool,
}

/// The sections of a (grouped) order: one per run of equal groups, titled by the first tab's
/// group, collapsed when `collapsed` holds its key.
pub(crate) fn sections(groups: &[Group], collapsed: &BTreeSet<String>) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    for (i, g) in groups.iter().enumerate() {
        match out.last_mut() {
            Some(last) if last.group.same(g) => last.rows.push(i),
            _ => out.push(Section {
                collapsed: is_folded(collapsed, g),
                group: g.clone(),
                rows: vec![i],
            }),
        }
    }
    out
}

/// Whether `group` is folded in `collapsed`: keys compare case-insensitively, as names that
/// differ only by case are one group.
pub(crate) fn is_folded(collapsed: &BTreeSet<String>, group: &Group) -> bool {
    let key = group.key().to_lowercase();
    collapsed.iter().any(|k| k.to_lowercase() == key)
}

/// Unfold `group` in `collapsed`, whatever case its key was saved in. Returns whether it was
/// folded.
pub(crate) fn unfold(collapsed: &mut BTreeSet<String>, group: &Group) -> bool {
    let key = group.key().to_lowercase();
    let before = collapsed.len();
    collapsed.retain(|k| k.to_lowercase() != key);
    collapsed.len() != before
}

/// The window indices of the rows that can be seen, in sidebar order.
pub(crate) fn visible_rows(sections: &[Section]) -> Vec<usize> {
    sections
        .iter()
        .filter(|s| !s.collapsed)
        .flat_map(|s| s.rows.iter().copied())
        .collect()
}

/// One keyboard move over the rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Step {
    Next,
    Prev,
    /// The number shortcuts: 1 to 8 count the rows, 9 is the last row.
    Number(u8),
}

/// The window index `step` selects from `active` over the visible `rows` (window indices in
/// sidebar order): next and previous walk the rows and wrap, from a hidden active tab they go
/// to the nearest visible one in that direction; none when there is no row to go to.
pub(crate) fn travel(rows: &[usize], active: usize, step: Step) -> Option<usize> {
    let (first, last) = (*rows.first()?, *rows.last()?);
    match step {
        Step::Next => Some(rows.iter().copied().find(|&r| r > active).unwrap_or(first)),
        Step::Prev => Some(
            rows.iter()
                .rev()
                .copied()
                .find(|&r| r < active)
                .unwrap_or(last),
        ),
        Step::Number(0) => None,
        Step::Number(9) => Some(last),
        Step::Number(n) => rows.get(usize::from(n) - 1).copied(),
    }
}

impl crate::app::App {
    /// Folding is on: `tab_groups` with tabs on the left.
    pub(crate) fn groups_on(&self) -> bool {
        self.settings.tab_groups_on()
    }

    /// Each tab's group in window `wid`, in window order.
    pub(crate) fn tab_groups(&self, wid: iced::window::Id) -> Vec<Group> {
        self.windows.get(&wid).map_or_else(Vec::new, |w| {
            w.tabs.iter().map(|t| group_of(t, &self.profiles)).collect()
        })
    }

    /// The sidebar's sections in window `wid`. With folding off: one open section holding
    /// every row, so callers need no special case.
    pub(crate) fn sections_of(&self, wid: iced::window::Id) -> Vec<Section> {
        let Some(w) = self.windows.get(&wid) else {
            return Vec::new();
        };
        if !self.groups_on() {
            return vec![Section {
                group: Group::Other,
                rows: (0..w.tabs.len()).collect(),
                collapsed: false,
            }];
        }
        let empty = BTreeSet::new();
        let collapsed = self.collapsed.get(&wid).unwrap_or(&empty);
        sections(&self.tab_groups(wid), collapsed)
    }

    /// The window indices of the rows that can be seen in window `wid`, in sidebar order.
    pub(crate) fn visible_rows(&self, wid: iced::window::Id) -> Vec<usize> {
        visible_rows(&self.sections_of(wid))
    }

    /// While folding is on, put window `wid`'s tabs in grouped order (stable, the active tab
    /// followed by id). Returns whether anything moved.
    pub(crate) fn regroup(&mut self, wid: iced::window::Id) -> bool {
        if !self.groups_on() {
            return false;
        }
        let groups = self.tab_groups(wid);
        let Some(w) = self.windows.get_mut(&wid) else {
            return false;
        };
        // A tab whose group changed since the last regroup goes to the end of its new group.
        let moved: Vec<bool> = w
            .tabs
            .iter()
            .zip(&groups)
            .map(|(t, g)| {
                self.tab_group_seen
                    .get(&t.id)
                    .is_some_and(|seen| !seen.same(g))
            })
            .collect();
        for (t, g) in w.tabs.iter().zip(&groups) {
            // Only a new or changed group is stored: no clone per tab per message.
            if self.tab_group_seen.get(&t.id) != Some(g) {
                self.tab_group_seen.insert(t.id, g.clone());
            }
        }
        let order = grouped_order_moved(&groups, &moved);
        if order.iter().enumerate().all(|(i, &o)| i == o) {
            return false;
        }
        let active = w.active_tab().map(|t| t.id);
        let mut tabs: Vec<Option<Tab>> = w.tabs.drain(..).map(Some).collect();
        w.tabs = order.into_iter().filter_map(|i| tabs[i].take()).collect();
        if let Some(i) = active.and_then(|id| w.find(id)) {
            w.active = i;
        }
        true
    }

    /// After every update: every window's tabs in grouped order while folding is on, saved
    /// once when anything moved.
    pub(crate) fn regroup_all(&mut self) {
        let ids: Vec<_> = self.windows.keys().copied().collect();
        let moved = ids.into_iter().fold(false, |m, wid| self.regroup(wid) | m);
        // Closed tabs need no remembered group.
        let live: std::collections::HashSet<_> = self
            .windows
            .values()
            .flat_map(|w| w.tabs.iter().map(|t| t.id))
            .collect();
        self.tab_group_seen.retain(|id, _| live.contains(id));
        if moved {
            self.save_state();
        }
    }

    /// The active tab of window `wid` became active by other means than keyboard travel: its
    /// group unfolds if it was folded. Returns whether it did.
    pub(crate) fn expand_active(&mut self, wid: iced::window::Id) -> bool {
        let Some(group) = self
            .windows
            .get(&wid)
            .and_then(|w| w.active_tab())
            .map(|t| group_of(t, &self.profiles))
        else {
            return false;
        };
        let removed = self
            .collapsed
            .get_mut(&wid)
            .is_some_and(|set| unfold(set, &group));
        if removed {
            self.save_state();
        }
        removed
    }

    /// `NextTab`, `PrevTab` and the number shortcuts in the focused window (or `wid`): one
    /// [`Step`] over the rows that can be seen, so a folded group is skipped and never
    /// unfolded. With folding off every tab is a row, as before.
    pub(crate) fn travel_tabs(
        &mut self,
        wid: Option<iced::window::Id>,
        step: Step,
    ) -> iced::Task<crate::app::Message> {
        let Some(wid) = wid.or_else(|| self.focused_window()) else {
            return iced::Task::none();
        };
        let rows = self.visible_rows(wid);
        self.with_window(wid, |w| {
            travel(&rows, w.active, step).is_some_and(|i| w.select(i))
        })
    }

    /// A header click: fold or unfold the sidebar's section at `index`, and remember it.
    pub(crate) fn toggle_group(
        &mut self,
        wid: iced::window::Id,
        index: usize,
    ) -> iced::Task<crate::app::Message> {
        let Some(section) = self.sections_of(wid).into_iter().nth(index) else {
            return iced::Task::none();
        };
        let set = self.collapsed.entry(wid).or_default();
        if !unfold(set, &section.group) {
            set.insert(section.group.key());
        }
        self.save_state();
        iced::Task::none()
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::collections::BTreeSet;

    use fbterm_config::{ProfileId, ProfileStore};

    use super::*;
    use crate::restore_tests::profile;
    use crate::tabs::{Page, Pane, Tab};

    /// A tab of a profile in `store`, with `group`.
    fn tab(store: &mut ProfileStore, group: Option<&str>) -> Tab {
        let mut p = profile("p");
        p.group = group.map(str::to_owned);
        let tab = Tab::single(Pane::for_profile(&p));
        store.profiles.push(p);
        tab
    }

    fn page() -> Tab {
        Tab::page(Page::Settings)
    }

    fn named(s: &str) -> Group {
        Group::Named(s.into())
    }

    #[test]
    fn groups_sort_featherbit_first_named_by_name_other_last() {
        let mut v = vec![
            Group::Other,
            named("staging"),
            Group::Featherbit,
            named("Production"),
        ];
        v.sort();
        assert_eq!(
            v,
            vec![
                Group::Featherbit,
                named("Production"),
                named("staging"),
                Group::Other
            ]
        );
    }

    #[test]
    fn keys_are_the_titles() {
        assert_eq!(Group::Featherbit.key(), "featherbit");
        assert_eq!(named("prod").key(), "prod");
        assert_eq!(Group::Other.key(), "other");
        assert_eq!(Group::Featherbit.title(), "featherbit");
        assert_eq!(Group::Other.title(), "other");
        assert_eq!(named("prod").title(), "prod");
    }

    #[test]
    fn ungrouped_blank_and_orphaned_tabs_are_other() {
        let mut store = ProfileStore { profiles: vec![] };
        let none = tab(&mut store, None);
        let blank = tab(&mut store, Some("   "));
        let prod = tab(&mut store, Some(" prod "));
        let orphan = {
            let mut p = profile("gone");
            p.group = Some("prod".into());
            p.id = ProfileId::new();
            Tab::single(Pane::for_profile(&p))
        };
        assert_eq!(group_of(&none, &store), Group::Other);
        assert_eq!(group_of(&blank, &store), Group::Other);
        assert_eq!(group_of(&prod, &store), named("prod"), "trimmed");
        assert_eq!(
            group_of(&orphan, &store),
            Group::Other,
            "profile not in the store"
        );
        assert_eq!(group_of(&page(), &store), Group::Featherbit);
    }

    #[test]
    fn group_names_differing_by_case_are_one_group() {
        let groups = [named("Prod"), Group::Other, named("prod")];
        assert_eq!(grouped_order(&groups), vec![0, 2, 1]);
        let order: Vec<Group> = grouped_order(&groups)
            .into_iter()
            .map(|i| groups[i].clone())
            .collect();
        let s = sections(&order, &BTreeSet::new());
        assert_eq!(s.len(), 2);
        assert_eq!(
            s[0].group,
            named("Prod"),
            "titled by the first tab's spelling"
        );
        assert_eq!(s[0].rows, vec![0, 1]);
        assert_eq!(s[1].group, Group::Other);
    }

    #[test]
    fn grouping_is_stable_within_a_group() {
        let groups = [
            named("b"),
            Group::Other,
            named("a"),
            named("b"),
            Group::Featherbit,
        ];
        assert_eq!(grouped_order(&groups), vec![4, 2, 0, 3, 1]);
    }

    #[test]
    fn sections_mark_collapsed_and_visible_rows_skip_them() {
        let groups = [Group::Featherbit, named("a"), named("a"), Group::Other];
        let collapsed = BTreeSet::from(["a".to_string()]);
        let s = sections(&groups, &collapsed);
        assert_eq!(
            s.iter()
                .map(|s| (s.rows.clone(), s.collapsed))
                .collect::<Vec<_>>(),
            vec![(vec![0], false), (vec![1, 2], true), (vec![3], false)]
        );
        assert_eq!(visible_rows(&s), vec![0, 3]);
    }

    #[test]
    fn travel_walks_visible_rows_and_wraps() {
        let rows = [0, 3, 4]; // 1 and 2 are hidden
        assert_eq!(travel(&rows, 0, Step::Next), Some(3));
        assert_eq!(travel(&rows, 4, Step::Next), Some(0));
        assert_eq!(travel(&rows, 0, Step::Prev), Some(4));
        assert_eq!(travel(&rows, 3, Step::Prev), Some(0));
        assert_eq!(
            travel(&rows, 1, Step::Next),
            Some(3),
            "from a hidden tab: the next visible"
        );
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

    use fbterm_config::TabBar;
    use iced::window;

    use crate::app::{App, Message, TabBarMsg};
    use crate::restore_tests::paths_with;
    use crate::tabs::WindowState;
    use fbterm_config::keys::Action;

    /// An app with one window holding one tab per `(name, group)`, in that order, the first
    /// active; folding and the left side off until a test turns them on.
    pub(crate) fn app_with_groups(specs: &[(&str, Option<&str>)]) -> (App, window::Id) {
        let profiles: Vec<_> = specs
            .iter()
            .map(|(name, group)| {
                let mut p = profile(name);
                p.group = group.map(str::to_owned);
                p
            })
            .collect();
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

    pub(crate) fn fold(app: &mut App) {
        app.settings.tab_groups = true;
        app.settings.tab_bar = Some(TabBar::Left);
    }

    #[test]
    fn toggling_a_group_collapses_it_and_saves() {
        let (mut app, wid) = app_with_groups(&[("a", Some("prod")), ("b", None)]);
        fold(&mut app);
        // The window's first update counts as a new active tab (restore unfolds its group):
        // settle that before the click.
        let _ = app.update(Message::TabBar(wid, TabBarMsg::Select(0)));
        let _ = app.update(Message::TabBar(wid, TabBarMsg::ToggleGroup(0)));
        assert!(app.collapsed[&wid].contains("prod"));
        assert_eq!(app.visible_rows(wid), vec![1]);
        let _ = app.update(Message::TabBar(wid, TabBarMsg::ToggleGroup(0)));
        assert!(!app.collapsed[&wid].contains("prod"));
        assert_eq!(app.visible_rows(wid), vec![0, 1]);
    }

    /// The window of the regroup tests: `b(staging) a(prod) c(none) d(prod)`, then a page
    /// tab `p` pushed last, active `d`.
    fn mixed() -> (App, window::Id) {
        let (mut app, wid) = app_with_groups(&[
            ("b", Some("staging")),
            ("a", Some("prod")),
            ("c", None),
            ("d", Some("prod")),
        ]);
        let w = app.windows.get_mut(&wid).unwrap();
        w.push(Tab::page(Page::Settings));
        w.active = 3;
        (app, wid)
    }

    fn names(app: &App, wid: window::Id) -> Vec<String> {
        app.windows[&wid]
            .tabs
            .iter()
            .map(|t| {
                if t.page.is_some() {
                    "p".to_owned()
                } else {
                    t.name().to_owned()
                }
            })
            .collect()
    }

    #[test]
    fn switching_folding_on_sorts_tabs_stably_and_keeps_the_active_tab() {
        let (mut app, wid) = mixed();
        fold(&mut app);
        app.regroup_all();
        assert_eq!(names(&app, wid), ["p", "a", "d", "b", "c"]);
        assert_eq!(app.windows[&wid].active, 2, "d stays active");
    }

    #[test]
    fn a_tab_whose_group_changes_moves_to_the_end_of_its_new_group() {
        let (mut app, wid) = app_with_groups(&[
            ("a", Some("prod")),
            ("x", Some("prod")),
            ("s1", Some("staging")),
            ("s2", Some("staging")),
        ]);
        fold(&mut app);
        app.regroup_all();
        assert_eq!(names(&app, wid), ["a", "x", "s1", "s2"]);
        // The profile's group is edited: x now belongs to staging.
        let x = app
            .profiles
            .profiles
            .iter_mut()
            .find(|p| p.name == "x")
            .unwrap();
        x.group = Some("staging".into());
        app.regroup_all();
        assert_eq!(names(&app, wid), ["a", "s1", "s2", "x"]);
    }

    #[test]
    fn folding_off_keeps_the_order() {
        let (mut app, wid) = mixed();
        fold(&mut app);
        app.regroup_all();
        app.settings.tab_groups = false;
        let mut e = profile("e");
        e.group = Some("prod".into());
        app.windows
            .get_mut(&wid)
            .unwrap()
            .push(Tab::single(Pane::for_profile(&e)));
        app.regroup_all();
        assert_eq!(names(&app, wid), ["p", "a", "d", "b", "c", "e"]);
    }

    #[test]
    fn folding_on_with_tabs_on_top_changes_nothing() {
        let (mut app, wid) = mixed();
        app.settings.tab_groups = true;
        app.settings.tab_bar = Some(TabBar::Top);
        app.regroup_all();
        assert_eq!(names(&app, wid), ["b", "a", "c", "d", "p"]);
    }

    #[test]
    fn a_new_tab_lands_at_the_end_of_its_group() {
        let (mut app, wid) = mixed();
        fold(&mut app);
        app.regroup_all();
        let prod = app
            .profiles
            .profiles
            .iter()
            .find(|p| p.name == "a")
            .unwrap()
            .id;
        let _ = app.open_tab(wid, prod);
        app.regroup_all();
        assert_eq!(names(&app, wid), ["p", "a", "d", "a", "b", "c"]);
        assert_eq!(
            app.windows[&wid].active, 3,
            "the new tab is active, after d"
        );
    }

    #[test]
    fn the_post_update_hook_regroups() {
        let (mut app, wid) = mixed();
        fold(&mut app);
        let _ = app.update(Message::TabBar(wid, TabBarMsg::Select(0)));
        assert_eq!(names(&app, wid), ["p", "a", "d", "b", "c"]);
    }

    #[test]
    fn selecting_a_hidden_tab_expands_its_group_but_collapsing_the_active_one_keeps_it() {
        let (mut app, wid) = mixed();
        fold(&mut app);
        let _ = app.update(Message::TabBar(wid, TabBarMsg::Select(0))); // regrouped to [p, a, d, b, c]
        // Order [p, a, d, b, c]; make b (staging) active, then collapse prod (section 1).
        let _ = app.update(Message::TabBar(wid, TabBarMsg::Select(3)));
        let _ = app.update(Message::TabBar(wid, TabBarMsg::ToggleGroup(1)));
        assert!(app.collapsed[&wid].contains("prod"));
        // Selecting a (hidden) by a click expands prod.
        let _ = app.update(Message::TabBar(wid, TabBarMsg::Select(1)));
        assert!(!app.collapsed[&wid].contains("prod"));
        assert_eq!(app.windows[&wid].active, 1);
        // Collapsing the active tab's own group sticks.
        let _ = app.update(Message::TabBar(wid, TabBarMsg::ToggleGroup(1)));
        assert!(app.collapsed[&wid].contains("prod"));
        assert_eq!(app.windows[&wid].active, 1);
    }

    /// `mixed()` folded and grouped (`[p, a, d, b, c]`), `staging` collapsed, active `a`.
    fn folded() -> (App, window::Id) {
        let (mut app, wid) = mixed();
        fold(&mut app);
        let _ = app.update(Message::TabBar(wid, TabBarMsg::Select(1)));
        let _ = app.update(Message::TabBar(wid, TabBarMsg::ToggleGroup(2)));
        assert_eq!(names(&app, wid), ["p", "a", "d", "b", "c"]);
        assert!(app.collapsed[&wid].contains("staging"));
        assert_eq!(app.windows[&wid].active, 1);
        app.focused = Some(wid);
        (app, wid)
    }

    fn act(app: &mut App, wid: window::Id, action: Action) {
        let _ = app.update(Message::Shortcut(wid, action));
    }

    #[test]
    fn next_and_prev_skip_collapsed_groups_and_never_expand_them() {
        let (mut app, wid) = folded();
        act(&mut app, wid, Action::NextTab);
        assert_eq!(app.windows[&wid].active, 2, "d");
        act(&mut app, wid, Action::NextTab);
        assert_eq!(app.windows[&wid].active, 4, "c: b is hidden");
        act(&mut app, wid, Action::NextTab);
        assert_eq!(app.windows[&wid].active, 0, "p: wrapped");
        act(&mut app, wid, Action::PrevTab);
        assert_eq!(app.windows[&wid].active, 4, "c");
        assert!(app.collapsed[&wid].contains("staging"));
    }

    #[test]
    fn number_shortcuts_count_visible_rows() {
        let (mut app, wid) = folded();
        act(&mut app, wid, Action::SelectTab(3));
        assert_eq!(app.windows[&wid].active, 2, "d is the third visible row");
        act(&mut app, wid, Action::SelectTab(9));
        assert_eq!(app.windows[&wid].active, 4, "c is the last visible row");
        act(&mut app, wid, Action::SelectTab(5));
        assert_eq!(app.windows[&wid].active, 4, "no fifth visible row");
        act(&mut app, wid, Action::SelectTab(4));
        assert_eq!(app.windows[&wid].active, 4, "c again");
    }

    #[test]
    fn with_every_group_collapsed_travel_does_nothing() {
        let (mut app, wid) = folded();
        for section in [0, 1, 3] {
            let _ = app.update(Message::TabBar(wid, TabBarMsg::ToggleGroup(section)));
        }
        assert_eq!(app.visible_rows(wid), Vec::<usize>::new());
        for action in [Action::NextTab, Action::PrevTab, Action::SelectTab(1)] {
            act(&mut app, wid, action);
            assert_eq!(app.windows[&wid].active, 1, "a stays");
        }
    }

    #[test]
    fn closing_the_active_tab_into_a_collapsed_group_expands_it() {
        let (mut app, wid) = folded();
        let _ = app.update(Message::TabBar(wid, TabBarMsg::Select(2))); // d
        let _ = app.update(Message::TabBar(wid, TabBarMsg::Close(2)));
        assert_eq!(names(&app, wid), ["p", "a", "b", "c"]);
        assert_eq!(app.windows[&wid].active, 2, "b took d's place");
        assert!(!app.collapsed[&wid].contains("staging"));
    }

    #[test]
    fn a_fold_matches_group_names_that_differ_only_by_case() {
        let collapsed = BTreeSet::from(["Prod".to_string()]);
        let s = sections(&[named("prod"), Group::Other], &collapsed);
        assert!(s[0].collapsed, "Prod folded means prod folded");
        // In the app: the tab whose spelling titled the header closes, the fold stays.
        let (mut app, wid) =
            app_with_groups(&[("c", None), ("a", Some("Prod")), ("b", Some("prod"))]);
        fold(&mut app);
        let _ = app.update(Message::TabBar(wid, TabBarMsg::Select(0)));
        assert_eq!(names(&app, wid), ["a", "b", "c"]);
        let _ = app.update(Message::TabBar(wid, TabBarMsg::Select(2)));
        let _ = app.update(Message::TabBar(wid, TabBarMsg::ToggleGroup(0)));
        assert!(app.collapsed[&wid].contains("Prod"));
        let _ = app.update(Message::TabBar(wid, TabBarMsg::Close(0)));
        assert_eq!(names(&app, wid), ["b", "c"]);
        assert_eq!(app.visible_rows(wid), vec![1], "prod stays folded");
        // Unfolding the header now titled `prod` removes the `Prod` key.
        let _ = app.update(Message::TabBar(wid, TabBarMsg::ToggleGroup(0)));
        assert_eq!(app.visible_rows(wid), vec![0, 1]);
        assert!(app.collapsed[&wid].is_empty());
    }

    #[test]
    fn switching_the_tabs_to_the_top_and_back_keeps_the_folds() {
        let (mut app, wid) = app_with_groups(&[("a", Some("prod")), ("b", None)]);
        fold(&mut app);
        let _ = app.update(Message::TabBar(wid, TabBarMsg::Select(0)));
        let _ = app.update(Message::TabBar(wid, TabBarMsg::ToggleGroup(0)));
        assert!(
            app.collapsed[&wid].contains("prod"),
            "the active group, folded by hand"
        );
        let _ = app.update(Message::Shortcut(wid, Action::VerticalTabs));
        assert!(!app.settings.tabs_left());
        let _ = app.update(Message::TabBar(wid, TabBarMsg::Select(1)));
        let _ = app.update(Message::TabBar(wid, TabBarMsg::Select(0)));
        let _ = app.update(Message::Shortcut(wid, Action::VerticalTabs));
        assert!(app.settings.tabs_left());
        assert!(app.collapsed[&wid].contains("prod"));
    }

    #[test]
    fn sections_without_folding_are_one_open_section() {
        let (mut app, wid) = app_with_groups(&[("a", Some("prod")), ("b", None)]);
        app.collapsed.entry(wid).or_default().insert("prod".into());
        assert!(!app.groups_on());
        let s = app.sections_of(wid);
        assert_eq!(s.len(), 1);
        assert!(!s[0].collapsed, "nothing is hidden while folding is off");
        assert_eq!(app.visible_rows(wid), vec![0, 1]);
    }
}
