# Cold History Compression Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Opt-in compression of cold scrollback blocks in feathercritty, driven by a background
worker in fbterm and two new settings, so very large histories cost a fraction of their memory.

**Architecture:** feathercritty's `History` gains a second block state, cold (whole 64 KiB blocks
compressed). Blocks change state only through jobs: copy out under the lock, compress or
decompress without it, install under it with an identity check. Reads of cold blocks go through
a 4-block decompression cache. fbterm runs one shared low-priority worker thread that the session
feed and setting changes wake. Its settings are `history_compression` and
`uncompressed_history_lines`.

**Tech Stack:** Rust 2024, feathercritty (fork of alacritty_terminal 0.26), fbterm (iced 0.14),
and one compressor crate chosen in Task 1 (`lz4_flex` with safe features, or a zstd crate).

**Spec:** `docs/superpowers/specs/2026-11-08-history-compression-design.md` (this worktree).
Parent: `docs/superpowers/specs/2026-10-24-compact-history-design.md`.

**Repos and branches:**
- feathercritty: `../feathercritty`, branch `feature/history-compression` (created off `main`).
- fbterm: this worktree, branch `feature/history-compression`.
- During fbterm tasks, the root `Cargo.toml` gets an uncommitted
  `[patch."https://github.com/featherbitplatform/feathercritty"] feathercritty = { path = "../../../feathercritty" }`.
  Never commit it. Task 13 replaces it with a `rev` bump.

## Global Constraints

- **Off means off:** with `history_compression: None`, no code path added here runs beyond one
  `Option` branch in `push_newest`. Ingest stays within 1%, live bytes are identical, and there
  are zero allocations per scrolled row at a full history.
- **No `unsafe`:** no new `unsafe` in feathercritty.
- **Purity:** `History` stays `Send`. Records stay pointer-free bytes.
- **Fork conventions:** every modified feathercritty file keeps the notice line at its top.
  Conventional Commits in both repos. No `Co-Authored-By`, `Claude-Session` or "Generated with"
  lines anywhere.
- **Naming:** never name another terminal application in anything committed. No credit to
  other terminal projects.
- **Lock budget:** the worker holds a terminal lock at most 0.1 ms per step (p99): `next_job`
  plus the copy, or `install`. It never compresses, decompresses or copies a whole block inside
  `install`.
- **Remote programs** never cause `memory::release_freed()`. Only user actions do: switching on,
  lowering the uncompressed value, lowering scrollback.
- **Settings:** `history_compression` (bool, default false). `uncompressed_history_lines`
  (absent means `min(10_000, scrollback_lines - 1)`; valid range `1_000..scrollback_lines`).
  Their field is disabled while the switch is off, and when `scrollback_lines <= 1_000`, with
  the note `Needs scrollback above 1,000 lines.`
- **Switch note (exact copy):** `Saves memory on very long histories, at the cost of extra CPU
  when output scrolls into old history and when you scroll back, search or export. Recommended
  for scrollback of 100,000 lines or more.`
- **Gates** are measured in release builds on the owner's M5 Max, medians of at least 5 runs
  with the spread reported (spec 6).
- **Test data:** recorded streams committed as test data contain no home-directory paths, user
  names or emails (redact paths to `/work`, and format `git log` without authors).

## Review Focus

1. **A job installed after its block changed.** Eviction, `clear`, a `set_limit`, a reflow, or a
   compression setting change can each land between `next_job` and `install`. Expected: refused,
   and the history stays correct. Pinned in Task 3 (`stale_job_refused_after_*`) and fuzzed in
   Task 7.
2. **A reflow while blocks are cold or jobs are in flight.** Expected: identical rows, peak live
   bytes never above off, and the old history's in-flight jobs refused. Pinned in Task 5
   (`reflow_over_cold_history_matches_warm`, `job_from_before_reflow_is_refused`).
3. **Lowering scrollback below the uncompressed value, or to 1,000 or less.** Expected: the
   value is dragged down to `scrollback - 1` (never below 1,000), the confirmation says so, and
   with scrollback at 1,000 or less nothing compresses. Pinned in Task 9
   (`lowering_scrollback_drags_uncompressed_down`) and Task 11.
4. **Turning compression off on a huge tab.** Expected: decompression happens in background jobs,
   the tab stays usable, and memory grows back to off size, never above. Pinned in Task 4
   (`switching_off_warms_every_block`) and Task 10 (`worker_runs_jobs_until_none`).
