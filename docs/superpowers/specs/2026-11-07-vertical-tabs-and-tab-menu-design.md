# Vertical tabs and the tab menu: design

Status: implemented on the feature branch for the owner's review (PR to `develop`), 2026-11-07. Two additions to the tab bar, asked for
together: tabs listed down the left side of the window, and a right-click menu on a tab that
copies an SSH profile's saved password; plus a Welcome tab, shown once, that asks which tab
layout to use and takes over the update-check question from its popup (the Settings and
Profiles windows became page tabs in PR #58, so the first-launch questions become a page tab
too). Everything else about tabs stays as the main design
(`2026-10-23-featherbit-terminal-design.md`, sections 5.6 and 5.13) and the tiling design
(`2026-10-26-tiling-design.md`) specify.

## 1. Intent

With twenty sessions open, a row of 196 px tabs across the top of the window runs out of room
long before the sessions do. The owner wants the tabs down the left side instead, in a column
that can be scrolled, so every open session has a readable name. Because it is a new layout,
fbterm asks once which of the two the user prefers, showing both: not in a popup but in a
Welcome tab, which also takes the update-check question a popup asked until now.

Separately: a password profile keeps its password in the saved secrets, and sometimes the
owner needs that password outside fbterm (a web console, `sudo` on the host, a second tool).
Opening the profile editor to read it is slow and the editor hides the value. A right-click on
the session's tab should put the password on the clipboard.

Success: on the first launch after the update the owner sees a Welcome tab with a small
picture of each layout, picks the left one, sees all twenty sessions by name in a column with their
state dots and accents, scrolls the column when it overflows, drags a tab up or down to
reorder it, and right-clicks the `prod` tab to copy its password and paste it into a browser.

## 2. Goals

1. **Tabs on the left.** A `tab_bar` setting with two values, `"top"` (what fbterm does
   today) and `"left"`: the tabs are listed in a sidebar at the left edge of each terminal
   window, under the bar, one row per tab, in tab order, scrolling when they overflow. The bar
   stays (it is the title bar on macOS and carries the drawn window controls elsewhere), with
   its buttons where they are today, after the (now empty) row of tabs, so they sit above the
   sidebar.
2. **Asked once, in a Welcome tab.** A `settings.toml` without `tab_bar` means the question
   was never answered: tabs show on top, and at launch a Welcome page tab opens (like the
   Settings tab) with a drawn preview of each layout to choose from, and the update-check
   question (`check_updates` absent) under it. Each answer is written as soon as it is
   chosen; closing the tab writes the defaults for what was not answered, so it opens once.
   The `Check for updates?` popup goes away.
3. **Switching is one action.** The setting changes from the Settings tab (Appearance), from a
   `Vertical tabs` action with a chord and a palette row, and by editing `settings.toml`.
   Every open window follows at once.
4. **Everything a tab does works in the sidebar:** select, close (button, wheel click, the
   chord), reorder by dragging, drop onto another tab's content to merge as tiles, drag a tile
   onto the sidebar to make it a tab, the accent stripe or whole-tab tint, the state dot, the
   trouble dot, tooltips, page tabs (Profiles, Settings) listed like the others.
5. **A tab menu.** A right-click on a tab, in the bar or in the sidebar, opens a menu at the
   pointer. It holds `Copy password` when the tab's focused pane belongs to an SSH profile
   with password authentication, and `Close tab` always. `Copy password` puts the profile's
   saved password on the clipboard.
6. **A password never leaks by accident.** The menu opens only on the user's own right-click.
   The copies of the password fbterm holds are zeroized once it is on the clipboard, it is
   never logged, and it is cleared from the clipboard a minute later if it is still there and
   fbterm is still running.

## 3. Non-goals and anti-goals

- No resizing of the sidebar in this change (one width), no collapsing it to icons, no tab
  groups, no per-window choice: the setting is global, like every other appearance setting.
- No title or breadcrumb in the bar's empty space when tabs are on the left.
- `Copy password` copies only a saved password. It never asks for a password that is not
  saved, never copies a key passphrase, and never reads the password out of a running session.
- No change to the terminal's own right-click menu's items (`context_menu.rs` may be
  generalized to serve both menus).
- No second clipboard path: the menu uses iced's clipboard, as Copy and Paste do. No
  clearing of the clipboard at quit (see 4.3).
