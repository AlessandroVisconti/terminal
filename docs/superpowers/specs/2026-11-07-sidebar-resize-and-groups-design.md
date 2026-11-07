# Sidebar width and tab groups: design

Status: agreed with the owner on 2026-11-07, for a reviewer's read before planning. Two
additions to the tab sidebar (`tab_bar = "left"`, designed in
`2026-11-07-vertical-tabs-and-tab-menu-design.md`): a sidebar the user can resize per window,
with the full tab name as a tooltip, and an optional folding of the rows into the profiles'
groups. Everything else about the sidebar, the bar and the tab menu stays as that design
specifies.

Build order: part A (sections 4.1 to 4.3) first as its own feature branch and pull request;
part B (sections 4.4 to 4.6) after it, on its own.

## 1. Intent

Along the top of the window, how much of a tab's name shows is decided by how many tabs are
open: the width per tab shrinks with the count. Down the left side that is not the case. Every
row is as wide as the sidebar, and the row clips the name at the sidebar's edge on one line,
so the sidebar's width is the one knob that decides how much of a name the user sees. Today
that knob is welded at 220 px. A user with profiles named `prod-eu-west-1-bastion` wants the
sidebar wider; a user on a laptop with short names wants it narrower.

Then, with twenty rows down the left, a flat list is hard to scan. Profiles already carry an
optional group name (the Profiles page lists them by it). With folding on, the rows are
sectioned under their group, each section collapsible, so the user folds away the groups not
in use and keeps the others in order.

Success: the owner drags the sidebar's rule in one window until the longest production host
name shows whole, that width survives a relaunch of that window while another window keeps
its own, hovering a clipped name shows the whole name, and with folding on the twenty open
sessions show as `featherbit`, `production`, `staging` and `other`, `staging` collapsed with
its count, and dragging a `production` tab moves it only among the `production` rows.

## 2. Goals

Part A:

1. **Resizable sidebar, per window.** The user drags the sidebar's right rule to set its
   width, from 140 to 480 logical pixels (default 220). Each window keeps its own width,
   remembered in `state.toml` with that window and restored with it.
2. **The full name on hover.** A row whose name is cut off shows the whole name in a
   tooltip, like the bar's buttons show theirs.
3. **The number shortcuts cover the tail.** The ninth number shortcut selects the last tab
   instead of the ninth, so with more than nine tabs the last one is always a chord away.

Part B:

4. **Tab groups.** A `tab_groups` setting (default off) folds the sidebar's rows into the
   groups of their profiles: a header per group, the rows under it, every header collapsible,
   a collapsed header showing how many tabs it hides. The page tabs (Profiles, Settings,
   Welcome) live in a privileged group named `featherbit`, always first.
5. **Order within a group.** With folding on, dragging a row reorders it only among the rows
   of its own group; the groups' order is fixed (section 4.4).
6. **Keyboard travel follows what is shown.** Next and previous tab and the number shortcuts
   walk the rows the user can see, skipping collapsed groups; selecting a hidden tab another
   way unfolds its group.

## 3. Non-goals and anti-goals

- No wrapping of a name onto two lines; rows stay single-line and clip.
- No collapsing of the sidebar to an icon rail; the minimum width is a width.
- No sidebar width in `settings.toml`: it is window state, like the panels' widths.
- No groups along the top: `tab_groups` has no effect while `tab_bar` is `"top"`, and the
  Settings row for it shows only while tabs are on the left.
- No editing of groups from the sidebar (renaming, creating, moving a profile to another
  group): groups are edited in the Profiles page, as today. Dragging a tab into another group
  is refused, never a change to the profile.
- No change to the width of the file panel or the tunnels panel, which stay one width for
  every window.
- Anti-goals:
  - A remote program resizing the sidebar, folding a group or switching the setting.
  - Losing a tab, its position in its group or the active tab when folding is switched on or
    off, when a group collapses or when a window is restored.
  - The sidebar growing past what leaves the terminal its minimum width.
  - A tooltip that shows the name of a row the pointer is not over, or that stays after the
    pointer leaves.

## 4. Behaviour

### 4.1 Resizing the sidebar

