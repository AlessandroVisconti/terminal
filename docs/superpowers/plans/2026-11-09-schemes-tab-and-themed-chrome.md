# Color schemes tab and themed chrome Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The whole app takes its colors from the global color scheme, and the Color schemes
window becomes a page tab that notices fonts installed while fbterm runs.

**Architecture:**
- `theme.rs` gains a pure derivation from `SchemeColors` to `Tokens`. Graphite and Graphite
  Light map to today's hand-tuned constants.
- The app keeps one interned `&'static Tokens` and one iced `Theme` for the global scheme,
  rebuilt in one place (`App::retheme`).
- The `theme` setting is removed.
- The Color schemes UI moves from its own window into `Page::Schemes`, reusing
  `SchemesState` and its view unchanged.

**Tech Stack:** Rust, iced 0.14, the existing `fbterm-config` scheme and settings types.

**Spec:** `docs/superpowers/specs/2026-11-09-schemes-tab-and-themed-chrome-design.md`

## Global Constraints

- **Default look.** Users on `graphite` see no change: `graphite` gives `theme::DARK` value
  for value, and `graphite-light` gives `theme::LIGHT`. In both, `terminal` is that
  scheme's background.
- **Contrast floors** for derived tokens: `text` ≥ 7:1 on `app`; `text_secondary` and
  `text_muted` ≥ 4.5:1 on `app`; accent and status colors ≥ 3:1 on `app`.
- **Accent order:** the cursor color if chromatic and ≥ 3:1, else ANSI blue (index 4), else
  bright blue (index 12), else violet `#7c49e3`.
- **No recoloring per frame:** tokens and the theme are rebuilt only when the global scheme
  or its colors change.
- **A profile override never changes tokens.** The active tab and the content behind the
  tiles keep following the focused pane's scheme background, as today.
- **Copy:**
  - tab title `Color schemes`;
  - removed-key notice: `theme is no longer used: the window follows color_scheme. For a light window, choose Graphite Light.`
- **Repo rules:**
  - Conventional Commits, no attribution lines;
  - never name other terminal applications;
  - code builds on macOS, Linux and Windows;
  - the gate is `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`
    and `cargo test --workspace`;
  - screenshots only via `FBTERM_SCREENSHOT` with `FBTERM_CONFIG_DIR=$(mktemp -d)` and
    `FBTERM_SECRETS=memory`.

## Review Focus

1. **The global `color_scheme` names a missing or deleted scheme.** The chrome uses Graphite
   tokens, like the terminal fallback. (Task 3,
   `missing_global_scheme_gives_graphite_chrome`.)
2. **A degenerate scheme**: foreground equal to background, cursor equal to background, gray
   ANSI colors. The floors still hold and no color is NaN. (Task 1,
   `degenerate_scheme_still_meets_floors`.)
3. **An unsaved edit of the global scheme** in the Color schemes tab does not recolor the
   chrome; saving does. (Task 3, `editing_the_global_scheme_recolors_on_save_only`.)
4. **The window hosting the Color schemes tab closes, or the app quits** with it open. The
   state is cleared, and reopening makes a fresh tab in a focused window. (Task 4,
   `closing_the_host_window_clears_the_schemes_tab`.)
5. **`settings.toml` edited on disk** to change `color_scheme` recolors the chrome, and an
   old `theme = "light"` gives the notice exactly once across reloads. (Task 2,
   `old_theme_key_notices_once`; Task 3, `file_change_of_color_scheme_retheme`.)

---

### Task 1: Token derivation from a scheme

**Files:**
- Modify: `crates/fbterm/src/theme.rs`
- Create: `crates/fbterm/tests/fixtures/terminal-themes-catalog.toml`, copied from
  `../terminal-themes/catalog.toml` at its current `main`. Add a one-line comment at the top
  naming the source repository.

