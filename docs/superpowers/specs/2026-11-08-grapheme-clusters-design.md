# Grapheme clusters (mode 2027)

Date: 2026-11-08. Status: approved by the owner, 2026-11-08.

## Intent

Some characters are built from several code points: flags (two regional indicators), skin
tones (base plus modifier), joined emoji (`👨‍👩‍👧`, pieces joined by ZWJ), and emoji or text
presentation chosen by a variation selector. Today the emulator handles one code point at a
time: zero-width pieces attach to the previous cell, but every visible piece takes cells of
its own, so a flag or a joined emoji spreads over several cells and draws broken.

Nobody has hit this in fbterm yet (owner, 2026-11-08). The work is future-proofing: programs
that announce they count characters by grapheme cluster, through DEC private mode 2027,
should get one cell per cluster with the right width, and programs that don't must see no
change at all.

## Goals

- DEC private mode 2027 (`CSI ? 2027 h` / `l`) turns grapheme clustering on and off.
- `DECRQM` for 2027 (`CSI ? 2027 $ p`) reports the mode's current state (set or reset), so
  programs can detect support.
- While the mode is set, each grapheme cluster lives in one cell (1 or 2 columns wide), the
  cursor advances by the cluster's width, and selection, copy and search treat it as one
  character.
- A Settings switch, off by default, makes mode 2027 set by default in every terminal.

## Non-goals

- Ligatures and font features (step 4 of the roadmap). That work must later treat a cluster
  cell as one unit; nothing in this design blocks it.
- Changing anything while mode 2027 is reset. Output, widths and cursor movement stay
  byte-for-byte what they are today.
- Grapheme-aware cursor keys, backspace or line editing. Those belong to the program.
- Prepend-class code points (Unicode `Prepend`) widening or narrowing a cluster: a prepend
  base keeps its own width.

## Anti-goals

- A prompt, editor or table drawn by a program that did not ask for mode 2027 ends up with
  its cursor in a different column than today. That is the failure this design exists to
  avoid, and the reason the mode is opt-in (owner decision).
- Plain ASCII or Latin-1 output gets measurably slower. The clustering path must be skipped
  cheaply for code points up to U+00FF and at column 0.

## Owner decisions

- **Opt-in.** Mode 2027 is reset by default; programs set it. (2026-11-08)
- **Always-on switch.** A Settings switch, off by default, sets the mode by default for
  terminals. (2026-11-08)
- **No credit to other projects** in code, commits, docs or the fork's README. The rule that
  no other terminal application is named anywhere committed to the terminal repo applies to
  feathercritty too for this work. (2026-11-08)
- **Order.** This comes before ligatures. (2026-11-08)
- **Done means correct cells.** Cluster placement, width and cursor are tested on all three
  systems in CI; how a cluster looks is checked by hand on macOS only. A system that draws a
  correctly placed cluster wrongly (font fallback, shaping) is a separate follow-up, not part
  of this work. (2026-11-08)

## Behaviour while mode 2027 is set

The previous cell is the cell a new code point may join: the cell left of the cursor, or the
cell under the cursor when a wrap is pending (`input_needs_wrap`). If that cell is a
`WIDE_CHAR_SPACER`, the cell before it holds the character. If the previous cell is empty
(`' '` with no zero-width pieces and default flags, or the cursor is at column 0 with no
pending wrap), the new code point starts a new cell.

