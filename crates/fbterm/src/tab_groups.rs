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

    pub(crate) fn from_key(key: &str) -> Self {
        match key {
            "featherbit" => Self::Featherbit,
            "other" => Self::Other,
            name => Self::Named(name.to_owned()),
        }
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
    fn same(&self, other: &Self) -> bool {
        self.rank() == other.rank()
    }
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
    profiles
        .get(tab.focused_pane().profile)
        .and_then(|p| p.group.as_deref())
        .map(str::trim)
        .filter(|g| !g.is_empty())
        .map_or(Group::Other, |g| Group::Named(g.to_owned()))
}

/// The indices of `groups` in grouped order: stable, so tabs of one group keep their order,
/// and names differing only by case count as one group.
pub(crate) fn grouped_order(groups: &[Group]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..groups.len()).collect();
    order.sort_by_key(|&i| groups[i].rank());
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
                collapsed: collapsed.contains(&g.key()),
                group: g.clone(),
                rows: vec![i],
            }),
        }
    }
    out
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

#[cfg(test)]
mod tests {
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
    fn keys_round_trip() {
        for g in [Group::Featherbit, named("prod"), Group::Other] {
            assert_eq!(Group::from_key(&g.key()), g);
        }
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
}
