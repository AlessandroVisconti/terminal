# Ligatures and font features

Date: 2026-11-08. Status: draft for owner review.

## Intent

Programming fonts draw sequences such as `->`, `=>`, `!=` and `>=` as single glyphs
(ligatures). The owner uses Geist Mono, fbterm's bundled default, and wants arrows like `->` and
`=>` to show as arrows. Ligatures are on by default for everyone, and not only for Geist Mono
(owner, 2026-11-08).

What makes this more than a switch:

- Geist Mono 1.802 (bundled) has 68 ligatures but keeps them in stylistic set `ss11`, which
  is off unless the program drawing the text asks for it. Other fonts (Fira Code, JetBrains
  Mono, Cascadia Code) keep theirs in the standard `liga`/`calt` features, which shaping
  applies by default. Some fonts may hide them in other stylistic sets.
- iced 0.14 draws text through cosmic-text but does not pass OpenType features through
  `iced::advanced::text::Text`. It does expose cosmic-text (`iced::advanced::graphics::text::
  cosmic_text`, its shared `font_system()`) and lets a renderer draw a buffer shaped
  elsewhere (`fill_raw` with a `graphics::text::Raw`). The terminal's text drawing has to
  move onto that path. This stays in the renderer: no toolkit change, no new GUI dependency.

## Goals

- Ligatures show by default in every font that has them, including fonts that keep them in a
  stylistic set.
- A `Ligatures` switch in Settings (on by default) turns every ligature off and on.
- A `font_features` setting applies any OpenType features on top, for any font.
- Ligatures never mislead about what is in the grid: they break where the cells differ in
  what the user needs to see.

## Non-goals

- Changing the grid, selection, copy, search or export. They keep seeing plain characters:
  copying a `->` ligature gives `-` and `>`.
- Per-profile or per-scheme feature lists (a scheme may set a font; features stay global).
- Font fallback or emoji rendering changes beyond what step 3 did.
- Ligatures in the UI chrome (tabs, Settings, palette). Only the terminal grid.

## Anti-goals

- A font that looks different apart from its ligatures: a stylistic set that changes letter
  shapes (an alternate `a`, a slashed zero) must never be turned on automatically.
- Slower redraws. Shaping is cached; a full-screen redraw of unchanged text must not shape
  again, and the cost of a full-screen update must stay within 10% of `develop`, measured in
  release.
- A ligature hiding where the cursor is, or spanning text of two colors.

## Owner decisions

- **Ligatures on by default, for every font.** (2026-11-08)
- **Copy and the grid stay plain characters.** Covered by "renderer only" in the roadmap
  (2026-11-08).
- No other terminal application is named anywhere committed (standing repo rule).

## Behaviour

### Which features are on

The features for a run are built in this order, later entries winning for the same tag:

1. With `Ligatures` on: `liga`, `calt` (and `clig`) on, as shaping does by default, plus every
   stylistic set the font's own data marks as ligature-only (below). With it off: `liga`,
   `calt`, `clig` and `dlig` off, and no stylistic set added.
2. Then each entry of `font_features`, in order.

**Ligature-only stylistic sets.** When a font is first used, fbterm reads its `GSUB` table and
turns on a stylistic set (`ss01` to `ss20`) only if every substitution it makes joins two or
more characters into one symbol, and none restyles a character on its own. Concretely, each
substitution must be one of:

- a ligature substitution entry with two or more components (an entry with one component, such
  as `(` becoming `parenleft.alt`, is a letter alternate and disqualifies the set);
- a contextual rule matching two or more glyphs whose nested substitutions turn every matched
  glyph but one into a blank spacer glyph (no outline) and the remaining one into the symbol
  covering all of them, which is how monospace fonts build ligatures.