- Anti-goals:
  - Anything a program in a terminal sends opening the tab menu, copying a password,
    switching the tab bar's side or answering the launch question.
  - The password showing on screen, in the log, in a notice or in a tooltip.
  - Losing a session, its scrollback, its position in the tab order or the active tab when the
    bar switches side.
  - The sidebar stealing the keyboard from the terminal.
  - The Welcome tab coming back once its questions are answered or it was closed, opening
    in runs that keep no state (debug aids, tests), or being restored at relaunch.

## 4. Behaviour

### 4.1 Tabs on the left

- **Layout.** Under the bar, the window splits into the sidebar at the left edge and the
  content (tiles, with a docked panel at the right edge as today). The sidebar is 220 logical
  pixels wide and runs the full height under the bar. It draws one hairline rule on its right
  (`ui::vertical_rule`, as the docked panels draw theirs on the left). In a window too narrow
  for 220 px plus the terminal's minimum width (`MIN_TERMINAL_WIDTH`, 200) the sidebar
  yields: its width is what is left above that minimum, down to nothing, and its rows clip.
  A docked panel's width is computed from the width left after the sidebar, so the panel
  still never squeezes the terminal below its minimum.
  The content outline along the content's top starts at the sidebar's rule instead of the
  window's left edge and runs to the docked panel's rule or the window's right edge; there is
  no active-tab outline, no corners and no flares, because no tab sits on the bar.
- **The bar** keeps its height, background, drag-to-move area, drawn controls and the buttons
  (new tab, Profiles, tunnels, files, Settings), which keep their place after the empty tab
  row and so sit above the sidebar. The `BarBackground` paints the
  bar plain, with no active tab to paint around.
- **A row** is the full width of the sidebar, 32 px high, with 8 px side padding: the accent
  stripe (3 by 14 px), the state dot, the trouble dot when the tab is in trouble, the title
  clipped on one line, and the close button at the right end. The active row takes the
  terminal's background color (the focused pane's scheme background at the background
  opacity, as the active tab does on the bar), inactive rows are clear with the bar's hover
  color on hover. A whole-tab tint applies to the row as it would to the tab (`tab_look`):
  inactive lightly, active stronger, text picked for contrast. Corners: 5 px, all four, like
  a button. Rows are spaced 2 px apart, with 6 px padding around the list. The sidebar's
  background is the bar's.
- **Scrolling.** The list scrolls with the shared scrollbar look (`ui::scroll_style`); when
  the window's active tab changes (chord, click, a new tab) and its row is not fully in view,
  the list scrolls just enough to show it.
- **Dragging** works as on the bar, vertically: a press on a row moved past the threshold
  carries the tab; while carried, it reorders by the row it is over, and dropping it on the
  content of the shown tab merges as tiles (the existing `DragMsg` flow). A tile carried onto
  the sidebar shows a gap between rows and becomes a tab there. Dropping on the bar's empty
  area does nothing.
- **Page tabs** (Profiles, Settings) are rows like the others, with no state dot.
- **Switching sides** keeps every tab, its order and the active tab; the window's content
  resizes (the terminals reflow) as it does when a panel docks.
- **Restore** is the setting's: `state.toml` records nothing new.
- **Debug aids:** `FBTERM_TAB_BAR=left` (debug builds only) starts with the setting at `left`
  without writing it, so screenshots show the sidebar; `FBTERM_OPEN_WELCOME=1` opens the
  Welcome tab in the first window (`FBTERM_DEMO_PROMPT=updates` goes away with the popup).

### 4.2 The `Vertical tabs` action and the Settings row

- A new action `VerticalTabs` in `fbterm-config/src/keys.rs`, group `Windows`, titled
  `Vertical tabs`, name `vertical_tabs`, default chord Cmd+Shift+B on macOS and Ctrl+Shift+B
  elsewhere (free in the default table), rebindable like every other action. It flips
  `tab_bar` between `"top"` and `"left"` (an unanswered setting counts as `"top"`, so the
  action makes it `"left"`), writes `settings.toml` and applies to every window, exactly as a
  Settings tab change does. Used from a window without tabs (Color schemes, export and
  import) it still flips the setting.
- The palette lists it with the app actions as `Vertical tabs`.
- The Settings tab's Appearance card gets a `Tabs` row with two choices, `Top` and `Left`,
  built like the `Title bar` row, shown on every system; an unanswered setting shows `Top`.