5. **A closed tab with queued jobs.** Expected: the worker skips it without a panic or a leak.
   Pinned in Task 10 (`worker_skips_dropped_terminal`).

---

## Phase 0 and 1: feathercritty (`../feathercritty`)

### Task 1: Realistic corpora and the compressor choice

**Files:**
- Create: `tests/corpora/mod.rs`, the realistic generators.
- Create: `tests/corpora/data/cargo-build.log` and `tests/corpora/data/git-graph.log`, recorded
  and redacted.
- Create: `tests/compress_choice.rs`, an `#[ignore]` comparison test.
- Modify: `Cargo.toml` (`[dev-dependencies]` for the candidates; the winner moves to
  `[dependencies]` in Task 2), `benches/RESULTS.md` (new section "Compressor choice").

**Interfaces:**
- Produces:
  - `pub fn build_log(seed: u64, rows: usize) -> Vec<u8>`
  - `pub fn service_log(seed: u64, rows: usize) -> Vec<u8>`
  - `pub fn shell_session(seed: u64, rows: usize) -> Vec<u8>`
  - `pub fn recorded(name: &str) -> Vec<u8>`, for `"cargo-build"` and `"git-graph"`

  Each returns bytes to feed a `Term`, with SGR colors, one row per newline. All are
  deterministic per seed.
- Produces: a recorded decision, compressor plus level, used by Task 2.

- [ ] **Step 1: Write the generators.**
  - `build_log`: `   Compiling <crate> v<x.y.z>` lines, `warning: unused variable` blocks
    with `--> src/<path>.rs:<l>:<c>` and gutter lines, with crate and path names drawn from
    fixed lists.
  - `service_log`: an ISO timestamp, a colored level (`INFO`, `WARN`, `ERROR`), a module path,
    and a message from 20 templates with varying ids, durations and IPs.
  - `shell_session`: a colored prompt, a command from a list, then 1 to 30 lines of `ls -l`
    style colored output.
- [ ] **Step 2: Record the two streams.**
  - From this repository: `git log --graph --color=always --format='%C(auto)%h %s' -n 3000`.
  - From the fbterm worktree: `cargo build --color=always 2>&1` after `cargo clean -p fbterm`.
  - Replace every absolute path prefix with `/work`. Check the files contain no `/Users/`, no
    `/home/` and no `@`.
- [ ] **Step 3: Write `compress_choice`** (`#[ignore]`). For each corpus, it fills a `Term` at
  126 columns with a 1,000,000-line limit via `Processor::advance`. It collects the history's
  64 KiB block bytes through the test-only accessor `History::block_bytes_for_test(&self) ->
  Vec<&[u8]>` (add it under `#[cfg(any(test, feature = "test-internals"))]`, or reuse an
  existing test feature if the crate has one). For each candidate (`lz4_flex` block format with
  `safe-encode`/`safe-decode`; `zstd` at levels 1 and 3; a pure-Rust zstd encoder if one passes
  `cargo deny`), it prints the ratio, compression MB/s and decompression MB/s per block.
- [ ] **Step 4: Run it.**
  Run: `cargo test --release --test compress_choice -- --ignored --nocapture`.
  Expected: a table per corpus. Paste it into `benches/RESULTS.md` under "Compressor choice".
- [ ] **Step 5: Choose, by spec 5.3's rule.**
  1. Projected compressed history, ratio applied to block bytes plus 16 bytes per row index,
     must be at most 40% of off for every realistic corpus.
  2. Among those, take the fastest decompression.
  3. Prefer pure Rust when it is within 20% on both ratio and speed.

  Run `cargo deny check` with the winner. Write the decision and its numbers in RESULTS.md, and
  ledger it as a ruling.
- [ ] **Step 6: Commit.**
  `git add tests/corpora tests/compress_choice.rs Cargo.toml Cargo.lock benches/RESULTS.md src`
  `git commit -m "test: realistic history corpora and compressor comparison"`

### Task 2: Cold block codec

**Files:**
- Create: `src/grid/cold.rs` (declared in `src/grid/mod.rs` as `mod cold;`).
- Modify: `Cargo.toml` (the winner moves to `[dependencies]`; drop the losing candidates).

**Interfaces:**
- Produces:
  - `pub(crate) fn compress(warm: &[u8], out: &mut Vec<u8>)`. It writes the header (`u8` format
    tag `1`, `u32` LE uncompressed length), then the payload.
  - `pub(crate) fn decompress(cold: &[u8], out: &mut Vec<u8>)`. It clears `out`, fills it with
    exactly the uncompressed bytes, and panics on a bad tag or a length mismatch. Cold bytes are
    only ever produced by `compress`, so a mismatch is a bug.
  - `pub(crate) fn uncompressed_len(cold: &[u8]) -> usize`.