A contextual rule that only restyles one character by its neighbours (a different backslash
before a letter, `x` drawn as a multiplication sign between digits) disqualifies the set: the
anti-goal wins over any structural test. *(My call.)* For Geist Mono 1.802 this must select
`ss11` (all chained rules of the spacer kind) and none of `ss01` to `ss10` (`ss10` is a ligature
lookup whose entries mostly have one component).
The result is cached per font face. Features apply to a whole run, so a character in it that
falls back to another font gets the detected sets too; this is accepted rather than splitting
runs wherever the main font lacks a glyph (a character-map lookup per cell, against the redraw
budget), and detected sets are left off the cells drawn on their own (wide, clusters,
standalone), where no ligature can form. *(My call: a fallback font's same-numbered set is
rarely a letter alternate for the box-drawing and symbol characters that fall back; cost if
wrong, an odd glyph in a fallback character.)* *(Builder: the exact parse is yours. A maintained crate for
reading OpenType tables (`ttf-parser`, `skrifa` or `read-fonts`) is expected, MIT or Apache-2.0,
passing `cargo deny`; prefer one already in the dependency tree.)*

### Where ligatures break

A run is shaped as one piece only across cells that share foreground, background, bold,
italic, underline, strikethrough and selection state. On top of that, a run always splits:

- at the cursor cell (whatever the cursor shape, while it is shown), so the cell under the
  cursor is drawn on its own;
- at the edges of the selection and of the find bar's current match;
- around every wide cell, cell with joined pieces (grapheme clusters from step 3) or other
  cell the renderer already draws on its own (`standalone`).

A ligature's glyph must stay within its run's cells; monospace ligature fonts draw them that
way. A run's glyphs are placed from the run's first cell, as now.

### Settings

- `ligatures` (`true` or `false`, default `true`) in `settings.toml`, and a `Ligatures` switch
  in Settings beside the font family and size, with the hint
  `Draws sequences such as -> and != as one symbol when the font has them.`
- `font_features` (a list of strings, default empty) in `settings.toml` only, no Settings row.
  Each entry is a four-letter OpenType tag, optionally prefixed with `+` (on) or `-` (off), or
  followed by `=<number>` (a value, for character variants): `"ss11"`, `"-calt"`, `"cv01=2"`.
  An entry that does not parse is skipped and reported once in the window banner, like other
  settings notices; the rest still apply.
- Both apply to every open terminal at once (they only change drawing).

## Where the work goes

- `crates/fbterm-term/src/widget/draw.rs`: runs split as above; text runs drawn through
  cosmic-text buffers shaped with the run's features (`Attrs::font_features`) and handed to the
  renderer with `fill_raw`, wide and standalone cells as well so every terminal glyph goes
  through one path. Buffers are cached across frames (keyed by text, font, size, weight,
  style and features) in the widget's state, and dropped when no longer drawn.
- A small module for the ligature-only stylistic set detection, with its per-face cache.
- `crates/fbterm-config`: `ligatures` and `font_features` in `Settings`, the template, the
  setting changes, parsing of `font_features` entries.
- `crates/fbterm`: the Settings row, applying both to open terminals, the banner notice for a
  bad entry.
- Docs: `website/docs/files.md` rows for both keys; a short section on ligatures where fonts are
  documented.

## What's left to the builder

- The exact GSUB parse and crate; cache sizes and eviction; how buffers are kept alive for
  `fill_raw`'s weak reference.
- Whether bold and italic faces get their own detection (they are separate faces).
- Test layout. The detection is tested on the bundled Geist Mono; run splitting is tested on
  `DrawList`s; feature lists are tested as data.
- The benchmark used for the 10% redraw budget.

## Done when

- Unit tests: Geist Mono selects `ss11` only; runs split at the cursor, selection edges, style
  changes and standalone cells; `font_features` parsing; the switch and the setting order.
- The full gate passes on macOS, Linux and Windows.
- The redraw budget holds, with the numbers in the PR.
- By hand on macOS (screenshot with a throwaway profile): with Geist Mono, `->`, `=>`, `!=` and
  `>=` show as ligatures; with the cursor on one of them it breaks; with `Ligatures` off they
  show as plain characters.