### 4.3 The tab menu

- **Opening.** A right-click on a tab (bar or sidebar) opens the menu at the pointer, over the
  window, with the same look, placement rules (`menu_origin`), keyboard driving (Up, Down,
  Enter, Escape) and closing rules as the terminal's right-click menu: a choice, Escape, a
  click outside it, the window losing focus or being resized. Not while the palette, the
  find-in-all-tabs box, a dialog or the vault dialog covers the window. Opening the tab menu
  closes the terminal's menu and the other way round (one menu at a time per app).
  A right-click does not select the tab. The menu closes if its tab leaves the window.
- **Items.** `Copy password`, present only when the tab's focused pane belongs to a saved SSH
  profile whose `auth` is `Password`; `Close tab`, always. `Close tab` does what the tab's
  close button does (`TabBarMsg::Close`, asking first when several live panes would close).
  The menu is 196 px wide, 28 px per item, no chord labels (`Close tab` already shows its
  chord in its tooltip, and the chord closes the active tab, not the right-clicked one).
- **Copy password.** The profile's password is read from the saved secrets on the blocking
  pool (`SecretStore::get`, never on the UI thread), handed through a `Delivered` message as
  a `Zeroizing<String>`, written to the clipboard and dropped. No notice on success, as the
  terminal's Copy gives none. Outcomes that are not a password:
  - not saved: the window banner shows `No saved password for <profile>.`, nothing is written
    to the clipboard;
  - vault locked: the main password dialog opens over the window (`ask_main_password`, with
    the copy as its `then` message and nothing on cancel), and the copy runs after the unlock;
  - the system storage fails: the banner shows the storage's error line, as the editor does.
- **Zeroization is best effort.** fbterm's own copies travel as `Zeroizing<String>` and are
  dropped as soon as the clipboard write is issued. `iced::clipboard::write` takes an owned
  `String` that iced's runtime holds briefly and drops unzeroized; that transient copy is
  accepted (no second clipboard path, no new dependency).
- **Clipboard clear.** Sixty seconds after a successful copy, if the clipboard still holds
  exactly the copied password, it is replaced with an empty string. One `Task` with a sleep,
  then a clipboard read and compare; never a subscription or a tick. The comparison value is
  kept zeroized and dropped after the check. A second copy before the first clears restarts
  the minute for the new value; an older check that finds a different value does nothing.
  Limits, documented on the website's saved-secrets page: the clear runs only while fbterm is
  running (quitting within the minute leaves the password where a Copy would), and on Wayland
  a window without focus cannot read or replace the selection, so the clear takes effect
  there only if fbterm has focus when the minute is up.
- **Keyboard** while the menu is up: as the terminal's menu (`Mode::Menu`), no shortcut acts
  and nothing reaches the terminal.

### 4.4 The Welcome tab

- **What it is.** A page tab (`Page::Welcome`, titled `Welcome`) like the Settings and
  Profiles tabs: one in the app, never split, merged or restored at relaunch, closed with the
  close-tab chord, its close button or its own `Done` button. Below 520 px of width it
  scrolls sideways like Settings.
- **When it opens.** At launch, after the windows are restored and before any debug aid runs,
  selected in the terminal window that would take a page tab (`show_page`), when the run
  keeps state (`state_saving_enabled`), `settings.toml` is absent or loads, and at least one
  of `tab_bar` and `check_updates` is absent from it; never in unit tests. The restored active
  tab keeps connecting behind it; connect prompts show over the window as they do today. A
  user upgrading from an earlier release sees it once too (`tab_bar` is new).