**Interfaces:**
- Produces:
  - `pub fn tokens_for(id: &str, colors: &SchemeColors) -> &'static Tokens`. It returns
    `&DARK` with `terminal` replaced for `"graphite"`, `&LIGHT` likewise for
    `"graphite-light"`, and the derived set otherwise. Results are interned in a process-wide
    `Mutex<Vec<&'static Tokens>>`, looked up by value, so equal inputs return the same
    pointer and memory is bounded by the distinct color sets seen.
  - `pub fn derive(colors: &SchemeColors) -> Tokens`: pure, never interned.
  - `pub fn chrome_theme(t: &Tokens) -> iced::Theme`, replacing `graphite_theme(mode)`. Same
    extended-palette overrides as today. The text on `primary` is white or `#101013`,
    whichever contrasts more with `t.accent`.
  - `fn contrast(a: Color, b: Color) -> f32` (WCAG ratio) for the tests and for
    `tab_look`. Reuse `fbterm_config::schemes::contrast_ratio` if its types fit.
- Consumes: `fbterm_config::schemes::{SchemeColors, builtins}`,
  `fbterm_config::catalog::parse_catalog`.

- [ ] **Step 1: Write the failing tests** in `theme.rs`'s test module:
  - `graphite_maps_to_dark_exactly`:
    `*tokens_for("graphite", &graphite.colors) == DARK` with `terminal` set to Graphite's
    background.
  - `graphite_light_maps_to_light_with_its_own_terminal`: the same with `LIGHT`, and
    `terminal` equals Graphite Light's background, which is not `0x0b0b0e`.
  - `derive_is_deterministic_and_interned`: two calls with equal colors give pointer-equal
    `&'static`.
  - `every_builtin_and_catalog_theme_meets_floors`: for each of `builtins()` and each theme
    of `parse_catalog(include_bytes!("../tests/fixtures/terminal-themes-catalog.toml"))`,
    the floors from Global Constraints hold on `tokens_for(id, colors)`.
  - `accent_fallbacks_in_order`: a chromatic cursor gives the cursor. A gray cursor gives
    ANSI blue. A low-contrast ANSI blue gives bright blue. Both blues low-contrast gives
    `#7c49e3`.
  - `degenerate_scheme_still_meets_floors`: bg = fg = cursor = `#808080`, all ANSI
    `#808080`. The floors hold and every channel is finite.
  - `graphite_derives_close_to_dark`: `derive(&graphite.colors)` differs from `DARK` by at
    most 0.03 per channel on `app`, `surface`, `raised`, `border`, `text`, `text_secondary`
    and `text_muted`.
  - Update `terminal_token_is_the_terminal_background` to the new rule.
- [ ] **Step 2: Run** `cargo test -p fbterm --bin fbterm theme::` and expect it to fail to
  compile (no `tokens_for`).
- [ ] **Step 3: Implement `derive`, `tokens_for` and `chrome_theme`.**
  - **Neutrals:** mix the background toward the foreground by fixed fractions, in Graphite's
    order (`app` smallest, `border` largest). Tune them so that
    `graphite_derives_close_to_dark` passes.
  - `hover`: the foreground at about 4.5% alpha.
  - **Floors:** move a text or accent color away from `app`'s luminance until the floor is
    met (mix toward white on dark, toward black on light).
  - `accent_soft`: the accent at 14% alpha on dark, 9% on light.
  - `accent_edge`: the accent mixed 30% toward white on dark, and the accent itself on light,
    as Graphite does.
  - Status colors: ANSI green 2, yellow 3 and red 1 under the 3:1 rule, else Graphite's
    value for the scheme's lightness (`schemes::is_dark`).
  - Keep `graphite_theme` only if something outside the tests still needs it; otherwise
    delete it.
- [ ] **Step 4: Run** `cargo test -p fbterm --bin fbterm theme::` and expect PASS.
- [ ] **Step 5: Commit:** `feat(theme): chrome tokens derived from a color scheme`.

### Task 2: Remove the `theme` setting

**Files:**
- Modify: `crates/fbterm-config/src/settings.rs` (field `theme`, the default, the template
  line `# Color theme: "dark" or "light".` and the line after it, `ThemeMode`),
  `crates/fbterm-config/src/settings_edit.rs` (`SettingChange::Theme`),
  `crates/fbterm-config/src/lib.rs` (exports), `crates/fbterm-config/tests/config.rs`.
- Modify: `crates/fbterm/src/settings_window/{mod.rs,view.rs,glue.rs,tests.rs,app_tests.rs}`
  (the Light/Dark row and `SettingsMsg::Theme`), `crates/fbterm/src/context_menu.rs`
  (tests).

**Interfaces:**
- Produces: `Settings` without `theme`, and no `ThemeMode` or `SettingChange::Theme`
  anywhere.
