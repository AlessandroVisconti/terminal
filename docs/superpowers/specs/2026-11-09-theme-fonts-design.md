# Suggested fonts for themes: design

Status: draft for the owner's review, 2026-11-09. Written while the owner was away, from their
request; the calls marked **(call)** are the author's and are open to change. Builds on
`2026-10-27-color-schemes-design.md` and `2026-10-28-theme-gallery-design.md`, which stay binding
for everything this document does not change.

## 1. Intent

Each theme suggests a font family that suits it. fbterm never ships or downloads fonts: it shows
the suggestion with a link to the font's official download page; the user downloads and installs
the font themselves and then picks it in fbterm with one click.

## 2. Goals

1. Scheme files gain an optional `[suggested_font]` table (separate from the scheme's existing
   `font_family`/`font_size`, which apply the font; a suggestion applies nothing by itself):
   ```toml
   [suggested_font]
   family = "JetBrains Mono"
   url = "https://www.jetbrains.com/lp/mono/"
   license = "OFL-1.1"
   ```
   All three keys are required when the table is present; `url` must be `https://`.
2. The four built-in schemes carry a suggestion too (in code), e.g. the default scheme suggests
   Geist Mono, fbterm's default font **(call: each built-in's pick is the builder's, from the
   rules in section 4)**.
3. Every theme in `terminal-themes` gets a `[suggested_font]` table; the catalog carries it; the README's
   theme table gets a font column.
4. In the Color schemes window (installed schemes and gallery entries), a scheme with a
   suggestion shows a line: `Suggested font: <family>` and
   - `Download`: opens `url` in the default browser (a user click; never automatic);
   - `Use font`: sets the global `font_family` setting, enabled only when the family is installed
     **(call: global, not the scheme's own `font_family`, because writing an installed gallery
     theme's font would mark it edited and block its updates; a scheme that sets its own font
     still wins for its panes, as today, and the line then says so)**;
   - a state word: `In use` when it is already the font, `Not installed` when missing.
5. Installed-or-not is checked against the same system font list the Settings font picker uses,
   each time the scheme's details are shown, so a font installed while fbterm runs is found
   without a restart **(call)**.

## 3. Non-goals and anti-goals

- No bundled fonts, no downloading or installing fonts from fbterm, no network access beyond the
  browser the user opens.
- Picking a scheme never changes the font by itself; only `Use font` does **(call)**.
- The suggestion is not part of `install_hash` (adding or changing it never marks a theme edited);
  `terminal-themes` bumps a theme's version when it adds one so Update delivers it.
- No per-profile fonts (fonts stay a global setting, as today).
- Anti-goals: a suggested font that is not free to use (only OFL-1.1, Apache-2.0, MIT, or an
  equally permissive license); a link to anything but the font's official page (the foundry's or
  project's own site or its GitHub releases; no mirrors, no aggregator that rehosts files)
  **(call: Google Fonts' own page counts as official for fonts published there by their authors)**;
  a malformed `[suggested_font]` table breaking a scheme (it is dropped with a notice; the colors still
  load); naming or comparing to other terminal applications anywhere, including font notes; a
  remote program causing a browser to open.

## 4. Choosing each theme's font

- If the theme's upstream project recommends a font, use it (when it meets the license rule).
- Otherwise pick by character: a font whose feel matches the palette (e.g. a warm humanist mono
  for warm retro palettes, a crisp geometric mono for cool modern ones), with good box-drawing and
  powerline coverage, preferring well-maintained families. Spread the picks: no single family
  for more than about a third of the themes.
- Verify every `url` answers HTTP 200 (or a redirect to the official page) at the time of the
  change; record the license from the font's own repository or site.

## 5. Format and compatibility

- Older fbterm versions must keep installing themes whose files gain `[suggested_font]`: check whether the
  scheme and catalog parsers reject unknown tables. If they do, follow the catalog's existing
  format-version rule (bump the version and keep older clients served) rather than breaking them.
- An Update that only adds `[suggested_font]` counts as an update (version bump in `terminal-themes`), so
  installed themes get the suggestion; the edited-theme protection still applies.

## 6. Copy

`Suggested font`, `Download`, `Use font`, `In use`, `Not installed`. Docs: a short section in
`website/docs/color-schemes.md` (the `[suggested_font]` table, and that fbterm never downloads fonts), and
the format in `terminal-themes` README.

## 7. Tests

- Config: `[suggested_font]` parses; missing key, non-https url, or wrong types drop the table with a notice
  and keep the scheme; built-ins have suggestions; a round trip through install keeps `[suggested_font]`.
- Catalog: entries carry the font; older-format compatibility per section 5.
- UI: the line shows for schemes with a suggestion and not for others; `Use font` disabled when not
  installed, saves `font_family` when enabled; `In use` state; `Download` produces the open-URL
  action with exactly the https url (assert the action, do not open a browser in tests).
- `terminal-themes`: catalog script and its tests accept and require valid `[suggested_font]` tables;
  CI check that every theme has one; the README table is regenerated.
- Screenshots of the scheme details with a suggestion (throwaway config).

## 8. Left to the builder

Layout of the line within the existing details and gallery cards; the helper used to open URLs
(reuse whatever the gallery's project link uses); the font picks themselves within section 4.