- [ ] **Step 1: Write the failing tests in `cold.rs`:**
  - `round_trips_empty_and_full_blocks`: an empty input, 1 byte, 64 KiB of zeros, 64 KiB of
    random bytes, and a block taken from `tests/corpora`-like text. `decompress(compress(x)) == x`
    and `uncompressed_len` equals the input length.
  - `reuses_output_buffer`: decompressing into a `Vec` with capacity at least the result does
    not reallocate.
  - `rejects_unknown_tag` (`#[should_panic]`).
- [ ] **Step 2: Run them, expecting a failure.**
  Run: `cargo test --lib grid::cold`. Expected: FAIL, functions not defined.
- [ ] **Step 3: Implement** with the Task 1 winner at its chosen level.
- [ ] **Step 4: Run them, expecting them to pass.** `cargo test --lib grid::cold` → PASS, and
  `cargo clippy --all-targets -- -D warnings` is clean.
- [ ] **Step 5: Commit.** `git commit -m "feat(grid): codec for cold history blocks"`

### Task 3: Cold blocks in `History`: state, jobs and reads

**Files:**
- Modify: `src/grid/history.rs`: `Block`, `History`, new `Job`/`Done`, the read path, `update`,
  `pop_newest`, `release`, `clear`, `Clone`, and the module docs (a new section "Cold blocks").
- Test: `src/grid/history.rs` `#[cfg(test)]`, or `src/grid/tests.rs` where history tests live.

**Interfaces:**
- Consumes: `cold::{compress, decompress, uncompressed_len}`.
- Produces (all `pub`, re-exported from `grid` like `History`):
  - `History::set_compression(&mut self, keep_warm: Option<usize>)`. `None` means never
    compress. Changing it only marks blocks for jobs; it never compresses inline.
  - `History::compression(&self) -> Option<usize>`.
  - `History::has_jobs(&self) -> bool`. It must be O(1): keep a counter or flag that
    `push_newest` and `set_compression` update.
  - `History::next_job(&mut self) -> Option<Job>`. A `Job` holds the target state, the block
    identity `(seq: u32, generation: u64)` and an owned copy of the block's current bytes.
  - `Job::run(self) -> Done`, with no access to the history.
  - `History::install(&mut self, done: Done) -> bool`
  - `History::run_jobs(&mut self)`: runs and installs every job inline (the synchronous test
    mode, spec 6.1).
- Rules (spec 5.1):
  - Block eligibility is "all rows at index `i >= keep_warm`" (0 is the newest row).
  - The newest block is never cold.
  - Every `Block` gets `generation: u64`, from a history-wide counter bumped on every state
    change, on creation and on `clear`. That keeps identity unique across `front_seq` resets.
  - `release` never truncates a cold block.
  - A cold block whose `live` reaches 0 is freed, never kept as a spare.
  - `update` and `pop_newest` on a cold row read through the cache. `pop_newest` reaching a cold
    block warms it inline. This is rare: it needs a screen taller than the warm window.
- Read cache: `RefCell<Cache>` in `History` with at most `CACHE_BLOCKS = 4` decompressed blocks,
  least recently used first, keyed by identity. `record(entry)` returns bytes through it.
  `with_extras` and `decode` are unchanged. The cache is cleared on `clear`, on `set_limit`
  lowering, on `set_compression(None)`, and on any install that changes a cached block.