- **What it shows.** A page in the Settings page's layout (title, one line, sections,
  footer): `Welcome to fbterm` and `A couple of choices before you start. Everything here can
  be changed later in Settings.` Section `Tabs`: `Where should open sessions be listed?` and
  two cards side by side, `On top` and `On the left`, each a drawn miniature of a window (a
  small frame with the bar and three tab shapes along it, or an empty bar and three row
  shapes down a left column; iced containers in the Graphite tokens, no image file), the
  current choice outlined with the accent. Section `Updates`: the update question's text
  (`fbterm can check GitHub once a day...`, today's `QUESTION_BODY`) and two choices,
  `Check automatically` and `Not now`, none selected until answered. Footer: `Done`.
- **Answering.** Clicking a card or a choice writes that setting at once, through the same
  path as the `Vertical tabs` action, so every window follows while the tab is still open
  (choosing `On the left` moves this very tab into the sidebar) and an open Settings tab
  shows it. Closing the tab in any way writes the defaults for what is still unanswered:
  `tab_bar = "top"` and `check_updates = false`, as Escape was `Not now`. So the tab opens
  at most once per installation unless the lines are removed from `settings.toml`.
- **Keyboard** (`Mode::Welcome`): the close-tab chord closes it; tab navigation, New tab, New
  window, the palette, Quit, Profiles, Settings, Color schemes, Tunnels and Vertical tabs
  act; nothing else.
- **The popup goes away:** `Dialog::CheckUpdates`, its `Not now` / `Check automatically`
  buttons, the due rules in `update.rs` and `FBTERM_DEMO_PROMPT=updates`. The Settings tab's
  Updates switch stays.
- **Debug aid:** `FBTERM_OPEN_WELCOME=1` (debug builds) opens the tab in the first window
  without the due check, for screenshots.

## 5. Decided by the owner (2026-11-07)

- Tabs can be listed vertically, browsable from the left side of the window.
- A right-click on the tab of an SSH session that uses a password copies the password to the
  clipboard.
- Since this is a new layout, the user is asked once at startup whether they prefer vertical
  or regular tabs, with a preview of each in the question.
- Not as a popup: since the Settings and Profiles windows became page tabs, the questions
  fbterm asks at first launch (preferred layout, update checks) go in a Welcome tab.

## 6. Calls made without the owner (check these)

- The side is a global setting `tab_bar = "top" | "left"`, absent until answered (counts as
  `"top"`), not per window, changed in Settings (Appearance, `Tabs` row), by the
  `Vertical tabs` action (Cmd+Shift+B / Ctrl+Shift+B) and by the palette.
- The bar stays with its buttons in their usual place (above the sidebar, where the tab row
  started), empty where the tabs were; the sidebar holds only tabs.
- The sidebar is 220 px wide, not resizable, full height under the bar, with a right rule; in
  a window too narrow for it plus a 200 px terminal it yields width rather than the terminal
  (no change to the minimum window size).
- The active row takes the terminal's background color, like the active tab on the bar; rows
  have 5 px corners.
- The menu holds `Copy password` (only for password SSH profiles) and `Close tab` (always), so
  a right-click always opens a menu and local tabs get a useful one. A right-click does not
  select the tab.
- `Copy password` gives no success notice; a missing password, a locked vault and a storage
  error are handled as in 4.3.
- The clipboard is cleared of the password after sixty seconds, only if it still holds it,
  only while fbterm runs, best effort on Wayland; nothing is cleared at quit. Zeroization is
  best effort through iced's clipboard; no second clipboard path.
- The Welcome tab's wording and layout (the Settings page's), the drawn previews (no image
  file), answers written as chosen, closing the tab writing the defaults for the unanswered
  questions, the restored active tab connecting behind it, and the update popup removed
  rather than kept beside it.
- Debug aids `FBTERM_TAB_BAR=left` and `FBTERM_OPEN_WELCOME=1`; `FBTERM_DEMO_PROMPT=updates`
  removed.

## 7. Left to the builder

- Whether the sidebar reuses the `TabBar` widget's press, drag and gap logic with a vertical
  geometry, or gets a sibling widget sharing the helpers; the exact row look within the
  Graphite tokens; how the context menu module generalizes to two menus (one `ContextMenu`
  with a target and an item list, or a second small struct sharing `menu_origin`, the layer
  widget and `item_view`); the miniatures' exact geometry.
- Tests: the setting's parse and default, the action's toggle and write, the palette row,
  the Settings row's change, the menu's items by profile kind, the copy's outcomes with the
  in-memory store, the clipboard-clear comparison, the Welcome tab's due rules, answers and
  close defaults (the update popup's tests move over), the sidebar's geometry and
  scroll-into-view arithmetic, and the existing drag and restore tests run with the sidebar
  on.
- Docs: a `tabs.md` page on the website (the tab bar, vertical tabs, the Welcome tab, the
  tab menu), `keyboard-shortcuts.md` for the new action, `saved-secrets.md` for copying a
  password and the clipboard clear's limits, `files.md`'s update-check paragraph (the
  question is in the Welcome tab now), CLAUDE.md's debug aids, update-check note and status,
  and the sample in `settings.rs`.
