# Grapheme Clusters Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Grapheme clusters held in one cell while DEC private mode 2027 is set, with an fbterm setting that makes the mode set by default.

**Architecture:** feathercritty gets the mode, its report, its default in `Config`, and a clustering branch in `Term::input` that rebuilds the boundary state from the previous cell on every code point. fbterm bumps the rev, adds the `grapheme_clusters` setting and Settings row, passes it into each terminal's `Config`, and restores the default when a session restarts in an existing terminal.

**Tech Stack:** Rust; feathercritty (fork of alacritty_terminal 0.26.0), vte 0.15, `unicode-width` 0.2, `unicode-segmentation` (new); fbterm with iced 0.14.

**Spec:** `docs/superpowers/specs/2026-11-08-grapheme-clusters-design.md` (terminal repo). Read it first; this plan does not repeat its behaviour rules.

## Global Constraints

- Mode 2027 reset by default; with it reset, `Term::input` behaves exactly as today (the clustering code must not run).
- Fast path: a code point at or below U+00FF, or column 0 with no pending wrap, never looks at the previous cell.
- No other terminal application or project is named anywhere: code, comments, commits, PR text, READMEs, docs.
- No `Co-Authored-By`, no `Claude-Session` line, no session links or "Generated with" footers in commits or PRs.
- feathercritty: every modified file keeps or gains the notice line at its top; every change carries a merge-ready comment against upstream (`// feathercritty: ...`); the README's "Changes from alacritty_terminal 0.26.0" gains an entry.
- feathercritty gate: `cargo +nightly fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`, `cargo test --no-default-features`, `DIFF_TINY=1 cargo test --release --test differential`.
- fbterm gate: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo deny --all-features check`.
- New dependencies must be MIT or Apache-2.0 and pass `cargo deny` in both repos.
- Product copy: no emoji, no em-dashes.
- Joined pieces per cell: at most 32 (`MAX_CLUSTER_PIECES`).

## Review Focus

1. A cluster split across two `feed` calls (the ZWJ at the end of one read, the next emoji at the start of the next) must join exactly as if fed at once. Test in Task 2.
2. A cluster cell scrolled into the compact history and read back keeps its base, every joined piece and its `WIDE_CHAR` flag. Test in Task 3.
3. Resizing narrower and wider (reflow) keeps a wide cluster whole: base, pieces and spacer travel together. Test in Task 3.
4. Printing over half of a wide cluster (its base or its spacer) clears the whole cluster, as for any wide character, and leaves no orphan spacer or orphan pieces. Test in Task 3.
5. The DECRQM reply for 2027 has the exact bytes `ESC [ ? 2027 ; 2 $ y` (reset) and `ESC [ ? 2027 ; 1 $ y` (set). Test in Task 1.

---

## Part 1: feathercritty

Work in `../feathercritty` on a new branch `feature/grapheme-clusters` off `origin/main` (`3c290b8`).

### Task 1: Mode 2027, its report and its default

**Files:**
- Modify: `src/term/mod.rs` (`TermMode`, `Config`, `Term::new`, `reset_state`, `set_private_mode`, `unset_private_mode`, `report_private_mode`, tests module)

**Interfaces:**
- Produces: `TermMode::GRAPHEME_CLUSTER` (`1 << 23`); `Config::grapheme_clusters: bool` (default `false`); `Term::reset_grapheme_cluster_mode(&mut self)` (sets or clears `GRAPHEME_CLUSTER` to `config.grapheme_clusters`).

- [ ] **Step 1: Write the failing tests** in the `tests` module of `src/term/mod.rs`:
  - `mode_2027_is_reset_by_default`: new term, `!mode().contains(GRAPHEME_CLUSTER)`.
  - `mode_2027_set_and_reset`: feed `\x1b[?2027h` → contains; `\x1b[?2027l` → not.
  - `mode_2027_reports_state`: with a listener capturing `Event::PtyWrite`, feed `\x1b[?2027$p` → reply `"\x1b[?2027;2$y"`; after `\x1b[?2027h`, the same query → `"\x1b[?2027;1$y"`.
  - `mode_2027_default_from_config`: `Config { grapheme_clusters: true, .. }` → set at creation; feed `\x1b[?2027l` then `\x1bc` (RIS) → set again.
  - `reset_grapheme_cluster_mode_restores_default`: config false, feed `\x1b[?2027h`, call `reset_grapheme_cluster_mode()` → not set.
  - `set_options_never_flips_mode_2027`: config false, feed `\x1b[?2027h`, `set_options` with `grapheme_clusters: false` → still set; and the reverse with true.
- [ ] **Step 2: Run** `cargo test --lib mode_2027 reset_grapheme_cluster set_options_never` → FAIL (no such field / variant).
- [ ] **Step 3: Implement.** Add the bit and field; in `set_private_mode`/`unset_private_mode` match `PrivateMode::Unknown(2027)` before the "ignoring unknown mode" fallback; in `report_private_mode` answer `Unknown(2027)` from the bit; apply the default in `Term::new` and at the end of `reset_state` (after `self.mode.insert(TermMode::default())`). `set_options` leaves the bit alone. Each change gets a `// feathercritty:` comment.
- [ ] **Step 4: Run** the same tests → PASS; `cargo test` → all pass.
- [ ] **Step 5: Commit** `feat(term): DEC private mode 2027 with its report and a Config default`.

### Task 2: Clustering in `Term::input`

**Files:**
- Create: `src/term/cluster.rs` (pure helpers; module declared in `src/term/mod.rs`)
- Modify: `src/term/mod.rs` (`input`), `Cargo.toml` (add `unicode-segmentation = "1"`)
- Test: tests in `src/term/cluster.rs` and the `tests` module of `src/term/mod.rs`

**Interfaces:**
- Produces, in `src/term/cluster.rs`:
  - `pub(crate) const MAX_CLUSTER_PIECES: usize = 32;`
  - `pub(crate) fn joins(base: char, pieces: &[char], c: char) -> bool` — true when no extended grapheme cluster boundary falls before `c` in `base + pieces + c`.
  - `pub(crate) enum WidthEffect { NoChange, Wide, Narrow }`
  - `pub(crate) fn width_effect(base: char, c: char) -> WidthEffect` — VS16 after an emoji variation base → `Wide`; VS15 after such a base → `Narrow`; a variation selector after any other base → `NoChange`; any other `c` with `c.width() > Some(0)` → `Wide`; otherwise `NoChange`.
  - `fn is_emoji_vs_base(c: char) -> bool` over a sorted `&[u32]` table generated from Unicode 16.0.0 `emoji-variation-sequences.txt` (the Unicode version `unicode-width` 0.2 uses), with a comment giving the source file and version.
- Consumes: `TermMode::GRAPHEME_CLUSTER` (Task 1).

`joins` builds the string and asks `unicode_segmentation::GraphemeCursor::new(offset_of_c, len, true).is_boundary(&s, 0)`; the whole string is one chunk, so an `Err` cannot happen and is treated as a boundary.

- [ ] **Step 1: Write the failing helper tests** in `cluster.rs`:
  - `joins`: `('🇮', [], '🇹')` true; `('🇮', ['🇹'], '🇬')` false (a third indicator starts a new flag); `('👍', [], '\u{1F3FD}')` true; `('👨', [], '\u{200D}')` true; `('👨', ['\u{200D}'], '👩')` true; `('a', [], 'b')` false; `('e', [], '\u{301}')` true.
  - `width_effect`: `('❤', '\u{FE0F}')` Wide; `('😀', '\u{FE0E}')` Narrow; `('a', '\u{FE0F}')` NoChange; `('🇮', '🇹')` Wide; `('👨', '\u{200D}')` NoChange; `('e', '\u{301}')` NoChange.
- [ ] **Step 2: Run** `cargo test --lib cluster` → FAIL.
- [ ] **Step 3: Implement** the helpers and the table.
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Write the failing `input` tests** (mode set unless noted; read cells with `grid.screen()`):
  - `cluster_flag_one_wide_cell`: `🇮🇹` → cell 0 `c == '🇮'`, `zerowidth() == ['🇹']`, `WIDE_CHAR`; cell 1 spacer; cursor column 2.
  - `cluster_skin_tone`, `cluster_family_zwj`: same shape for `👍🏽` and `👨‍👩‍👧` (pieces in order).
  - `cluster_vs16_widens`: `❤\u{FE0F}x` → `❤` wide with piece `FE0F`, `x` at column 2.
  - `cluster_vs15_narrows`: `😀\u{FE0E}x` → `😀` narrow, no spacer, `x` at column 1.
  - `cluster_vs_on_plain_base_ignored`: `a\u{FE0F}b` → `a` narrow, `b` at column 1.
  - `cluster_split_across_feeds` (Review Focus 1): feed `👨\u{200D}` then `👩` in two calls → one cell, same as one feed.
  - `cluster_orphan_zero_width_dropped`: at column 0, feed `\u{200D}` → nothing stored, cursor column 0.
  - `cluster_piece_cap`: `👨` followed by 100 × `\u{200D}` → `zerowidth().len() == MAX_CLUSTER_PIECES`.
  - `cluster_mode_reset_unchanged`: mode reset, `🇮🇹` → today's layout: `🇮` at column 0, `🇹` at column 1, cursor column 2.
  - `cluster_mode_enabled_midway`: mode reset, feed `e\u{301}`, set mode, feed `\u{302}` → joins `e` (pieces `301`, `302`).
- [ ] **Step 6: Run** `cargo test --lib cluster_` → FAIL.
- [ ] **Step 7: Implement** the branch at the top of `input`: only when the mode is set, `c > '\u{FF}'`, and not (column 0 without pending wrap). Locate the previous cell per the spec (pending wrap → cursor cell; else column − 1; spacer → one more left). Empty previous cell → fall through. `joins` false → fall through, except a width-0 `c` is dropped. `joins` true → push the piece (unless at the cap) and apply `width_effect` for the in-row cases (narrow→wide with room, wide→narrow); until Task 3, the edge cases (last column, insert mode) store the piece without changing the width.
- [ ] **Step 8: Run** `cargo test` → all pass; `DIFF_TINY=1 cargo test --release --test differential` → pass.
- [ ] **Step 9: Commit** `feat(term): keep grapheme clusters in one cell while mode 2027 is set`.

### Task 3: Width changes at the edges, insert mode, history and reflow

**Files:**
- Modify: `src/term/mod.rs` (`input` clustering branch), tests module

**Interfaces:**
- Consumes: `joins`, `width_effect`, `WidthEffect`, `MAX_CLUSTER_PIECES` (Task 2).

- [ ] **Step 1: Write the failing tests** (5-column terms unless noted):
  - `cluster_widens_in_last_column_wraps`: `abcd❤\u{FE0F}` → row 0 column 4 has `LEADING_WIDE_CHAR_SPACER`, row 0 wraps; row 1 columns 0–1 hold `❤` (piece `FE0F`, `WIDE_CHAR`) and spacer; cursor row 1 column 2.
  - `cluster_widens_in_last_column_no_wrap`: same with `\x1b[?7l` first → `❤` stays narrow in column 4 with piece `FE0F`; nothing on row 1.
  - `cluster_joins_under_pending_wrap`: `abc👍` puts `👍` wide at columns 3–4 with a pending wrap; then `\u{1F3FD}` joins it: no new line, row 1 empty, wrap still pending.
  - `cluster_narrows_clears_spacer`: `😀\u{FE0E}` at column 3 of 5 with pending wrap set beforehand by `abc😀` → `😀` narrow at 3, column 4 clear, pending wrap cleared, cursor column 4.
  - `cluster_widens_in_insert_mode_shifts`: 5 columns, row `xyz`, cursor to column 0, `\x1b[4h`, feed `❤\u{FE0F}` → row is `❤` (wide), spacer, `x`, `y`, `z`.
  - `cluster_widens_in_insert_mode_cuts_wide_at_edge`: 5 columns, row `xy中` (`中` wide at columns 2–3), cursor to column 0, `\x1b[4h`, feed `❤\u{FE0F}` → `❤` wide at 0–1, `x` at 2, `y` at 3, column 4 cleared (the shifted `中` would be cut in half), no spacer left behind.
  - `cluster_overwrite_half_clears` (Review Focus 4): `🇮🇹` at 0–1, cursor to column 1, print `x` → column 0 empty with no pieces, column 1 `x`; same for printing at column 0.
  - `cluster_survives_history` (Review Focus 2): 3-line term with history, `🇮🇹\r\n` then enough lines to scroll it into history → `Grid::cell` at that history point returns `c == '🇮'`, pieces `['🇹']`, `WIDE_CHAR`.
  - `cluster_survives_reflow` (Review Focus 3): `ab🇮🇹` on 6 columns, resize to 3 then back to 6 → the cluster cell keeps base, pieces and spacer.
- [ ] **Step 2: Run** `cargo test --lib cluster_` → the new ones FAIL (history and reflow may already pass: keep them as guards).
- [ ] **Step 3: Implement** the last-column wrap (copy the base and pieces, write the leading spacer, `wrapline`, rewrite as wide), the no-wrap case, the narrowing cursor move, and the insert-mode shift, per the spec.
- [ ] **Step 4: Run** `cargo test`, `cargo test --no-default-features`, `DIFF_TINY=1 cargo test --release --test differential` → all pass.
- [ ] **Step 5: Commit** `feat(term): move and resize grapheme clusters at the line edges`.

### Task 4: Document, release and merge the fork change

**Files:**
- Modify: `README.md` (new change entry after "### 7. Compact history"), `Cargo.toml` (`version = "0.26.0+fbterm.6"`)

- [ ] **Step 1:** Add "### 8. Grapheme clusters (mode 2027)": what the mode does, that nothing changes while it is reset, `Config::grapheme_clusters`, `reset_grapheme_cluster_mode`, the 32-piece cap, the new dependency. No other project named.
- [ ] **Step 2:** Run the full feathercritty gate (Global Constraints) → all pass.
- [ ] **Step 3:** Commit `docs: grapheme clusters in the change list; 0.26.0+fbterm.6`, push the branch, open a PR to `main`, wait for CI green, merge. Record the merge commit SHA for Task 5.

## Part 2: fbterm

Work in `.worktrees/grapheme-clusters` (branch `feature/grapheme-clusters`, which already holds the spec and this plan).

### Task 5: Bump the fork; the setting, its row and its wiring

**Files:**
- Modify: `Cargo.toml` (feathercritty `rev` = Task 4's merge SHA)
- Modify: `crates/fbterm-config/src/settings.rs` (`grapheme_clusters: bool`, default `false`, commented template line `#grapheme_clusters = false` after `#kitty_keyboard = false`)
- Modify: `crates/fbterm-config/src/settings_edit.rs` (`SettingChange::GraphemeClusters(bool)`, key `"grapheme_clusters"`, joined to the bool arm with `KittyKeyboard`)
- Modify: `crates/fbterm-term/src/terminal.rs` (`set_grapheme_clusters`, `grapheme_clusters`, `reset_grapheme_clusters`)
- Modify: `crates/fbterm/src/sessions.rs` (`new_terminal`; after `feed(tabs::RESTART_RESET)` call `reset_grapheme_clusters()`)
- Modify: `crates/fbterm/src/settings_window/glue.rs` (apply to every pane's terminal when the value changes, beside `kitty_keyboard`)
- Modify: `crates/fbterm/src/settings_window/mod.rs` (`SettingsMsg::Clusters(bool)` → `SettingChange::GraphemeClusters`), `view.rs` (row after "Kitty keyboard protocol")
- Modify: `website/docs/files.md` (sample line and table row)
- Test: `crates/fbterm-config/tests/config.rs`, `crates/fbterm-config/src/settings_edit.rs` tests, `crates/fbterm-term/src/terminal.rs` tests, `crates/fbterm/src/restore_tests.rs`, `crates/fbterm/src/settings_window/tests.rs`

**Interfaces:**
- Consumes: `Config::grapheme_clusters`, `Term::reset_grapheme_cluster_mode`, `TermMode::GRAPHEME_CLUSTER` (Task 1).
- Produces: `Terminal::set_grapheme_clusters(&mut self, on: bool)` (updates `config` through `set_options` like `set_kitty_keyboard`, dropping the re-announced title, never flipping a live mode); `Terminal::grapheme_clusters(&self) -> bool` (the setting); `Terminal::reset_grapheme_clusters(&mut self)` (calls `reset_grapheme_cluster_mode`); `Settings::grapheme_clusters`; `SettingChange::GraphemeClusters(bool)`.

Row copy (exact): label `Grapheme clusters`, hint `Keeps joined emoji and flags in one cell in new tabs. Programs can turn this on themselves.`

`files.md` row (exact): `` | `grapheme_clusters` | `true` or `false` | `false` | Count joined emoji, flags and skin tones as one character in one cell, as programs that support it ask for themselves. When `false`, only programs that ask get it. A change applies to new tabs and to tabs after a reset or a restart, never to a program already running. | ``

- [ ] **Step 1: Write the failing tests:**
  - config: absent key → `false`; `grapheme_clusters = true` → `true`; `write_field(GraphemeClusters(true))` turns the commented line into a live one (mirror the `kitty_keyboard` tests).
  - terminal.rs: `grapheme_setting_sets_default_not_live_mode`: `set_grapheme_clusters(true)` → `term().mode()` lacks `GRAPHEME_CLUSTER`; `reset_grapheme_clusters()` → has it.
  - restore_tests: `grapheme_setting_reaches_new_terminals`: with the setting on, `new_terminal` → mode set; with it off → not set.
  - restore_tests: `restart_restores_grapheme_default`: setting off, a pane's terminal fed `\x1b[?2027h`, restart the pane → mode not set.
  - settings_window tests: `SettingsMsg::Clusters(true)` queues `SettingChange::GraphemeClusters(true)`.
- [ ] **Step 2: Run** each new test → FAIL.
- [ ] **Step 3: Implement** the files listed above.
- [ ] **Step 4: Run** the new tests → PASS; full fbterm gate → pass.
- [ ] **Step 5: Commit** `feat: grapheme_clusters setting for mode 2027 by default` (with the rev bump; mention `0.26.0+fbterm.6`).

### Task 6: Clusters end to end in fbterm, and the macOS look

**Files:**
- Test: `crates/fbterm-term/src/terminal.rs` tests (or a new `crates/fbterm-term/tests/clusters.rs`), `crates/fbterm-term/src/search.rs` tests, `crates/fbterm-term/src/export.rs` tests

- [ ] **Step 1: Write the tests** (they should pass on the bumped rev; any failure is a bug to fix in the owning crate):
  - `clusters_with_mode_one_cell_each`: feed `\x1b[?2027h🇮🇹 👍🏽 👨‍👩‍👧 ❤\u{FE0F}` → each in one `WIDE_CHAR` cell with its pieces; cursor column 11.
  - `clusters_without_mode_unchanged`: the same text without the mode → today's layout (assert the cell count per character as the current code produces it).
  - search: finding `👨‍👩‍👧` in a terminal with the mode set returns one match spanning the cluster's two columns.
  - export / selection text: the exported or selected text of that line equals the input text exactly.
- [ ] **Step 2: Run** `cargo test -p fbterm-term` → all pass (fix in the owning code if not).
- [ ] **Step 3: macOS look (by hand, owner rule for agents):** run a debug build with `FBTERM_CONFIG_DIR` set to a scratch dir holding a profile named `demo` whose program prints `\e[?2027h🇮🇹 👍🏽 👨‍👩‍👧 ❤️` then sleeps, `FBTERM_SECRETS=memory`, `FBTERM_OPEN_TABS=demo`, `FBTERM_SCREENSHOT=<scratch>/clusters.png`; look at the screenshot: four single glyphs. Report what it shows; a wrong glyph is a follow-up, not a blocker (spec, "Done means correct cells").
- [ ] **Step 4:** Full fbterm gate → pass. Commit `test: grapheme clusters end to end`.
- [ ] **Step 5:** Push the branch, open the PR to `develop` (spec and plan included; body lists the fork release, the setting, tests, and the screenshot result; no attribution lines).