- [ ] **Step 1: Write the failing tests** (blocks of 256 bytes via `with_block_size`, rows of
  `usize` cells as the existing history tests use):
  - `cold_rows_read_identically`: push 2,000 varied rows, `set_compression(Some(10))`,
    `run_jobs()`. Every `read(i)`, `cell(i, c)`, `wraps(i)` and `is_clear(i)` equals the same
    calls on a clone taken before compression.
  - `newest_block_never_cold`: after `run_jobs`, the newest block is warm, and every block whose
    rows are all `i >= keep_warm` is cold. Check through a test accessor
    `cold_blocks() -> Vec<bool>`.
  - `stale_job_refused_after_eviction`, `stale_job_refused_after_clear`,
    `stale_job_refused_after_set_limit` and `stale_job_refused_after_setting_change`. Each takes
    `next_job()`, interferes, then `install(job.run())` returns `false` and reads still equal
    the warm clone.
  - `switching_off_warms_every_block`: compress, then `set_compression(None)`, `run_jobs()`. No
    cold blocks remain and reads are unchanged.
  - `raising_keep_warm_warms_blocks` and `lowering_keep_warm_cools_blocks`.
  - `update_on_cold_row`: `update(i, f)` on a cold row changes exactly that row.
  - `pop_newest_into_cold_block`: with `keep_warm = 0`, pop every row newest-first. Each popped
    row equals the warm clone's.
  - `cache_holds_at_most_four_blocks`: reading rows from 6 different cold blocks keeps 4
    decompressed. Check through a test accessor `cached_blocks() -> usize`.
  - `off_does_nothing`: with `None`, `has_jobs()` is false after 10,000 pushes and `next_job()`
    is `None`.
- [ ] **Step 2: Run them, expecting a failure.** `cargo test --lib history` → FAIL (methods not
  defined).
- [ ] **Step 3: Implement** per the Interfaces block.
  - `push_newest` marks a job pending only when `self.compression.is_some()`, and only after the
    index passes a block boundary of the warm window. Use one comparison and keep it cheap.
  - Cold blocks keep `bytes: Vec<u8>` holding the compressed form, shrunk to fit, plus
    `cold: bool`.
- [ ] **Step 4: Run them, expecting them to pass.** `cargo test --lib` (all, including the
  existing history tests) → PASS. Clippy is clean.
- [ ] **Step 5: Commit.** `git commit -m "feat(grid): cold history blocks with background jobs and a read cache"`

### Task 4: Header bits in the index

**Files:**
- Modify: `src/grid/history.rs` (`Entry`, `wraps`, `is_clear`, `store`, `encode_in_place`,
  `move_oldest_to`).

**Interfaces:**
- Produces: `wraps(i)` and `is_clear(i)` never touch block bytes. `Entry.len` keeps the length in
  its low 30 bits, with `WRAPS` at bit 31 and `CLEAR` at bit 30. A `debug_assert!` ensures a
  record length is below `1 << 30`. Accessors `Entry::len()`, `Entry::wraps()` and
  `Entry::clear()` replace direct field reads.

- [ ] **Step 1: Write the failing test** `wraps_and_clear_never_decompress`: compress a history,
  clear the cache, call `wraps` and `is_clear` for every row, then assert `cached_blocks() == 0`.
  Also assert the results equal `read_header` of each decoded record.
- [ ] **Step 2: Run it, expecting a failure.** `cargo test --lib wraps_and_clear_never_decompress`
  → FAIL (the cache filled).
- [ ] **Step 3: Implement.** Set the bits from the encoded record's header at store time.
- [ ] **Step 4: Run, expecting a pass.** `cargo test --lib` → PASS.
- [ ] **Step 5: Commit.** `git commit -m "perf(grid): answer wraps and is_clear from the history index"`

### Task 5: Config, `Term` and `Grid` plumbing, reflow

**Files:**
- Modify: `src/term/mod.rs` (`Config::history_compression`, `Term::set_options`, new `Term`
  methods).
- Modify: `src/grid/mod.rs` (`Grid::set_history_compression`, lines 587 and 886 where histories
  are built).
- Modify: `src/grid/resize.rs` (lines 414 and 430).
- Modify: `src/grid/history.rs` (`History::sibling`).

**Interfaces:**
- Produces:
  - `Config { pub history_compression: Option<usize>, .. }`, defaulting to `None`.
  - `History::sibling(&self, limit: usize) -> History<T>`: an empty history with the same block
    size and compression. It replaces the four `History::with_block_size(…, self.history.block_bytes())`
    sites.
  - `Grid::set_history_compression(&mut self, keep_warm: Option<usize>)`
  - On `Term<T>`:
    - `has_history_jobs(&self) -> bool`
    - `next_history_job(&mut self) -> Option<HistoryJob>`
    - `install_history_job(&mut self, done: HistoryDone) -> bool`
    - `run_history_jobs(&mut self)`

    `HistoryJob` and `HistoryDone` wrap `Job` and `Done` and also carry which grid they belong
    to (normal or inactive) plus that grid's reflow epoch, a `u64` bumped by every resize that
    rebuilds the history. Install refuses a mismatch.
  - `Term::set_options` applies `history_compression` to both grids. The alternate grid has
    limit 0, so it never has jobs.