For a code point `c`, decide whether a grapheme boundary falls between the previous cell's
cluster (its base plus all its zero-width pieces, in order) and `c`, using the Unicode
extended grapheme cluster rules (UAX #29), including the regional-indicator pairing and the
emoji ZWJ sequence rule. The state is rebuilt from the cell every time, so it works across
read chunks and after the mode is turned on with pieces already stored by the old rules.

- **Boundary:** `c` is printed as today (new cell or cells by its own width), except that a
  zero-width `c` with no cluster to join is dropped instead of attached.
- **No boundary:** `c` joins the previous cell (stored like today's zero-width pieces) and
  may change its width:
  - **VS16 (U+FE0F)** after a base that has an emoji variation sequence makes a narrow
    cluster wide. **VS15 (U+FE0E)** after such a base makes a wide cluster narrow. After any
    other base, a variation selector is stored but changes nothing.
  - Any other joining code point that is not zero-width on its own (a second regional
    indicator, an emoji modifier, an emoji after a ZWJ) makes the cluster wide.
  - Zero-width joining code points (ZWJ, combining marks, other selectors) change nothing.

Width changes move cells:

- **Narrow to wide**, with room: the cell becomes `WIDE_CHAR`, the next cell becomes its
  spacer, and the cursor moves past the spacer (or sets a pending wrap at the right edge).
- **Narrow to wide in the last column**, with line wrap on: the last column becomes a
  `LEADING_WIDE_CHAR_SPACER` like today's wide characters that don't fit, the line wraps,
  and the whole cluster (base and every piece) is rewritten as a wide cell at the start of
  the next line. With line wrap off, the cluster stays narrow and `c` is still stored.
- **Wide to narrow:** the spacer is cleared and the cursor moves back to the column after
  the now-narrow cell, clearing any pending wrap.

A cell keeps at most 32 joined pieces; further joining code points are dropped. *(Builder:
the exact cap is yours; it exists so hostile output cannot grow one cell without bound.)*

In insert mode, a narrow cluster that turns wide shifts the rest of the line right by one
cell (the last cell falls off), the same as inserting a wide character would; a wide
character that a shift cuts in half at the right edge is cleared, never left as an orphan
spacer. *(My call.)* Scroll regions, origin mode and the alternate screen behave as they do
for any other write. The fast path (code point at or below U+00FF, or column 0 without a pending
wrap) never looks at the previous cell.

## Behaviour while mode 2027 is reset

Exactly today's `input`. The new code must not run.

## Mode lifetime

- The mode's default comes from the emulator's options (new `Config` field, see below).
- `RIS` (full reset, `reset_state`) puts the mode back to that default. There is no soft
  reset to handle: neither the emulator nor its parser implements `DECSTR`, and adding it is
  out of scope.
- A new session continuing into an existing terminal (reconnect, `spawn_local_into`,
  `open_shell_into`) also puts the mode back to the default, so a fresh shell never inherits
  clustering from a program that set it and then died. *(My call, from the anti-goal.)*
- Changing the option on a live terminal changes only what a later reset restores. It never
  flips the mode under a running program. *(My call: flipping it live could move a running
  program's cursor, which is the anti-goal.)*
- The mode is per terminal, shared by the normal and alternate screens.

## Where the work goes

### feathercritty (its own repo, next `0.26.0+fbterm.N` release)

- `TermMode::GRAPHEME_CLUSTER`, set and reset through `PrivateMode::Unknown(2027)` (vte 0.15
  has no named variant), reported by `report_private_mode`.
- `Config::grapheme_clusters: bool` (default `false`): the mode's default at creation and at
  reset.
- The clustering branch in `Term::input`, as above. Segmentation from a maintained crate
  (`unicode-segmentation` is the expected choice; the builder may pick another with
  MIT/Apache licensing that passes `cargo deny` in both repos), width data from
  `unicode-width` 0.2 and the Unicode emoji variation sequences.
- Every change carries the fork's merge-ready comment against upstream, as the README asks.
- Tests for each case in "Behaviour while mode 2027 is set", the reset path, DECRQM, and a
  check that output with the mode reset is unchanged (the existing differential tests
  against 0.26.0 must stay green).

### fbterm

- Bump the feathercritty rev.
- `grapheme_clusters` in `settings.toml` (default `false`) and a Settings row next to the
  kitty keyboard switch, with one line saying programs can turn it on themselves and that
  it applies to new terminals and to terminals after a reset.
- `fbterm-term::Terminal` passes the setting into `Config` at creation and through
  `set_options` when it changes, like `kitty_keyboard`.
- Check, with tests where the code allows: the renderer draws a cluster cell as one shaped
  run at its cell width (it already joins `WIDE_CHAR` and zero-width cells into one run);
  copy, export and search include the joined pieces (they already read `zerowidth()`);
  selection covers the whole cluster.
- Docs: the settings reference gains the key; the changelog entry comes with the release.

## What's left to the builder

- The piece cap, crate choice and where the width rules live.
- Test layout in both repos.
- Settings row wording, following the existing rows' style.

## Done when

- feathercritty: all new tests, the existing suite and the differential tests pass; a release
  is tagged.
- fbterm: the full gate passes with the bumped rev on all three systems, including tests
  that a terminal with mode 2027 set holds `🇮🇹`, `👍🏽`, `👨‍👩‍👧` and `❤️` each in one 2-wide
  cell with the cursor after it, and that the same output without the mode gives today's
  result.
- By hand on macOS: those four draw as one glyph each in their cells.
