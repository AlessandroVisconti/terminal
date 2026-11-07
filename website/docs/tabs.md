---
id: tabs
title: Tabs
sidebar_label: Tabs
---

Every open session is a tab. A tab shows its profile's name, a dot for the connection state (connecting, connected, disconnected) and the profile's accent as a stripe or as a tint over the whole tab (**Color as** in the profile editor). A tab with [split panes](tiling) shows the focused pane's name and a second dot when another pane of the tab is disconnected or its program has exited.

## The tab bar

Tabs sit along the top of the window, with the buttons for a new tab, Profiles, tunnels, the file panel and Settings after them. Click a tab to show it, its close button or a wheel click to close it, and drag a tab left or right to reorder it. Dragging a tab onto another tab's terminal merges it as a pane there; dragging a pane's handle onto the bar makes it a tab of its own (see [Moving tiles](tiling#moving-tiles)).

## Tabs on the left

With many sessions open, a row of tabs runs out of room. Choose **Tabs: Left** in Settings (Appearance) or press `Cmd+Shift+B` (`Ctrl+Shift+B` on Linux and Windows) to list the tabs down the left side of every window instead. The palette offers the same as **Vertical tabs**, and `tab_bar = "left"` in [`settings.toml`](files#settingstoml) does the same by hand; `"top"` puts them back.

![Tabs listed down the left side of the window, with the terminal beside them](/img/tabs-left.png)

Everything a tab does works in the column: select, close, reorder by dragging up and down, drop onto a tab's terminal to merge, drop a pane's handle between rows to make it a tab. The column scrolls when the tabs overflow, and the tab you select (by click or shortcut) scrolls into view. The bar keeps its buttons. In a window too narrow for the column and a 200 pixel terminal, the column gives way so the terminal keeps its width.

## The tab menu

Right-click a tab, on the bar or in the column, for a small menu:

- **Copy password** puts the saved password of the tab's SSH profile on the clipboard. It appears only for an SSH profile that logs in with a password, and copies only what is saved: nothing is asked for, and nothing is read from the running session. See [Copying a password](saved-secrets#copying-a-password) for what happens when no password is saved, when the vault is locked, and how the clipboard is cleared afterwards.
- **Close tab** does what the tab's close button does, asking first when the tab has several live panes.

Each item shows its shortcut when the key would do the same thing: the menu of the active tab shows `Cmd+Shift+K` (`Ctrl+Shift+K`) beside **Copy password**, and `Cmd+W` (`Ctrl+Shift+W`) beside **Close tab** when the tab has one pane (with several, the key closes only the focused pane). The menu of another tab shows no shortcuts, because the keys act on the active tab.

A right-click never selects the tab. Only your own right-click opens the menu: nothing a program in a terminal sends can.

## The Welcome tab

The first time fbterm starts without `tab_bar` or `check_updates` in `settings.toml` (a fresh install, or the first launch after upgrading to a version with these settings), a **Welcome** tab opens beside your sessions and asks two things: where open sessions should be listed, with a small picture of each layout, and whether fbterm may [check for updates](files#update-check). Each choice applies and is saved as soon as you click it, so you can see the tabs move before deciding. **Done**, the tab's close button or the close-tab shortcut closes it; whatever you did not answer keeps its default (tabs on top, no update check), and the tab does not come back. Quitting with it still open leaves the questions open, so it shows again next time.

![The Welcome tab: a picture of each tab layout to choose from, and the update check question](/img/welcome.png)