- Reflow (spec 5.6): the new history is built with `sibling`, so it has the same compression
  setting. It does no compression inline; `push_*` only marks jobs. Old cold blocks are read
  through the cache while `RowSource` pops them, and freed as their last row leaves.

- [ ] **Step 1: Write the failing tests** in `src/grid/tests.rs` (a `Term` with history 5,000,
  tiny blocks, `history_compression: Some(50)`):
  - `reflow_over_cold_history_matches_warm`: fill with wrapping colored lines, then
    `run_history_jobs`. Resize 126→100→126 columns and 40→25 lines. Every row equals the same
    sequence on a term with `None`.
  - `job_from_before_reflow_is_refused`: `next_history_job`, resize, then
    `install_history_job(job.run())` is `false`.
  - `set_options_reaches_the_normal_grid_under_alt_screen`: enter the alt screen,
    `set_options` with `Some(50)`, leave it. `has_history_jobs()` is true and `run_history_jobs`
    cools blocks.
  - `default_config_never_has_jobs`.
- [ ] **Step 2: Run them, expecting a failure.** `cargo test --lib grid::tests` → FAIL.
- [ ] **Step 3: Implement** per the Interfaces block.
- [ ] **Step 4: Run, expecting a pass.** `cargo test` and `cargo test --no-default-features` →
  PASS. Clippy is clean for both feature sets.
- [ ] **Step 5: Commit.** `git commit -m "feat(term): history_compression option and job API on Term"`

### Task 6: Large-history and peak-memory checks

**Files:**
- Modify: `tests/large_history.rs` (compression-on variants, realistic content via
  `tests/corpora`).
- Modify: `tests/alloc.rs` (off-path allocation check, and a peak-bytes check for reflow).

- [ ] **Step 1: Write the failing tests:**
  - `large_history_invariants_at_1m_compressed` (`#[ignore]`): the existing 1M invariants run
    with `Some(10_000)` and `run_history_jobs()` after the fill and after each resize.
  - `large_history_matches_oracle_at_100k_compressed` (`#[ignore]`): the existing 100k oracle
    comparison with compression on.
  - `alloc::compression_off_allocates_nothing_per_row`: the existing zero-allocation check with
    `history_compression: None` set explicitly.
  - `alloc::reflow_peak_with_cold_history_not_above_off`: a 100,000-line realistic
    `service_log` history, compressed. Its peak live bytes during 126→100→126 are at most the
    peak of the same history with `None`.
  - `large_history_cost_table` (manual, existing) gains columns for compressed live bytes per
    realistic corpus.
- [ ] **Step 2: Run them, expecting a failure.**
  `cargo test --release --test large_history -- --ignored --exact large_history_invariants_at_1m_compressed`
  and `cargo test --release --test alloc` → FAIL before the variants compile.
- [ ] **Step 3: Implement the variants.** Update the CI workflow's large-history step to include
  both `_compressed` tests.
- [ ] **Step 4: Run, expecting a pass.** Same commands → PASS.
- [ ] **Step 5: Commit.** `git commit -m "test: large-history and allocation checks with compression on"`

### Task 7: Differential pass with cold history

**Files:**
- Modify: `tests/support/mod.rs` (`DIFF_COLD` knob, documented in the module docs of
  `tests/differential.rs`).
- Modify: `tests/support/fc.rs` (`Side::apply` runs jobs).
- Modify: `.github/workflows/*.yml` (a `DIFF_COLD` pass beside the `DIFF_TINY` one, on all three
  OSes).

**Interfaces:**
- `DIFF_COLD=<n>` sets `history_compression: Some(n)` on feathercritty's side. After each step,
  `fc::Side` uses its own `ChaCha8Rng`, seeded from the case seed, to pick one of three actions:
  - run every job;
  - run some jobs and drop some results (run, not installed);
  - take a job, then apply the step's interference before installing it.

  The oracle side is untouched, so the generator's streams and seeds don't change.

- [ ] **Step 1: Add the knob and the side behavior,** then run
  `DIFF_COLD=1 DIFF_TINY=1 DIFF_SEEDS=50 cargo test --release --test differential`.
  Expected: PASS. If it diverges, debug with `superpowers:systematic-debugging` and fix the
  library, never the harness comparison.
- [ ] **Step 2: Run the full CI-sized passes** with `DIFF_COLD` set to 0, 1 and 37, each with
  and without `DIFF_TINY`. Expected: zero differences. Record the seeds and the runtime in the
  PR text later.