- **The handle.** The sidebar's right rule and a band of 4 px on the content's side of it
  (over the left edge of the tiles) is the grab zone, as a pane divider's grab room lies over
  the panes next to it. Nothing of the band lies over the sidebar: its rightmost 6 px are the
  list's floating scrollbar (`ui::edge_scrollbar`), which keeps every press it gets today. A
  press in the band never reaches the terminal under it (the grab is taken in the window
  layer before the tiles see it, as a docked panel's edge is). Over the zone the pointer
  shows the horizontal-resize cursor (`mouse::Interaction::ResizingHorizontally`); while a
  drag is on, the rule draws in the accent color (`accent_edge`).
- **The drag.** A press in the grab zone starts a resize, never a tab drag; moving the
  pointer sets the width to the pointer's x less the sidebar's left edge, clamped to
  `MIN_SIDEBAR_WIDTH` (140) ..= `MAX_SIDEBAR_WIDTH` (480); the release ends it and saves
  state. Two messages, as the file panel has: one per move with the new width, one at the
  end. The content reflows live (the terminals resize through the existing resize debounce).
  A drag ends without a release when the pointer leaves the window or the window loses focus,
  as a pane divider drag does. Not while the palette, the find-in-all-tabs box, a dialog or a
  menu covers the window (the `modal` guard the sidebar already has).
- **Double-click** in the grab zone sets the width back to the default (220), like a
  double-click on a pane divider equalizes its split, using the divider's double-click
  interval.
- **The narrow-window rule stays.** `sidebar_width` keeps yielding the width above
  `MIN_TERMINAL_WIDTH`, down to nothing, in a window too narrow for the chosen width: the
  chosen width is what the user set, the drawn width is the lesser of the two. A drag in a
  narrow window can set a width larger than what shows; the extra appears when the window
  grows. The drawn width is what the hairline, the dock and the outline use, as today.
- **Per window.** `App` keeps a width per terminal window (default 220 for a window that
  never had one). A new window, opened any way, starts at the default, not at the width of the
  window it was opened from. `SavedWindow` gains `sidebar_width: Option<f32>`, written only
  when it differs from the default and skipped when `None`, clamped on load (a value that is
  not a finite number reads as the default), restored with the window. Windows restored from
  a `state.toml` written before this change have no width and take the default.
- **Switching sides** (`tab_bar`) does not reset widths: a window that goes to top and back
  to left shows its width again. Widths are kept while the setting is `"top"` and still
  written to state, so they survive a relaunch in between.

### 4.2 The name tooltip

- A row whose text does not fit its label (the label is the row's width less the stripe, the
  dots, the close button and the paddings) shows that whole text in a tooltip on hover, with
  the sidebar's tooltip look and delay (`tooltip::tip`, as the close button's). A row whose
  text fits shows no tooltip. The tooltip shows exactly what the row draws, which today is
  the focused pane's title (`Tab::title`: the profile name until a program sets a title, the
  profile name again on reconnect); the fit rule measures that same string. What the row
  draws is not changed by this design.
- The tooltip never shows while a tab or tile is being carried, while the sidebar is being
  resized or while the window is modal; it hides when the pointer leaves the row.
- How the fit is measured is the builder's (section 7); if measuring the clipped label in
  iced turns out impractical, the fallback is to show the tooltip on every row.

### 4.3 The ninth number shortcut selects the last tab

- `Action::SelectTab(9)` selects the window's last tab (with folding on, the last visible row,
  section 4.6). Its title becomes `Last tab`; its name stays `select_tab_9`, so a user's
  override in `[keys]` keeps working. `SelectTab(1)` to `SelectTab(8)` select the first to
  eighth as today. With nine or fewer tabs, nothing changes for the user except the title.
- The palette, the Settings tab's Keyboard section and the docs show the new title.

### 4.4 Tab groups

- **The setting.** `tab_groups = true | false` in `settings.toml` (default `false`), a
  `SettingChange` like `tab_bar`, applied to every window at once, changed from the Settings
  tab and by editing the file. The Settings tab's Appearance card gets a `Group tabs` switch
  right under the `Tabs` row, with the line `Fold the sidebar into collapsible groups; tabs
  reorder within their group`, shown only while the `Tabs` row says `Left`. No action, no
  chord, no palette row.
