# Renaming tabs

Status: implemented on `feature/tab-rename`. Owner's request, 2026-11-09: a user may double
left click a tab and rename it, or right click and choose rename in the tab menu, with a
keyboard shortcut too; page tabs cannot be renamed. Everything else below is a builder's call.

## Behavior

- **Starting.** A double-click on a terminal tab (bar or sidebar; two left presses on the same
  tab within iced's double-click interval and distance), the tab menu's `Rename tab`, or the
  `rename_tab` action (`Rename tab`, group Tabs, default Cmd+Shift+R on macOS and Ctrl+Shift+R
  elsewhere; plain Ctrl+R stays the shell's reverse search) for the active tab. The palette lists
  the action; it needs a terminal tab, so on a page tab it is hidden and the shortcut does
  nothing. Not while the palette, the find-in-all-tabs box, a dialog or the vault dialog covers
  the window (`App::menu_covered`).
- **Page tabs** (Profiles, Settings, Color schemes, Welcome) are never renamed: no menu item, no
  double-click, the shortcut does nothing.
- **The field.** An inline text field in place of the tab's label (bar: 12.5 px; sidebar: the
  row's label size), the Graphite input style with a 4 px radius, holding the label the tab shows,
  all selected. One rename at a time in the app; starting another keeps the first one's text.
- **Ending.** Enter or a press of any mouse button outside the field, or the window losing focus,
  keeps the typed name; Escape keeps the old one. A rename ends (keeping its text) when its tab
  leaves the window or something covers the window. Closing the tab drops it.
- **Keys.** While the field is open its window's keys are `Mode::Rename`: Escape and Enter as
  above, bound shortcuts dropped, everything else to the field. The window's terminals are drawn
  unfocused, so nothing typed reaches a session.
- **The name.** Trimmed, control characters dropped, at most 120 characters. An empty name
  clears it (the automatic label returns). Committing the automatic label unchanged pins
  nothing, so an unnamed tab keeps following its focused pane.
- **Where it shows.** `Tab::label` replaces the profile name on the bar (`Tab::shown_name`), and
  the program's title in the sidebar row, its tooltip and the window title
  (`Tab::shown_title`); also the ghost of a carried tab. It belongs to the tab, not a pane: it
  stays across focus changes and reorders, a merge keeps the target tab's name (the merged
  tab's is dropped with it), and `Move tile to new tab` makes an unnamed tab.
- **Saved.** `SavedSplitTab.label` (optional, omitted when unset). A named tab is always written
  in the split form, even with one unzoomed pane; older versions ignore the key and restore the
  tab unnamed.
- **Security.** Only the user's own input starts or commits a rename; nothing a program writes
  (titles, OSC sequences) changes the name.

## Debug aid

`FBTERM_DEMO_RENAME=<text>` (debug builds): about a second after startup the first window's
active tab shows the rename field holding that text; a screenshot waits for it.