- [ ] **Step 3: Commit.** `git commit -m "test: differential pass with cold history and racing jobs"`

### Task 8: Library gates, docs and release of the fork

**Files:**
- Modify: `benches/ingest.rs` and `benches/reads.rs` (compression-on variants at a
  1,000,000-line history, and a lock-hold measurement of `next_history_job` and
  `install_history_job`).
- Modify: `benches/RESULTS.md`, `README.md` (new change entry "9. Cold history compression":
  what, why, invariants, the merge note "nothing upstream corresponds"), `Cargo.toml` (version
  `0.26.0+fbterm.7`).

- [ ] **Step 1: Run the gates of spec 6.2 to 6.4 at library level.**
  - Ingest off against `main`: within 1%.
  - Ingest on with jobs: parser side at most 3% slower than off. Jobs run on a second thread
    in the bench to model the worker.
  - Lock holds: p99 at most 0.1 ms.
  - Memory per realistic corpus: at most 40% of off.
  - Search, export-equivalent reads and reflow time: at most 2x off. Reflow at most off plus
    decompress time, with a 1 s ceiling.

  Record the numbers in RESULTS.md. A missed gate stops the task. Debug it, or ledger a ruling
  if the spec's gate is wrong, and bring it to the owner in the final report.
- [ ] **Step 2: Write the README entry and the module docs section.**
- [ ] **Step 3: Run the full fork gate.** `cargo +nightly fmt --check`,
  `cargo clippy --all-targets -- -D warnings` (with and without default features),
  `cargo test`, `cargo test --no-default-features`. Expected: all PASS.
- [ ] **Step 4: Commit and push the branch.**
  `git commit -m "docs: cold history compression results and change entry"` then
  `git push -u origin feature/history-compression`. Opening and merging the PR is the owner's;
  ask before merging.

## Phase 2: fbterm (this worktree)

### Task 9: Settings fields and rules

**Files:**
- Modify: `crates/fbterm-config/src/settings.rs`: fields, defaults, `validate` notices, the
  template lines, and a new helper.
- Modify: `crates/fbterm-config/src/settings_edit.rs`: `SettingChange::HistoryCompression(bool)`
  and `SettingChange::UncompressedHistoryLines(usize)`, both key names.
- Modify: `website/docs/files.md`: two rows. The `history_compression` row states the trade-off (memory against CPU on reads and when output scrolls into old history; recommended for 100,000 lines or more). There is no separate Settings docs page.

**Interfaces:**
- Produces:
  - `Settings { pub history_compression: bool, pub uncompressed_history_lines: Option<usize>, .. }`
  - `pub const UNCOMPRESSED_FLOOR: usize = 1_000;`
  - `pub const UNCOMPRESSED_DEFAULT: usize = 10_000;`
  - `Settings::history_compression_keep(&self) -> Option<usize>`: `None` when the switch is off
    or `scrollback_lines <= UNCOMPRESSED_FLOOR`. Otherwise the effective value: the setting, or
    `min(UNCOMPRESSED_DEFAULT, scrollback_lines - 1)` when absent, clamped to
    `UNCOMPRESSED_FLOOR..=scrollback_lines - 1`.
  - Template lines `#history_compression = false` and
    `#uncompressed_history_lines = 10000  # at most scrollback_lines - 1`.

- [ ] **Step 1: Write the failing tests** in `settings.rs`:
  - `history_compression_defaults_off`: defaults give `history_compression_keep() == None`.
  - `uncompressed_default_tracks_scrollback`: switch on, scrollback 10,000 and value absent →
    `Some(9_999)`, with no notice. Scrollback 1,000,000 and absent → `Some(10_000)`.
  - `uncompressed_out_of_range_is_clamped_with_notice`: value 500 → 1,000 with a notice naming
    `uncompressed_history_lines`. Value 50,000 with scrollback 20,000 → 19,999 with a notice.
  - `small_scrollback_disables_compression`: scrollback 1,000 with the switch on → `None`.
  - `lowering_scrollback_drags_uncompressed_down`: `apply(SettingChange::ScrollbackLines(5_000))`
    on settings with value 8,000 also sets the value to `Some(4_999)` (the edit writes both
    keys).
  - The template test asserts both commented lines.
- [ ] **Step 2: Run, expecting a failure.** `cargo test -p fbterm-config` → FAIL.
- [ ] **Step 3: Implement.** The edit for `ScrollbackLines` writes `uncompressed_history_lines`
  too, but only when the key is present and would become invalid.
