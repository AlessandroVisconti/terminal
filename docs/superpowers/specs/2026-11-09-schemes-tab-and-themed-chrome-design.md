# Color schemes tab and themed chrome: design

Status: draft for the owner's review, 2026-11-09. The owner's decisions are marked as theirs;
the calls marked **(call)** are the author's and are open to change. This document changes
`2026-10-27-color-schemes-design.md` where it says so (part B); everything else there stays
binding.

## Part A: Color schemes as a page tab

### A1. Intent

The Color schemes window becomes a page tab like Settings and Profiles (owner's decision,
2026-11-09). A font installed while fbterm runs should show in it without closing and reopening
anything.

### A2. Goals

1. `Page::Schemes` (title `Color schemes`) joins `Page::Settings`, `Page::Profiles` and
   `Page::Welcome`, with the same rules as Settings and Profiles:
   - one in the app;
   - the `Color schemes` action, its shortcut, the palette row, the Settings button
     (`SettingsMsg::EditSchemes`) and the profile editor's button (`EditorMsg::EditSchemes`)
     open it in the current window, or select it where it is open. A request that names a
     scheme (Settings and the editor pass one) selects that scheme, whether the tab is new or
     already open **(call: today an open window only gains focus)**;
   - Cmd+W (Ctrl+W) closes it, Cmd+S saves the edited scheme;
   - not restored at relaunch, never split or merged, grouped with the other page tabs
     (`Group::Featherbit`).
2. **Same contents and behavior as the window today:**
   - the scheme list with its sidebar and footer;
   - the details and editor, the preview and the suggestion line;
   - the Gallery tab, Import/Export, Duplicate and Delete;
   - the edited-theme question and the unsaved-changes confirmation.

   Closing the tab behaves as closing the window does today: there is no unsaved-changes
   question, and unsaved drafts are dropped **(call: parity; a confirmation would be a new
   dialog on the close and quit paths, out of scope here)**.
3. **Width.** It takes the window's width. Below a minimum width it scrolls sideways like the
   other pages **(call: the minimum is the window's current minimum width)**.
4. **The separate window is removed**, with its size, position and close handling. Nothing in
   `state.toml` records the window today; if something does, it is ignored on load.
5. **Fonts installed while fbterm runs.** As the window does today on opening,
   `refresh_installed_fonts` and then the existing scan (`App::scan_fonts`, only while a
   suggested family is missing) runs when the Color schemes tab becomes its window's active
   tab, and when its window gains focus while that tab is active (the existing
   `WindowFocused` message). So switching back to
   fbterm after installing a font updates the line. Both are user actions. No timer, no
   background scan.
6. **Debug aids keep working:**
   - `FBTERM_OPEN_SCHEMES` opens the tab in the first window;
   - `FBTERM_SCREENSHOT_WINDOW=schemes` captures the terminal window showing it, like
     `settings`;
   - `FBTERM_DEMO_GALLERY` is unchanged.

### A3. Non-goals

- No redesign of the scheme list, editor or gallery beyond what fitting a tab needs.
- The export/import window stays a separate window.

## Part B: the app follows the global color scheme

### B1. Intent

The owner's intent (2026-11-09, correcting what `2026-10-27-color-schemes-design.md` section 3
recorded) is:
- the **global** `color_scheme` colors the whole app, chrome included;
- a profile's scheme override colors only that profile's sessions.

Today every scheme rules only inside sessions, and the chrome stays Graphite Dark or Light.

### B2. Owner's decisions (2026-11-09)

1. **The Dark/Light app theme setting is replaced.** The chrome always follows the global
   scheme: a light scheme gives a light window. The `theme` setting and its Settings row go
   away.
2. **The accent comes from the scheme**, not a fixed violet. It applies wherever violet is used
   today: focus rings, selected rows, primary buttons, the drag marker and the sidebar resize
   rule. No new accent treatment is added (the active tab has none today and gets none).
3. **Everything follows:**
   - the tab bar and the left sidebar;
   - the Settings, Profiles, Welcome and Color schemes pages;
   - the file and tunnels panels;
   - dialogs and prompts, the palette, menus and tooltips;
   - the export/import window.

### B3. Goals

1. **One token set from the global scheme.** `theme::Tokens` stops being two constants
   picked by `ThemeMode`. The app keeps one `Tokens`, and the iced `Theme` built from it, for
   the current global scheme. Both are rebuilt only when the global scheme or its colors
   change (setting change, scheme edit, reload), never per frame. Every place that calls
   `theme::tokens(mode)` or `graphite_theme(mode)` reads the app's current set instead, and so
   does every direct use of the `DARK`/`LIGHT` constants outside `theme.rs`'s own tests, such
   as `tab_look::ink`, which picks tab text between the current set's text colors.
   `App::tokens` becomes an owned field.