- `Settings::load_checked` returns the removed-key notice (Global Constraints copy, with
  `line` = the key's line) when the file has a top-level `theme` key and no other problem.
  If another problem is reported, that one wins, following the existing one-warning rule.

- [ ] **Step 1: Write failing tests** in `crates/fbterm-config/tests/config.rs`:
  - `old_theme_key_notices_once`: a file with `font_size = 14.0\ntheme = "light"\n`.
    `load_checked` returns the notice with `line == Some(2)` and `font_size == 14.0`.
    Loading it again returns the same single notice; it is the same text each load, and the
    banner already shows each distinct line once.
  - `template_has_no_theme_line`: `!SETTINGS_TEMPLATE.contains("theme =")` and no
    `Color theme` line.
  - `write_field_never_writes_theme`: after `write_field` of any change, the file has no
    `theme` key.
- [ ] **Step 2: Run** `cargo test -p fbterm-config` and expect FAIL.
- [ ] **Step 3: Remove** the field, `ThemeMode`, `SettingChange::Theme`, the template lines,
  the Settings row and `SettingsMsg::Theme`, and add the notice. Temporary uses of
  `settings.theme` in `app.rs`/`glue.rs` become `theme::tokens_for("graphite", …)`, so the
  build stays green until Task 3.
- [ ] **Step 4: Run** `cargo test --workspace` and expect PASS.
- [ ] **Step 5: Commit:** `feat(settings)!: remove the theme setting; the window follows the color scheme`.

### Task 3: The app follows the global scheme

**Files:**
- Modify: `crates/fbterm/src/app.rs` (the `tokens`/`theme` fields stay
  `&'static Tokens`/`Theme`; their initialisation), `crates/fbterm/src/settings_window/glue.rs`
  (`apply_settings`), `crates/fbterm/src/schemes.rs` (`reload_schemes`),
  `crates/fbterm/src/schemes_window/glue.rs` (`save_scheme`),
  `crates/fbterm/src/tab_look.rs` (`ink`), and every test that builds tokens from
  `ThemeMode`.
- Test: `crates/fbterm/src/theme_app_tests.rs` (new, `#[cfg(test)] mod` in `main.rs`).

**Interfaces:**
- Consumes (Task 1): `theme::tokens_for`, `theme::chrome_theme`.
- Produces:
  - `App::chrome_scheme(&self) -> &Scheme`: the scheme named by `settings.color_scheme`,
    else built-in `graphite`. It ignores profile overrides.
  - `App::retheme(&mut self)`: sets `self.tokens = tokens_for(&s.id, &s.colors)` and
    `self.theme = chrome_theme(self.tokens)`. It is called from `App::new`, from
    `apply_settings` (both sources) and from `reload_schemes`. It is a no-op when the
    pointer is unchanged.
  - `tab_look::ink(t: &Tokens, bg: Color) -> &Tokens`: returns `t` when
    `contrast(t.text, bg) >= 4.5`, else whichever of `&DARK`/`&LIGHT` has text contrasting
    more with `bg`.

- [ ] **Step 1: Write failing tests** in `theme_app_tests.rs`, using the existing
  `schemes_tests::app_with` helpers:
  - `global_scheme_colors_the_chrome`: with `color_scheme` set to a dark user scheme,
    `app.tokens.app != DARK.app` and `*app.tokens == *tokens_for(id, colors)`.
  - `profile_override_leaves_the_chrome`: a profile with an override open and focused leaves
    the tokens equal to the global scheme's.
  - `missing_global_scheme_gives_graphite_chrome`: `color_scheme = "user-gone"` gives
    `app.tokens` pointer-equal to `tokens_for("graphite", …)`.
  - `editing_the_global_scheme_recolors_on_save_only`: in the schemes state, change the
    global scheme's background in the draft and the tokens are unchanged. Send `Save`, and
    the tokens change.
  - `file_change_of_color_scheme_retheme`: write `color_scheme` to the file and deliver the
    watcher's `Changed { settings: true, .. }`, and the tokens follow.
  - `ink_keeps_the_current_set_when_readable`: in `tab_look`, `ink(&t, t.terminal)` is `t`
    for a derived dark set, and a light background with a dark set gives `&LIGHT`.
- [ ] **Step 2: Run** `cargo test -p fbterm --bin fbterm theme_app` and expect FAIL.
- [ ] **Step 3: Implement** `chrome_scheme`, `retheme` and the calls, and change `ink`.
  Replace every remaining `theme::tokens(ThemeMode::…)` or `theme::DARK`/`LIGHT` use outside
  `theme.rs` with `app.tokens` or `tokens_for("graphite", …)` (tests). `tab_look` tests keep
  using the constants directly.
- [ ] **Step 4: Run** `cargo test --workspace` and expect PASS.
- [ ] **Step 5: Commit:** `feat: the app's colors follow the global color scheme`.

### Task 4: Color schemes as a page tab

**Files:**
- Modify: `crates/fbterm/src/tabs.rs` (`Page::Schemes`, title `Color schemes`),
  `crates/fbterm/src/windows.rs` (`page_closed`, the close-request branch, the non-terminal
  window list), `crates/fbterm/src/window.rs` (`page_view`, `page_min_width`
  → `760.0`, the page icon (reuse the icon the palette shows for `Action::EditSchemes`, or
  `ICON_SETTINGS`'s family), `key_mode`, and removal of the schemes branch of `window_view`),
  `crates/fbterm/src/schemes_window/glue.rs` (`open_schemes`, `Close`),
  `crates/fbterm/src/app.rs` (`schemes_window` users at the old window paths, the screenshot
  `ShotWindow::Schemes`), `crates/fbterm/src/debug.rs`, `crates/fbterm/src/dialogs.rs`
  (editor button), `crates/fbterm/src/shortcuts.rs` (mode docs and test).
- Move: the window-specific assertions in `schemes_window/tests.rs`, `gallery_tests.rs` and
  `shortcuts.rs` to the tab (same behavior, new open path).

**Interfaces:**
- `App::schemes_win: Option<(window::Id, SchemesState)>` keeps its shape. The id is now the
  hosting terminal window, as with `settings_win`, and `App::schemes_window()` returns it.
- `App::open_schemes(select: Option<String>) -> Task<Message>`:
  - calls `show_page(Page::Schemes)`;
  - creates the state as today if it is absent (installed fonts, gallery state carried
    over);
  - otherwise sends `SchemesMsg::Select(select)` (the named scheme, else the global one);
  - does not scan by itself (Task 5 scans on show).
- `SchemesMsg::Close` → `close_page(Page::Schemes)`; `page_closed(Page::Schemes)` sets
  `schemes_win = None`.
- `key_mode(Some(Page::Schemes), …)` gives `Mode::SchemesConfirm`/`SchemesImport`/`SchemesUpdate`/`Schemes`
  with today's priority. Mode docs say "tab" instead of "window".
- `EditorMsg::EditSchemes` → `open_schemes(Some(<the edited profile's scheme or global>))`.
- Restore: `Page::Schemes` is skipped like the other pages. Grouping: `Group::Featherbit`.
  Check `restore.rs` and `tab_groups.rs` and add the variant wherever pages are matched.

- [ ] **Step 1: Write failing tests** in `schemes_window/tests.rs`:
  - `color_schemes_opens_as_a_page_tab`: `SchemesMsg::Open` adds a `Page::Schemes` tab to
    the focused window and selects it. A second `Open` adds none and selects it.
  - `routes_open_the_tab_with_the_named_scheme`: the palette `Action::EditSchemes`,
    `SettingsMsg::EditSchemes` and `EditorMsg::EditSchemes` each open or select the tab, and
    the state's selection is the named scheme. This includes when the tab is already open
    on another scheme.
  - `closing_the_tab_drops_drafts_and_state`: edit a draft, send `Close`; the tab is gone,
    `schemes_win` is `None`, and nothing is written.
  - `closing_the_host_window_clears_the_schemes_tab`: close the hosting window (as a close
    request), then `schemes_win` is `None`. `Open` afterwards makes a fresh tab in the
    remaining focused window.
  - `schemes_tab_is_not_restored`: saved state with the tab open restores without it.
  - In `shortcuts.rs`, convert `the_schemes_window_closes_saves_and_reaches_the_others` to
    the tab: Cmd+W closes the tab, Cmd+S saves.
- [ ] **Step 2: Run** `cargo test -p fbterm --bin fbterm schemes` and expect FAIL.
- [ ] **Step 3: Implement** the variant and routing, and remove the window. Debug aids:
  - `FBTERM_OPEN_SCHEMES` opens the tab in the first window;
  - `FBTERM_SCREENSHOT_WINDOW=schemes` captures the window whose active tab is
    `Page::Schemes`, as `settings` does.

  Update the `CLAUDE.md` debug-aid text in Task 6, not here.
- [ ] **Step 4: Run** `cargo test --workspace` and expect PASS. Then take one screenshot,
  `FBTERM_OPEN_SCHEMES=1 FBTERM_SCREENSHOT_WINDOW=schemes FBTERM_SCREENSHOT=target/shots/schemes-tab.png`
  with a throwaway config, and Read it.
- [ ] **Step 5: Commit:** `feat(schemes): Color schemes is a page tab`.

### Task 5: Notice new fonts when the tab is shown

**Files:**
- Modify: `crates/fbterm/src/windows.rs` (`App::shown`), `crates/fbterm/src/app.rs`
  (`Message::WindowFocused` handler), `crates/fbterm/src/theme_fonts.rs` (module docs).
- Test: `crates/fbterm/src/theme_fonts_tests.rs`.

**Interfaces:**
- Produces: `App::schemes_shown(&mut self) -> Task<Message>`. It calls
  `refresh_installed_fonts()` then `scan_fonts()`. It is called from `shown(wid)` when
  `wid`'s active tab is `Page::Schemes`, and from `WindowFocused(id)` when `id`'s active tab
  is `Page::Schemes`.

- [ ] **Step 1: Write failing tests** in `theme_fonts_tests.rs`. Change the existing tests
  that open the window to the tab where needed.
  - `selecting_the_schemes_tab_scans`: with a suggested family missing, open the tab, select
    another tab, install the font in the fake, select the tab again. One more scan runs, and
    the line becomes installed after `finish_scan`.
  - `focusing_the_window_with_the_tab_active_scans`: `Message::WindowFocused(wid)` with the
    tab active starts a scan. With another tab active, it starts none.
  - `nothing_missing_means_no_scan_on_show`: nothing missing, so selecting or focusing
    starts no scan.
- [ ] **Step 2: Run** `cargo test -p fbterm --bin fbterm theme_fonts` and expect FAIL.
- [ ] **Step 3: Implement** `schemes_shown` and the two calls.
- [ ] **Step 4: Run** `cargo test --workspace` and expect PASS.
- [ ] **Step 5: Commit:** `feat(schemes): look for new fonts when the tab is shown or focused`.

### Task 6: Docs, CLAUDE.md and screenshots

**Files:**
- Modify: `website/docs/color-schemes.md`, `website/docs/files.md`,
  `CLAUDE.md`, `docs/superpowers/specs/2026-10-27-color-schemes-design.md` (add
  "Superseded by `2026-11-09-schemes-tab-and-themed-chrome-design.md` (part B)" under its
  chrome bullet).

- [ ] **Step 1: Docs.**
  - `color-schemes.md`: the global scheme colors the whole app, chrome included; a
    profile's scheme colors only its sessions; Color schemes is a tab.
  - `files.md`: remove the `theme` row; add to `color_scheme`'s row that it colors the
    whole app.
  - `CLAUDE.md`:
    - the page tabs list gains Color schemes, and the Color schemes window is no longer
      listed as a separate window;
    - the debug aids `FBTERM_OPEN_SCHEMES` and `FBTERM_SCREENSHOT_WINDOW=schemes` now
      refer to the tab;
    - the color scheme resolution gotcha gains the chrome rule (`App::retheme`,
      `chrome_scheme`, interned `tokens_for`);
    - the look-and-feel rule follows the spec's Docs section;
    - the status line.
- [ ] **Step 2: Screenshots** (throwaway config, Read each):
  - the chrome with the Settings tab open under `graphite`, `graphite-light`, and two gallery
    themes installed through `FBTERM_DEMO_GALLERY`'s sample or written as scheme files (one
    dark, one light);
  - save them to `target/shots/chrome-<id>.png`;
  - check by eye that text is readable and that Graphite looks as before.
- [ ] **Step 3: Run the full gate** (fmt, clippy, test) and `cd website && npm run build` if
  `node_modules` exists. Otherwise say it was skipped.
- [ ] **Step 4: Commit:** `docs: the app follows the global color scheme; Color schemes tab`.