- [ ] **Step 4: Run, expecting a pass.** `cargo test -p fbterm-config` → PASS.
- [ ] **Step 5: Commit.** `git commit -m "feat(config): history compression settings"`

### Task 10: Terminal wiring and the history worker

**Files:**
- Create: `crates/fbterm-term/src/history_worker.rs` (exported as `fbterm_term::history_worker`).
- Modify: `crates/fbterm-term/src/terminal.rs`: `set_history_compression`, `has_history_jobs`,
  and job passthroughs.
- Modify: `crates/fbterm-session/src/lib.rs`: in `Feeder::feed`, after `term.feed`, if
  `term.has_history_jobs()`, call `history_worker::enqueue(&self.terminal)`.
- Modify: `crates/fbterm/src/sessions.rs:125`: new terminals get `settings.history_compression_keep()`.

**Interfaces:**
- Produces:
  - `Terminal::set_history_compression(&mut self, keep: Option<usize>)`
  - `Terminal::has_history_jobs(&self) -> bool`
  - `Terminal::next_history_job(&mut self) -> Option<HistoryJob>`
  - `Terminal::install_history_job(&mut self, done: HistoryDone) -> bool`
  - `history_worker::enqueue(term: &SharedTerminal)`: stores a `Weak`, deduplicated by pointer,
    and wakes the worker.
  - `history_worker::Worker`: a testable unit with `new()`, `enqueue`, and
    `run_until_idle(&self)` for tests. The process-wide instance is a `OnceLock<Worker>` whose
    thread `fbterm-history-worker` starts on the first `enqueue`. Lowered priority: `libc::nice`
    is per-thread on Linux; on macOS use `pthread_set_qos_class_self_np(QOS_CLASS_UTILITY)`
    behind `cfg`; on Windows `SetThreadPriority(BELOW_NORMAL)` if the windows crate already has
    the feature, else skip it with a comment. The one FFI call per platform is allowed in fbterm,
    not in feathercritty.
- Loop: pop a terminal round robin, upgrade its `Weak` (skip if dropped). Lock, `next_history_job`,
  unlock. `run`. Lock, `install_history_job`, unlock. Requeue it if it still `has_history_jobs()`.
  Block on a `Condvar` when the queue is empty. Never tick.

- [ ] **Step 1: Write the failing tests** in `history_worker.rs` (using `Worker::new()`, not the
  global):
  - `worker_runs_jobs_until_none`: a terminal with history 5,000 and compression `Some(1_000)`,
    fed 20,000 lines. After `enqueue` and `run_until_idle`, `has_history_jobs()` is false. Every
    history row read through `RowReader` equals the same terminal with compression off.
  - `worker_skips_dropped_terminal`: enqueue, drop the terminal, then `run_until_idle` returns
    without a panic.
  - `worker_round_robins_terminals`: two terminals with jobs both end with no jobs, and their
    install calls interleave. Record the order in a test hook.
- [ ] **Step 2: Run, expecting a failure.** `cargo test -p fbterm-term history_worker` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run, expecting a pass.** `cargo test -p fbterm-term` and
  `cargo test -p fbterm-session` → PASS.
- [ ] **Step 5: Commit.** `git commit -m "feat(term): background history worker"`

### Task 11: Applying the settings to open tabs

**Files:**
- Modify: `crates/fbterm/src/settings_window/glue.rs`: in `apply_settings`, when
  `history_compression_keep()` changes, call a new `App::apply_history_compression(keep)`.
- Test: `crates/fbterm/src/settings_window/app_tests.rs`.

**Interfaces:**
- `apply_history_compression` sets the value on every pane's terminal (normal screen; the
  library ignores the alternate one) and enqueues each one. It calls
  `crate::memory::release_freed()` after the worker drains, only when the change frees memory:
  `None → Some` or a lower keep. Implement that as a one-shot task on the blocking pool that
  waits for the worker to go idle (`Worker::wait_idle()`, add it) and then releases. Use no
  timer loop.
- `apply_scrollback` runs before it in the same apply, so a dragged-down value is already in
  `new`.

- [ ] **Step 1: Write the failing tests:**
  - `compression_setting_reaches_open_terminals`: two tabs, apply switch on → each terminal's
    `has_history_jobs()` or cold state is set (compression is `Some`).
  - `compression_off_applies_to_open_terminals`.
  - `switching_on_requests_release_once_idle`: uses the existing release counter or test hook in
    `memory.rs` (add one if absent). Exactly 1 request after idle.
  - `output_driven_compression_never_requests_release`: feed output with compression on, run
    the worker idle, then 0 requests.
  - `lowering_scrollback_below_uncompressed_applies_both`.
