# Ligatures Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ligatures on by default in the terminal grid for every font that has them (stylistic-set ligatures included), with a `Ligatures` switch and a `font_features` setting.

**Architecture:** fbterm-term shapes each text run itself with cosmic-text (iced's re-export) and the run's OpenType features, caches the shaped buffers across frames in the widget state, and draws them with `fill_raw`. Ligature-only stylistic sets are detected once per font face from its `GSUB` table with `ttf-parser`. Runs split where a ligature would mislead (cursor, selection and match edges, style changes). Settings carry `ligatures` and `font_features` into `TerminalStyle`.

**Tech Stack:** Rust; iced 0.14 (`iced::advanced::graphics::text::{cosmic_text, font_system, Raw}`), cosmic-text 0.15, `ttf-parser` 0.25 (already in the tree through fontdb).

**Spec:** `docs/superpowers/specs/2026-11-08-ligatures-design.md`. Read it first.

## Global Constraints

- `ligatures` default `true`; `font_features` default empty; both in `settings.toml`; only `Ligatures` has a Settings row, hint exactly `Draws sequences such as -> and != as one symbol when the font has them.`
- Feature order per run: switch features first (on: `liga`, `calt`, `clig` on + detected sets; off: `liga`, `calt`, `clig`, `dlig` off, no detected sets), then `font_features` entries in order, later winning.
- Detected sets never apply to cells drawn on their own (wide, clusters, standalone).
- Grid, copy, search, selection and export are untouched.
- A full-screen update costs at most 10% more than `develop`, measured in release; numbers go in the PR.
- Copy: no emoji, no em-dashes. No other terminal application named anywhere. No attribution lines in commits or PRs.
- Gate: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, `cargo deny --all-features check`.

## Review Focus

1. A font with no `GSUB` table, or a font file fontdb cannot read, must draw as before with no detected sets and no panic. Test in Task 2.
2. A font family name that matches no installed face (a scheme naming a missing font) falls back like today and detects nothing. Test in Task 2.
3. Changing the font (Settings or a scheme) or the switch redraws open panes with the new features at once, with no stale shaped buffer. Test in Task 5.
4. A run longer than the screen width (a wrapped line of `-` and `>` characters) never draws glyphs outside its cells. Test in Task 4 (a ligature at a run's last cell).
5. Bold and italic runs use their own face's detection (`GeistMono-Bold` is a separate face). Test in Task 2.

---

### Task 1: Settings keys and feature parsing

**Files:**
- Create: `crates/fbterm-config/src/font_features.rs` (module declared in `lib.rs`)
- Modify: `crates/fbterm-config/src/settings.rs`, `crates/fbterm-config/src/settings_edit.rs`
- Test: in `font_features.rs`, `crates/fbterm-config/tests/config.rs`, `settings_edit.rs` tests

**Interfaces:**
- Produces: `Settings::ligatures: bool` (default `true`), `Settings::font_features: Vec<String>` (default empty, serde default); `SettingChange::Ligatures(bool)` (key `"ligatures"`); template lines `#ligatures = true` and `#font_features = []` with a one-line comment each, after the font size line; `pub struct FeatureSetting { pub tag: [u8; 4], pub value: u32 }`; `pub fn parse_features(entries: &[String]) -> (Vec<FeatureSetting>, Vec<String>)` (parsed, rejected entries verbatim).

- [ ] **Step 1: Write the failing tests:** `parse_features` on `["ss11", "+calt", "-liga", "cv01=2", "toolong", "x", "ss1=", ""]` → parsed `[(ss11,1),(calt,1),(liga,0),(cv01,2)]`, rejected `["toolong", "x", "ss1=", ""]`; a tag must be exactly four ASCII alphanumerics. Config: absent keys → `ligatures == true`, `font_features.is_empty()`; `ligatures = false` and `font_features = ["ss11"]` parse; `write_field(Ligatures(false))` lands after `#ligatures = true`.
- [ ] **Step 2: Run** `cargo test -p fbterm-config` → FAIL (missing items).
- [ ] **Step 3: Implement** the module, fields, change and template lines.
- [ ] **Step 4: Run** `cargo test -p fbterm-config` → PASS.
- [ ] **Step 5: Commit** `feat(config): ligatures and font_features settings`.

### Task 2: Ligature-only stylistic sets

**Files:**
- Create: `crates/fbterm-term/src/ligature_sets.rs` (declared in `lib.rs`)
- Modify: `crates/fbterm-term/Cargo.toml` (`ttf-parser = "0.25"`)

**Interfaces:**
- Produces: `pub fn ligature_sets(data: &[u8], index: u32) -> Vec<[u8; 4]>` (sorted tags, empty when the font cannot be parsed or has no `GSUB`); `pub fn detected_sets(font: iced::Font) -> Arc<[[u8; 4]]>` resolving the face through `font_system().write().raw().db()` (`fontdb::Query` from the font's family, weight and style), cached per face id in a process-wide `Mutex<HashMap<fontdb::ID, Arc<[[u8; 4]]>>>`, empty when no face matches.

The rule (spec, "Ligature-only stylistic sets"), as geometry, since a contextual rule's own shape cannot tell a ligature from a restyle: collect every substitution reachable from the feature `ssNN` (directly, or through the nested lookups of context and chained-context rules). The set qualifies when it has at least one substitution and:
- every ligature-substitution entry has two or more components; and
- every single substitution's output glyph is either blank (no outline: `Face::outline_glyph` draws nothing, or no bounding box) or a multi-cell symbol: its bounding box reaches at least half an advance outside its own advance box (`x_min < -advance/2` or `x_max > 1.5 * advance`);
- any other lookup type (multiple, alternate) disqualifies the set.

- [ ] **Step 1: Write the failing tests** (fonts via `include_bytes!("../../fbterm/assets/fonts/GeistMono-Regular.ttf")` and `-Bold.ttf`):
  - `geist_mono_regular_selects_ss11_only`: `ligature_sets(GEIST, 0) == [*b"ss11"]`.
  - `geist_mono_bold_selects_ss11_only`: same for Bold (Review Focus 5).
  - `unparsable_font_selects_nothing` (Review Focus 1): `ligature_sets(b"not a font", 0)` and `ligature_sets(&GEIST[..64], 0)` (a truncated file) are empty and do not panic.
  - `detected_sets_resolves_by_family` (Review Focus 2): after loading Geist Mono bytes into `font_system()`, `detected_sets(Font::with_name("Geist Mono"))` contains `ss11`; `detected_sets(Font::with_name("No Such Font 123"))` is empty.
- [ ] **Step 2: Run** `cargo test -p fbterm-term ligature_sets` → FAIL.
- [ ] **Step 3: Implement** with `ttf_parser::Face` (`tables().gsub`, `opentype_layout` lookups and subtables).
- [ ] **Step 4: Run** → PASS.
- [ ] **Step 5: Commit** `feat(term): detect ligature-only stylistic sets per font face`.

### Task 3: Features for a run

**Files:**
- Modify: `crates/fbterm-term/src/widget/mod.rs` (`TerminalStyle`), new `crates/fbterm-term/src/widget/features.rs`
- Test: in `features.rs`

**Interfaces:**
- Consumes: `FeatureSetting` (Task 1), `detected_sets` (Task 2).
- Produces: `#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)] pub struct FontFeatures { pub ligatures: bool, pub extra: Arc<[FeatureSetting]> }` re-exported from `fbterm_term`; `TerminalStyle::features: FontFeatures`; `pub(crate) fn run_features(f: &FontFeatures, detected: &[[u8; 4]], alone: bool) -> Vec<(u32 /* tag as u32 */, u32)>` in spec order (the caller turns it into `cosmic_text::FontFeatures`). Note `Default` gives `ligatures: false`; callers build it from settings.

- [ ] **Step 1: Write the failing tests:** on, detected `[ss11]`, not alone → `liga=1, calt=1, clig=1, ss11=1`; on, alone → no `ss11`; off → `liga=0, calt=0, clig=0, dlig=0` and no `ss11`; extra `[-calt, ss02]` appended after and so winning for `calt`.
- [ ] **Step 2: Run** `cargo test -p fbterm-term features` → FAIL.
- [ ] **Step 3: Implement** the type, field (`TerminalStyle` construction sites in fbterm get `features` from settings in Task 5; until then a temporary `FontFeatures { ligatures: true, extra: [].into() }` keeps fbterm compiling) and function.
- [ ] **Step 4: Run** → PASS; `cargo build --workspace`.
- [ ] **Step 5: Commit** `feat(term): OpenType features for each text run`.

### Task 4: Runs that split where a ligature would mislead

**Files:**
- Modify: `crates/fbterm-term/src/widget/draw.rs` (`build`, `TextRun`)
- Test: `draw.rs` tests

**Interfaces:**
- Produces: `TextRun::alone: bool` (true for wide, cluster and standalone cells, which already get runs of their own). Text runs additionally split, in `build`, at: the cursor cell while the cursor is visible (any shape), selection edges, search mark edges, and changes of background, underline kind or strikeout (foreground, bold and italic already split).

- [ ] **Step 1: Write the failing tests:**
  - `runs_split_at_the_cursor`: feed `a->b`, cursor on the `>` cell (`\x1b[1;3H`) → runs `"a-"`, `">"`, `"b"` (the cursor cell alone).
  - `runs_split_at_selection_edges`: `a->b`, selection columns 2..=3 → `"a-"`, `">b"`.
  - `runs_split_at_match_edges`: highlight `>` → `"a-"`, `">"`, `"b"`.
  - `runs_split_on_background_and_underline`: `\x1b[41m-\x1b[0m>` → two runs; `\x1b[4m-\x1b[0m>` → two runs.
  - `alone_runs_are_marked`: `日` and `e\u{301}` runs have `alone == true`, `->` has `false`.
  - `ligature_at_the_last_cell_stays_in_its_run` (Review Focus 4): 4 columns, `ab->` → the last run is `"->"` spanning columns 2..=3.
- [ ] **Step 2: Run** `cargo test -p fbterm-term draw` → new tests FAIL.
- [ ] **Step 3: Implement** the splits and the field.
- [ ] **Step 4: Run** `cargo test -p fbterm-term` → PASS.
- [ ] **Step 5: Commit** `feat(term): split text runs at the cursor, selection, matches and style changes`.

### Task 5: Shaping with features, cached, drawn with `fill_raw`

**Files:**
- Create: `crates/fbterm-term/src/widget/shaped.rs`
- Modify: `crates/fbterm-term/src/widget/draw.rs` (`render` draws every text run through the cache), `crates/fbterm-term/src/widget/mod.rs` (`State` holds the cache; `draw::Metrics` gains `features: FontFeatures`)
- Test: in `shaped.rs`

**Interfaces:**
- Consumes: `run_features`, `detected_sets`, `TextRun::alone`, `run_font` (existing, emoji font from step 3 kept).
- Produces: `pub(crate) struct ShapeCache` with `pub fn buffer(&mut self, key: &ShapeKey, text: &str) -> Arc<cosmic_text::Buffer>`, `pub fn end_frame(&mut self)` (drops entries not used since the previous `end_frame`), `pub fn shaped_count(&self) -> usize` (for tests); `ShapeKey { text_hash: u64, font: Font, size_bits: u32, weight, style, features: Vec<(u32, u32)>, cells: u16 }` (or equivalent hashable form). Buffers: `Metrics::new(font_size, cell_height)`, size `(cells * cell_width + cell_width, cell_height)`, `Shaping::Advanced`, wrap none. `render` keeps the `Arc`s alive in the cache until after `fill_raw` returns (weak references are upgraded during the frame).

- [ ] **Step 1: Write the failing tests:**
  - `geist_ss11_shapes_an_arrow`: with Geist Mono loaded into `font_system()`, a buffer for `"->"` with features on and `ss11` produces a glyph whose id equals the id of glyph name `hyphen_greater.liga` (looked up with `ttf_parser::Face::glyph_index_by_name`); with ligatures off it does not.
  - `cache_reuses_unchanged_runs`: two frames over the same key → `shaped_count() == 1`; after `end_frame` twice without use, the entry is gone.
  - `new_features_reshape` (Review Focus 3): same text, different features → `shaped_count() == 2`.
- [ ] **Step 2: Run** `cargo test -p fbterm-term shaped` → FAIL.
- [ ] **Step 3: Implement** the cache and switch `render` to it for all text runs (alone runs too, features without detected sets); delete the `fill_text` path for grid text.
- [ ] **Step 4: Run** `cargo test -p fbterm-term` → PASS (existing draw tests unchanged).
- [ ] **Step 5: Commit** `feat(term): shape grid text with OpenType features and draw it through a cache`.

### Task 6: Settings row, wiring, docs, look and cost

**Files:**
- Modify: `crates/fbterm/src/settings_window/{glue.rs,mod.rs,view.rs}` (`terminal_style` and `pane_style` set `features` from `ligatures` + parsed `font_features`; `SettingsMsg::Ligatures(bool)`; the row beside font family and size), banner notice for rejected `font_features` entries (one line per distinct entry, like scheme notices: `font_features: "<entry>" is not a feature tag; it is ignored.`)
- Modify: `website/docs/files.md` (sample lines and rows for `ligatures` and `font_features`), the fonts section of the docs (where font family and size are documented) with a short "Ligatures" paragraph
- Test: `crates/fbterm/src/settings_window/tests.rs`, `crates/fbterm/src/restore_tests.rs`

- [ ] **Step 1: Write the failing tests:** `SettingsMsg::Ligatures(false)` queues `SettingChange::Ligatures(false)`; with `ligatures = false` in `settings.toml`, `App::pane_style(..).features.ligatures == false`, and after a `ConfigChanged` back to `true` it is `true` (Review Focus 3); `font_features = ["bad!"]` yields the banner line above once.
- [ ] **Step 2: Run** `cargo test -p fbterm ligatures` → FAIL.
- [ ] **Step 3: Implement** the row, wiring, notice and docs.
- [ ] **Step 4: Run** the full gate → PASS.
- [ ] **Step 5: Look (macOS, by hand):** debug build, `FBTERM_CONFIG_DIR` with a neutral profile `demo` printing `-> => != >= a->b` and holding the cursor after it, `FBTERM_SECRETS=memory`, `FBTERM_OPEN_TABS=demo`, `FBTERM_SCREENSHOT`; confirm the ligatures show, then with `ligatures = false` that they do not. Then a profile that leaves the cursor on the `>` of `->` (printf then `\e[1D`-style move) and confirm that ligature breaks.
- [ ] **Step 6: Cost:** release builds of `develop` and this branch, `--perf-panes 1 --perf-cmd` running a script that prints a full screen of mixed code-like text continuously (`yes 'fn a() -> b { x != y && z >= 1 }' | head -c 200000000`), sampling the fbterm process's CPU with `ps` once a second for 30 s, 3 runs each; the branch's median within 10% of `develop`'s. Record both medians for the PR.
- [ ] **Step 7: Commit** `feat: Ligatures switch and font_features, applied to open terminals`.