2. **Built-ins look exactly as today:**
   - `graphite` gives today's `DARK` tokens, value for value;
   - `graphite-light` gives today's `LIGHT` tokens;
   - in both, `terminal` is that scheme's background. That changes `graphite-light`'s
     `terminal` from Graphite Dark's background to its own, which is intended: it is the
     fallback tile background and the find bar's, so it now matches the terminal it frames.

   So a user on the default scheme sees no change **(call: hand-tuned tokens for these two,
   derived tokens for every other scheme)**.
3. **Derivation for every other scheme**, in `theme.rs` as one pure function from
   `SchemeColors` to `Tokens`:
   - **Neutrals:** `app`, `surface`, `raised`, `border` and `hover` come from the background,
     stepped toward the foreground by small fixed amounts in the same order as Graphite's
     (`app` closest to the terminal background, `border` furthest). `terminal` is the scheme
     background.
   - **Text:** `text` is the foreground. `text_secondary` and `text_muted` are the foreground
     mixed toward the background.
   - **Accent:** the scheme's cursor color when it is chromatic and contrasts at least 3:1
     with `app`, else its ANSI blue (bright blue if plain blue fails), else today's violet.
     `accent_soft` and `accent_edge` come from the accent the same way Graphite's do from
     violet.
   - **Status colors:** `success`, `warning` and `danger` (and `danger_soft`) come from ANSI
     green, yellow and red, under the same 3:1 rule, each falling back to Graphite's value
     for the scheme's lightness.
   - **Contrast floors**, raised by moving the color away from the background until met:
     `text` at least 7:1 on `app`, `text_secondary` 4.5:1, `text_muted` 4.5:1. Text drawn
     on the accent (primary buttons, the selected row) is white or near-black, whichever
     contrasts more.

   The exact step amounts are the builder's, tuned so that applying the derivation to
   Graphite's own colors lands close to `DARK`. A test pins how close.
4. **Live.** Changing the global scheme, or saving an edit to it in the Color schemes tab,
   recolors every open window at once, chrome included. A profile's override still changes
   only its sessions.
5. **The removed `theme` key** is accepted and ignored with one settings notice saying the
   window now follows the color scheme, and pointing to Graphite Light for a light window.
   The template line and the docs row are removed **(call: no automatic switch of
   `color_scheme` for users who had `theme = "light"`, because that would also change their
   terminal colors)**.
6. **Translucency and blur are unchanged.** The bar and pages keep painting the token
   colors at `background_opacity` as today. Classic blur's appearance still follows the active
   tab's first pane.

### B4. Non-goals and anti-goals

- No per-tab or per-profile chrome: an override recolors no token-drawn chrome. The one
  exception stays as today: the active tab and the content area behind the tiles take the
  focused pane's scheme background (`window.rs`, `tab_look::look`), because the active tab is
  joined to the terminal it frames. Their text is picked from the current token set.
- No new settings beyond removing `theme`.
- **Anti-goals:**
  - any built-in scheme, or any of the 30 gallery themes, giving chrome text below the
    floors in B3.3;
  - a changed look for users on `graphite`;
  - the chrome recolored by a remote program (it only follows settings and scheme files the
    user changes).

### B5. Tests

- `theme.rs`:
  - `graphite` and `graphite-light` give `DARK` and `LIGHT` exactly, apart from `terminal`;
  - the derivation is pure and deterministic;
  - every built-in and every theme in the terminal-themes catalog (a fixture copy) meets the
    contrast floors;
  - the accent fallbacks are taken in order;
  - Graphite's own colors derive close to `DARK`.
- **App:** changing `color_scheme` rebuilds the tokens once and recolors open windows. A
  profile override does not change them (the active tab still follows the focused pane, as
  today). Editing the global scheme recolors them on save.
- **Settings:** the theme row is gone; an old `theme` key gives the notice once and nothing
  else; the template line is removed.
- **Part A:**
  - the tab opens, selects, closes (with and without unsaved edits) and is never restored;
  - the shortcut, palette, Settings button and profile editor button route to it, selecting
    the named scheme;
  - selecting the tab, or focusing its window, with a suggestion missing starts a scan, and
    neither does when nothing is missing;
  - the existing schemes window tests move to the tab.
- **Screenshots** (throwaway config): the Color schemes tab, and the chrome under Graphite,
  Graphite Light and two gallery themes (one dark, one light).

## Docs

- `website/docs/color-schemes.md`:
  - the global scheme colors the whole app, overrides only their sessions;
  - Color schemes is a tab.
- `website/docs/files.md`: drop `theme`; note on `color_scheme`.
- `CLAUDE.md`: the page tabs list, the scheme resolution and chrome paragraphs, and the status.
  Also the look-and-feel rule: Graphite stays the default look, but the accent is the global
  scheme's and violet is Graphite's own accent (owner's decision B2.2), not a fixed one.
- Mark `2026-10-27-color-schemes-design.md` section 3's chrome bullet as superseded by this
  document.

## Left to the builder

- The derivation's step amounts within B3.3.
- How tokens reach the views (a field on `App` passed down, as `&Tokens` is today).
- Whether parts A and B land as one branch or two (one plan either way).
- The tab's minimum width, if the window's (760) turns out wrong for a tab.