- [ ] **Step 2: Run, expecting a failure.** `cargo test -p fbterm settings_window` → FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run, expecting a pass.** `cargo test -p fbterm` → PASS.
- [ ] **Step 5: Commit.** `git commit -m "feat: apply history compression to open tabs"`

### Task 12: Settings rows

**Files:**
- Modify: `crates/fbterm/src/settings_window/mod.rs`:
  - `SettingsMsg::HistoryCompression(bool)`, `UncompressedText(String)` and `UncompressedSubmit`;
  - state `uncompressed_text`;
  - the note constants;
  - one extra line in the scrollback-lowering confirmation when the drag applies:
    `Recent lines kept uncompressed will drop to {n}.`
- Modify: `crates/fbterm/src/settings_window/view.rs`: two rows under Scrollback, using the
  existing `switch` and number-field helpers.
- Test: `crates/fbterm/src/settings_window/tests.rs`.

- [ ] **Step 1: Write the failing tests:**
  - `compression_switch_saves`;
  - `uncompressed_field_disabled_while_off`;
  - `uncompressed_field_disabled_with_small_scrollback` (the note text is exact);
  - `uncompressed_out_of_range_saves_nothing` (typing 500, or a value at or above scrollback,
    shows the range and saves nothing);
  - `confirmation_mentions_uncompressed_drop`.
- [ ] **Step 2: Run, expecting a failure.** `cargo test -p fbterm settings_window::tests` → FAIL.
- [ ] **Step 3: Implement** with the exact copy from Global Constraints.
- [ ] **Step 4: Run, expecting a pass.** Then take own-window screenshots with a throwaway config
  (`FBTERM_CONFIG_DIR=$(mktemp -d)`, `FBTERM_SECRETS=memory`, `FBTERM_OPEN_SETTINGS=1`,
  `FBTERM_SCREENSHOT_WINDOW=settings`, `FBTERM_SCREENSHOT=target/shots/<name>.png`): switch off,
  switch on, and scrollback at 1,000. Look at each one.
- [ ] **Step 5: Commit.** `git commit -m "feat(settings): history compression rows"`

### Task 13: Compression-on variants, measurements, rev bump

**Files:**
- Modify: the fbterm tests that read history (the export golden tests in
  `crates/fbterm-term/src/export.rs`, search tests in `crates/fbterm-term/src/search.rs`, the
  draw-list tests that scroll into history in `crates/fbterm-term/src/widget/`). Each gets a
  compression-on variant: `Some(1)` or the smallest the test's history allows, plus
  `run_history_jobs()` before reading, asserting byte-identical output.
- Modify: `dev/perf/sessions.sh` (scenario I) and `dev/perf/README.md`.
- Modify: root `Cargo.toml` (feathercritty `rev`, after the fork's PR merges), `Cargo.lock`.
- Modify: `docs/superpowers/specs/2026-10-23-featherbit-terminal-design.md` (sections 6 and 7:
  one line each).

- [ ] **Step 1: Write the variants, run them, expecting a pass.** `cargo test --workspace` →
  PASS. If a variant differs, that is a library bug: debug it in feathercritty.
- [ ] **Step 2: fbterm gates (spec 6.4, fbterm rows):**
  - draw-list build in cold history: first frame at most 2 ms, following frames at most 1.15x
    warm (extend `crates/fbterm-term/examples/search_bench.rs`, or add `cold_bench.rs` beside
    it);
  - search over 1,000,000 log lines: at most 2x off;
  - export: at most 2x off.

  Record the numbers in `dev/perf/README.md`.
- [ ] **Step 3: Scenario I:** one tab, `scrollback_lines = 1000000`, `service_log`-style output.
  Record RSS and footprint off and on after the backlog drains, plus idle CPU over 10 s.
  Expected: idle CPU 0.00% with compression on. Record it in `dev/perf/README.md`.
- [ ] **Step 4: After the owner merges the feathercritty PR,** remove the `[patch]`, set `rev`
  to the merge commit's full SHA, and run the full gate: `cargo fmt --all --check`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`,
  `cargo deny --all-features check`. Expected: all PASS.
- [ ] **Step 5: Commit.** `git commit -m "feat: cold history compression"` (plus separate
  `test:`/`docs:` commits as natural). Push and open the PR only when the owner asks.