- **Which group a tab belongs to.** The group of the tab's focused pane's profile
  (`Profile::group`, trimmed), the same pane whose name, accent and state the row shows, so a
  tab of merged panes from different groups follows its focused pane, as its name does. A
  profile without a group, a profile that no longer exists and a local shell without a group
  belong to the group `other`. Page tabs belong to `featherbit`.
- **The groups' order.** `featherbit` first, when at least one page tab is open; then the
  named groups alphabetically, case-insensitive, as the Profiles page lists them; `other`
  last. A header appears only for a group with at least one open tab, so an empty sidebar
  shows no headers.
- **The tab order is the grouped order.** While folding is on, the window's tab order itself
  is kept grouped: switching folding on sorts the tabs stably into group order (within a
  group they keep the order they had), a new tab is inserted at the end of its group, a tab
  whose group changes (its focused pane changes, the profile's group is edited in the
  Profiles page, a pane's profile is replaced on reconnect) moves to the end of its new group,
  and a tile carried onto the sidebar becomes a tab at the end of its profile's group.
  Switching folding off keeps the order as it is, and the user may then reorder freely. So
  there is one tab order per window, the one `state.toml` already records, and the number
  shortcuts, next and previous tab and restore keep reading it.
- **Reordering.** A row carried along the sidebar shows the gap marker only at positions
  within its own group (between its group's rows, including the group's ends) and is placed
  there on release; released anywhere else on the sidebar, including over a header or in
  another group, it goes back where it was. Dropping it on the content to merge as tiles
  works as today. A page tab reorders within `featherbit` as it does today in the flat list.
- **Headers.** A header is a 26 px row above its group with 8 px above it (none for the first
  header): a chevron (down when expanded, right when collapsed, Lucide `chevron-down` and
  `chevron-right` in the muted text color), the group's name in the muted text color at 11
  px, sentence case as given by the user (`featherbit` in the accent's light tone,
  `accent_edge`), and, when collapsed, a pill badge at the right end with the count of hidden
  tabs in Geist Mono, raised surface background, border color edge, 16 px high (the look of
  the tunnels indicator's count). A header has no close button, no state dot, no tooltip and
  no menu; a right-click on it does nothing. Clicking anywhere on a header toggles its group.
  A header is never the active row and takes no keyboard focus.
- **Collapsing.** A collapsed group's rows are not drawn and take no room; the header stays.
  Collapsing the active tab's group is allowed: the tab stays active and shown in the
  content, and the collapsed header's badge draws in the accent color to say the active tab
  is inside. Selecting a tab inside a collapsed group by any means that is not the keyboard
  travel of 4.6 (the palette, a profile's double-click in the Profiles page, a tile drop, a
  new tab opened into that group, restore) expands the group; then the list scrolls to show
  the row as it does today (`reveal_active_tabs`).
- **What is remembered.** The collapsed groups of each window are kept per window and
  written to `state.toml` as `SavedWindow.collapsed_groups: Vec<String>` (skipped when
  empty), by name, restored with the window; a name with no open tabs in that window is kept
  in the list and takes effect when a tab of that group opens. Switching folding off keeps
  the lists; switching it on applies them again.
- **Scrolling and drags** work over the sectioned list as over the flat one: headers scroll
  with the rows, the active row is scrolled into view, and a tile carried over the sidebar
  shows its gap in the group of the tile's profile (at that group's end when the pointer is
  over another group or a header).

### 4.5 Switching folding on and off, side by side with `tab_bar`

- The setting takes effect only while tabs are on the left. With tabs on top the bar keeps
  the flat order (grouped, if folding sorted it) and shows no groups.
- Switching `tab_bar` to `"left"` with folding on sorts the tabs into groups then; switching
  to `"top"` leaves the order alone.
- The `Vertical tabs` action, the Welcome tab and the Settings `Tabs` row are unchanged.

### 4.6 Keyboard travel with folding on

- `NextTab`, `PrevTab` and `SelectTab(1..=8)` walk the visible rows in sidebar order (the tab
  order with the rows of collapsed groups left out, headers not counted), wrapping as today;
  `SelectTab(9)` selects the last visible row. When every tab is hidden except the active one,
  next and previous do nothing. These never expand a group.
- Closing the active tab selects the tab the flat list would have selected today; if that tab
  is in a collapsed group, the group expands (it is a selection by other means, 4.4).
- The tab menu's `Close tab` and `Copy password` act on the right-clicked row as today.

## 5. Decided by the owner (2026-11-07)

- The sidebar's width can be changed by the user, each window keeping its own.
- Rows stay single-line: no wrapping, and a minimum width rather than an icon rail.
- A tab's whole name shows as a tooltip on hover.
- Vertical tabs can display, and organize, the open tabs within their profile group, when
  profiles have one: headers collapsible, a collapsed header showing how many tabs it hides.
- With folding on, tabs reorder only within their own group.
- The page tabs go in a privileged group named `featherbit`.
- Folding is a toggle. The owner saw mockups of the folded sidebar, the Settings row and the
  count as a pill badge (rather than a plain muted number) and approved them.
- Capitalizing the product name is deferred; nothing in this design changes the name.

## 6. Calls made without the owner (check these)

- Width limits 140 to 480, default 220 unchanged; the narrow-window yield applies on top of
  the chosen width; a new window starts at the default rather than inheriting.
- The grab zone is the rule plus 4 px on the content's side, none over the sidebar, so the
  list's floating scrollbar keeps its presses; the rule turns accent while dragged;
  double-click resets to the default.
- The tooltip shows only when the row's text is cut off (fallback: always), and shows the
  text the row draws (the pane's title), leaving what the row draws unchanged.
- `SelectTab(9)` becomes `Last tab`, its name unchanged.
- The setting is `tab_groups`, off by default, a Settings switch under `Tabs` shown only with
  tabs on the left; no action or palette row.
- The group of a tab is its focused pane's profile group; ungrouped, local and orphaned tabs
  go under `other`; groups sort as the Profiles page sorts them, `featherbit` first, `other`
  last; a header shows only for a group with open tabs.
- The window's tab order is kept grouped while folding is on (sorted stably when folding or
  the left side is switched on; new tabs and group changes go to the end of their group), so
  there is one order for the sidebar, the shortcuts and `state.toml`; switching off keeps the
  order.
- A drag outside the row's group is refused (snaps back), not clamped to the group's nearest
  end.
- Headers: 26 px, chevron, muted 11 px name, `featherbit` tinted with the accent, the pill
  badge in Geist Mono, click anywhere to toggle, no menu.
- Collapsing the active tab's group is allowed, its badge in the accent; selection by other
  means than keyboard travel expands the group; keyboard travel skips collapsed groups and
  the last-tab chord picks the last visible row.
- Collapsed groups are remembered per window in `state.toml` by name.

## 7. Left to the builder

- How the resize is wired: a grab zone in the sidebar widget (`Rows` in `tab_sidebar.rs`)
  or in the window layer that the docked panels' edge uses; where the per-window width and
  collapsed set live on `App` (one map keyed by `window::Id`, or fields on the window's
  state); how the sidebar reports the clipped label (measuring with iced's `Paragraph` in the
  widget, comparing the laid-out width with the label's bounds, or the always-on fallback).
- How the grouped order is computed and kept (a `group_of(tab)` helper over the profile
  store, a stable sort applied at the moments listed in 4.4, and a visible-rows iterator
  shared by the sidebar, `next`, `prev` and `select_number`); whether headers are rows of
  the same `Rows` widget with a different kind or a separate element between group
  columns, as long as drags and scrolling behave as specified.
- Tests: the width clamp and default, the state round trip (per window, absent for the
  default, old files), the drag messages and the double-click reset, the narrow-window
  interaction, the tooltip's fit rule, `SelectTab(9)` with fewer and more than nine tabs,
  the setting's parse and default, grouping and sort order (named, `other`, `featherbit`,
  case-insensitivity, a tab's group following its focused pane), the sort at switch-on and
  the order kept at switch-off, insertion at the group's end, refused cross-group drags and
  allowed in-group ones, collapsed-state round trip, keyboard travel over collapsed groups,
  expansion on selection by other means, and the existing sidebar drag and restore tests run
  with folding on.
- Docs: `tabs.md` (resizing, the tooltip, groups and folding, what the last-tab chord does),
  `keyboard-shortcuts.md` (`Last tab`), the sample in `settings.rs` (`tab_groups`),
  `CLAUDE.md` (the sidebar paragraph in the gotchas, state fields, status), and
  `CHANGELOG.md` on the release branch.
