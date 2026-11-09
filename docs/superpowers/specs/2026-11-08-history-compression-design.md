# Cold history compression: design

Date: 2026-11-08. Status: awaiting the owner's approval.

Scope: the emulation library fork `feathercritty` (`../feathercritty`, the history store in
`src/grid/history.rs`) and the parts of featherbit terminal (`fbterm`) that configure it, run its
background work and show its settings. Parent designs: `2026-10-23-featherbit-terminal-design.md`
and `2026-10-24-compact-history-design.md`. This design builds on the second one and doesn't
change it. Every rule there still holds (compact records, pooled blocks, the read API, the
differential oracle, clean-room rule).

Items marked *(owner's decision)* were decided by the owner on 2026-11-08. Items marked
*(builder's call)* were decided while writing this design. The owner can override any of them,
and the plan may revisit one if a measurement shows a better option.

## 1. Why

Compact history (0.1.0) cut a full 10,000-line tab from about 21 MB to about 1 MB, so the
default scrollback is already cheap: 20 full tabs use about 123 MB, of which about 100 MB is the
process itself. Large settings are a different story. `scrollback_lines` goes up to 1,000,000,
and feathercritty's large-history measurements (`dev/perf/README.md`, "Large histories") show
what one tab costs there:

| Content | 100,000 lines | 1,000,000 lines |
|---|---|---|
| short lines | 7.9 MB | 57.8 MB |
| log lines (95 characters, 4 colors) | 20.1 MB | 179.5 MB |
| full-width lines | 19.5 MB | 173.8 MB |

Almost none of that is ever read again: history far from the bottom is reached only by scrolling
back, searching, exporting, selecting or resizing. Compact records are text plus attribute runs,
and they still repeat a lot between rows (the same colors, the same log prefixes, the same
words). General-purpose compression of whole blocks removes that repetition. Typical ratios on
text like this are 3 to 6 times.

## 2. Decided by the owner

- **Target: very large histories.** The default 10,000-line case is not what this project is
  measured on.
- **Off by default.** Nobody's behavior changes unless they switch it on.
- **Two settings ship with this project:** an on/off switch, and the number of recent lines kept
  uncompressed. The uncompressed-lines value must always stay below `scrollback_lines`. Its field
  is disabled while the switch is off.
- **Honest trade-off in Settings.** The switch says it saves memory at the cost of extra CPU,
  and that it's recommended for very large histories.
- **The compressor and its level are not settings.** They are chosen by measurement (5.3).
- **Lock work is a separate project.** Holding the terminal lock less while drawing (the second
  half of roadmap step 5) gets its own design after this one ships.
- Standing rules: changes to emulation go in feathercritty and never upstream. fbterm pins the
  fork by `rev`. No credit to other terminal projects in committed text, and no names of other
  terminal applications.

## 3. Goals, non-goals, anti-goals

**Goals**

- With compression on, the cold part of a large history takes at most 40% of the memory it
  takes today for realistic content (build logs, service logs, shell sessions) (gates in 6.3).
- With compression off (the default), feathercritty and fbterm behave and perform exactly as
  today: same memory, same throughput, no new work anywhere.
- Compression work never runs on the parser's per-byte path and never holds the terminal lock
  for more than a short, bounded step (5.4).
- Reading compressed history gives exactly the same rows as reading uncompressed history, and is
  fast enough that scrolling back, searching and exporting stay usable (gates in 6.4).
- Switching the setting on or off, or changing the uncompressed-lines value, applies to open
  tabs without freezing them.

**Non-goals (for now)**

- Disk spill. The compressed block format stays plain bytes, so it remains ready for that
  (compact-history spec 5.10).
- Compressing the visible screen, or the uncompressed recent lines.
- Shared dictionaries across blocks or tabs, or trained dictionaries.
- Making resize of very large histories faster than today. It must stay correct and within the
  bounds of 6.4.
- Exposing the compressor, its level, block size or cache size.

**Anti-goals** (failures even if memory drops)

- Any observable change in emulation behavior: screen, history, cursor, selections, search
  results, export, reflow.
- Any cost with the switch off.
- A visible stall: a typing echo or frame delayed because compression held the lock, or a
  scrolled-back frame that hitches noticeably on first entry into cold history.
- A memory peak above today's uncompressed size during a resize, or while switching compression
  on or off.
- Host work triggered by remote programs beyond what output already causes. A program clearing
  its history frees blocks as today and never triggers allocator release (compact-history spec
  5.9).
- New `unsafe` in feathercritty.
- A panic or a wrong row when anything reads history, including reads that race with background
  compression.

## 4. User-facing behavior

### 4.1 Settings *(owner's decision; wording and names are builder's call)*

In `settings.toml`, next to `scrollback_lines`:

```toml
#history_compression = false
#uncompressed_history_lines = 10000
```

In the Settings tab, under the Scrollback row:

- **History compression**: a switch, off by default, with this note under it: `Saves memory on
  very long histories, at the cost of extra CPU when output scrolls into old history and when you
  scroll back, search or export. Recommended for scrollback of 100,000 lines or more.` The
  100,000 figure is `SCROLLBACK_WARN_ABOVE`, so the two notes agree. The copy follows the design
  system: no emoji, no em dashes.
- **Recent lines kept uncompressed**: a number field, default 10,000, disabled while the switch
  is off. It is also disabled, with the note `Needs scrollback above 1,000 lines.`, while
  `scrollback_lines` is 1,000 or less.

### 4.2 Value rules *(builder's call, within the owner's rule)*

- **Default:** when the key is absent, the value is 10,000 or `scrollback_lines - 1`, whichever
  is less, silently (so the shipped defaults, both 10,000, are valid and fire no notice). The
  Settings field shows that effective value. The template line documents it as
  `#uncompressed_history_lines = 10000  # 1000 to scrollback_lines - 1`.
- `uncompressed_history_lines` must be at least 1,000 and less than `scrollback_lines`. The
  floor keeps the screen and the history people usually scroll through uncompressed, so drawing
  near the bottom never decompresses.
- **In the Settings tab:** typing an out-of-range value saves nothing and shows the range
  inline, the way other number fields do.
- **Switching compression on in the Settings tab** with a stored value out of range (kept as
  written while compression is off) clamps it into range and saves it with the switch, so no
  error shows under the field and no notice comes at the next launch.
- **In a hand-edited `settings.toml`:** an out-of-range value is replaced by the nearest valid
  one, and the window banner shows a notice once per distinct line, like the existing
  `scrollback_lines` range notice.
- **Lowering `scrollback_lines` to or below `uncompressed_history_lines`** lowers the latter to
  `scrollback_lines - 1` (never below 1,000). The existing lowering confirmation adds one line
  saying so. If scrollback drops to 1,000 or less, compression has nothing to do. The switch
  keeps its value and the field shows the note above.
- With `scrollback_lines` at 1,000 or less, the switch may still be on, but nothing is
  compressed.

### 4.3 Applying changes *(builder's call)*

- All three changes apply to every open tab's normal screen, like `scrollback_lines` does today:
  switching on, switching off, and changing the uncompressed value. The alternate screen has no
  history and is never affected.
- **Switching on:** compresses the cold blocks already in each tab's history in the background.
- **Switching off:** decompresses them in the background. Memory grows back to today's size.
  That is what the user asked for, so there is no extra confirmation.
- **Raising the uncompressed value:** decompresses blocks that are now inside the recent window.
  **Lowering it:** compresses blocks that are now outside it.
- While the background work runs, the tab is fully usable. Reads of a block that is still
  waiting are simply reads of an uncompressed block.
- **Freeing memory:** when a user action frees memory (switching on, lowering the value), the
  host asks for allocator release through the existing `memory::release_freed()`, as lowering
  `scrollback_lines` already does. Compression of blocks going cold during normal output does
  not call it. The freed memory goes back to the allocator, and steady output reuses it.

## 5. Design

### 5.1 What is compressed

History is already a deque of 64 KiB blocks of compact records, oldest first
(`history.rs`). This design adds a second state to a block:

- **Warm:** today's block. Records are read straight from its bytes.
- **Cold:** the block's bytes replaced by their compressed form, plus the uncompressed length.
  Records keep their entries (block sequence number, offset, length, first extra) unchanged. An
  offset still means "offset in the uncompressed bytes".

A block becomes eligible to go cold when all of its rows are at least `uncompressed` rows away
from the newest history row (rows `i >= uncompressed`, with `i = 0` the newest). In practice
that means up to one block more than the setting stays warm. The newest block is never cold:
new records are only ever appended there. Oversized blocks (a single record larger than a
quarter block) follow the same rule.

A cold block that loses all its live entries (eviction at the limit, `update` moving a record,
`clear`) is freed like a warm one. Cold blocks never become spares, because spares must have a
normal block's capacity.

`update` on a row in a cold block works as today. The row is decoded (decompressing the block,
5.5), changed, and re-encoded into the newest block. The old record becomes dead bytes inside
the cold block, and those are reclaimed when the block's last live entry goes.

`pop_newest` reaching a cold block (a lines-only resize making the screen taller by more than the
warm window) decompresses that block back to warm first. This is rare, because the floor of
1,000 warm rows is larger than any screen.

The extras queue (`Arc<CellExtra>` values) is unchanged. Records refer to extras by position,
never by pointer, so compressing a block never touches them.

### 5.2 The library interface *(builder's call)*

feathercritty's `Config` gains one field:

```rust
/// Compress history blocks older than this many rows (None: never compress).
pub history_compression: Option<usize>,
```

`Term::set_options` applies it to the normal grid's history, like `scrolling_history`. The
library accepts any value, 0 included; the 1,000 floor of 4.2 is fbterm's rule, and tests use
small values to reach cold blocks constantly. With `None`, no code path added by this design
runs. In particular, `push_newest` does no extra work beyond one branch on an `Option`.

Background work is exposed as a small job API on `History`, reached through `Term` (exact names
are the builder's):

- `next_job() -> Option<Job>`: the next block that needs to change state (go cold, or come back
  warm after a setting change), with a copy of its bytes and an identity (sequence number plus a
  generation counter the block carries).
- `Job::run(self) -> Done`: compresses or decompresses the copied bytes. It needs no lock and no
  access to the history.
- `install(done) -> bool`: under the lock, swaps the result in if the block still exists with
  the same identity and is still in the expected state. Otherwise it discards the result and
  returns `false` (the block was evicted, cleared, or a setting changed meanwhile). Install is a
  pointer swap plus bookkeeping: it never compresses, decompresses or copies the block.
- `has_jobs() -> bool`: cheap, so the host can decide whether to schedule anything after a feed.

Copying 64 KiB out under the lock costs a few microseconds. Compression then runs outside the
lock, and only the install takes the lock again, briefly. A block's warm bytes are immutable
once it is no longer the newest block. They only ever lose their last record through
`truncate` while the block is newest. So a copy taken under the lock stays valid until install
checks the identity.

Decompression for reads (5.5) happens under the lock, inside the read. It is bounded to one
block per cache miss.

### 5.3 The compressor *(builder's call, settled by measurement in phase 0)*

Candidates:

- **LZ4** via `lz4_flex` with its safe encode/decode features: pure Rust, very fast in both
  directions, typically 2-3x on text.
- **Zstandard** at a low level: higher ratios, typically 3-6x on text, with fast decompression.
  Either the `zstd` crate (C library) or a pure-Rust implementation.

Phase 0 measures both on corpora built from the large-history test content (short lines,
95-character colored log lines, full-width lines) and on recorded real streams (a build log, a
long `journalctl`-style log, `git log --graph` output). It records ratio, compression and
decompression MB/s per 64 KiB block, and the dependency's license and audit status.

Selection rule:

1. Meet the memory gate (6.3).
2. Among candidates that do, prefer the one whose per-block decompression is fastest.
3. Prefer pure Rust when it is within 20% on both ratio and speed.

The choice, the level and the measurements go in the README entry and in `benches/RESULTS.md`.
Both repositories' `cargo deny` must pass with the new dependency.

The compressed form of a block is the compressor's raw output plus a small header (format tag,
uncompressed length), so a later change of compressor can read old blocks if disk spill ever
needs that. Today nothing outlives the process, so no migration is ever needed.

### 5.4 Background work in fbterm *(builder's call)*

- **One shared worker thread** for the whole app (`fbterm-history-worker`, low priority where the
  platform allows: `nice` on Unix, a below-normal thread priority on Windows), not one per tab.
  It keeps a queue of terminals that have jobs.
- **Feeding the queue:**
  - After each `Feeder::feed` batch (`crates/fbterm-session/src/lib.rs`, used by local and SSH
    sessions alike), the session checks `has_jobs()` while it still holds the lock, and if true
    enqueues its terminal. The check is one comparison.
  - A setting change enqueues every open terminal.
- **Working the queue:** the worker takes one job at a time. It locks the terminal to call
  `next_job`, unlocks, runs the job, locks again to `install`, unlocks, then moves on to the next
  job of the same terminal or of another one (round robin, so one huge tab doesn't starve the
  others).
- **Tab lifetime:** closing a tab drops its terminal. The worker holds a weak reference, so a
  closed tab's jobs are skipped.
- **When work runs:** there is no periodic tick. The worker sleeps on its queue and only wakes
  when a feed or a setting change enqueues work, which follows the redraw contract's spirit.
- **Hidden tabs:** they keep parsing (background sessions never stop parsing in fbterm), so their blocks keep going
  cold as output arrives. That is intended, since hidden long-running tabs are the main
  beneficiaries.

### 5.5 Reading cold blocks *(builder's call)*

Every read of a history row ends at `History::record(entry)`. With cold blocks, it needs the
block's warm bytes. A small cache of decompressed blocks provides them:

- **Size:** at most 4 blocks per history (256 KiB), least recently used evicted.
  - A scrolled-back screen of up to 200 lines touches at most 2 blocks of typical content.
  - A sequential sweep (search, export, reflow) touches each block once, in order.
- **Ownership:** the cache belongs to the `History`. Reads take `&self` today, so the cache needs
  interior mutability. Use a `RefCell` (or a `Cell`-based scheme): `Term` is only ever used
  under fbterm's lock, `History` stays `Send`, and nothing requires it to be `Sync`. If a
  requirement for `Sync` turns up, the builder records it and uses a `Mutex` with no contention
  instead. No `unsafe` either way.
- **Header-only questions:** `wraps(i)` and `is_clear(i)` are asked for many rows during drawing
  and selection. They read only the record header. Two bits in the 16-byte `Entry` cache them,
  taken from `len` (records are far below 1 GiB), so these questions never decompress. The same
  goes for `oldest_reflow_info` and `newest_header` if measurements show they need it.
- **`plain_text`** (search's fast path) reads through the cache like any other record read.
- **Freeing:** the cache is cleared when the history is cleared or its limit lowered, and when
  compression is switched off (everything becomes warm anyway). Its memory counts in the
  gates.

### 5.6 Resize and reflow

Reflow (`resize.rs`) pops the old history oldest first and builds a new history.

- **During reflow:** *(revised after the builder check)* with compression on, the new history is built **warm**, and its
  cold blocks are left for the worker to compress afterwards, as jobs like any others.
  Compressing inline would hold the lock for several times today's reflow time at 1,000,000
  lines. Reflow still has to decompress each old cold block once, sequentially, which is the
  unavoidable cost. Memory: old cold blocks are freed as their last row leaves while new warm
  blocks fill, so the peak stays at or below today's uncompressed size, never a dense copy and
  never above what the same history costs with compression off. The worker brings it back down
  after the resize.
- **Old history:** freed cold block by cold block as its last row leaves, as today.
- **Byte-copy fast path:** `move_oldest_to` reads the record through the cache. It continues to
  work as today on the warm bytes.
- **Lines-only resize:** works as today. Growth that pops into a cold block warms it (5.1).

### 5.7 Who sees what

- **Unchanged:** fbterm's search, export, selection, the find bar's `StablePoint`s and drawing.
  None of them change: they read through `RowReader`, `Grid::cell`, `Grid::wraps` and
  `Grid::is_clear` as today.
- **Wiring changes:** only the session feed hook, the worker, the settings plumbing and the
  Settings rows.
- **Library merges:** `history.rs` gains the cold state, the cache and the job API. Nothing
  upstream corresponds to it, so upstream merges are unaffected. The README change list gets an
  entry, and the module docs get a section on cold blocks (invariants, eligibility, the job
  identity rule).

## 6. Acceptance gates

All gates are measured in release builds on the owner's Apple M5 Max (macOS) unless stated, with
medians of at least 5 runs and the spread reported.

### 6.1 Correctness

- **Differential test** (`tests/differential.rs`): it gains passes with compression on:
  - uncompressed values 0, 1 and 37;
  - tiny blocks;
  - jobs run at random points between steps, sometimes run but not installed, sometimes
    installed after an intervening eviction, clear, limit change or setting change.

  It compares against unmodified 0.26.0 exactly as today. CI: the same seeds and step counts as
  the existing passes, on Linux, macOS and Windows. Pass: zero differences.
- **Unit tests in `history.rs`:**
  - a stale job is refused after each kind of interference;
  - `update` and `pop_newest` on cold blocks;
  - the cache's eviction order;
  - header bits agree with decoding for every row after compression;
  - every block's state agrees with eligibility after any sequence of setting changes once all
    jobs are run.
- **Large-history test** (`large_history`, `#[ignore]`, release): the same checks run with
  compression on at 1,000,000 lines.
- **Synchronous mode for tests:** the library exposes `Term::run_history_jobs()` (name the
  builder's), which runs and installs every pending job inline. It is ordinary public API, not an
  environment switch, so "off means off" holds. The differential passes call it at random points.
- **Existing tests:** every existing test in both repositories passes unchanged with the switch
  off (the default). Tests that read history in fbterm (the golden outputs for export, search,
  find-in-all-tabs and selection text, and the draw-list tests that scroll into history) gain a
  variant with compression on, a small uncompressed value and `run_history_jobs()` before the
  read. Pass: byte-identical to the variant with compression off.

### 6.2 Off means off

- **Ingest:** the benchmarks of compact-history 6.3, plus the large-history test, with
  `history_compression: None`, against the current fork `rev`. Pass: within 1% on every
  workload.
- **Memory:** live bytes are identical.
- **Allocations:** the counting-allocator check still shows zero allocations per scrolled row at
  a full history.

### 6.3 Memory (library)

*(Revised after the builder check.)* The existing large-history content
(`tests/large_history.rs`) is uniform random letters and digits, which no compressor can shrink
much, so it can't carry the gate. Phase 0 adds realistic generators, deterministic and committed
with the tests:

- **build log:** compiler-style lines with paths, warnings and repeated prefixes;
- **service log:** timestamped lines with levels, colored, and recurring message templates with
  varying ids and numbers;
- **shell session:** prompts, short commands, `ls`-style colored listings.

It also adds two recorded real streams kept as test data (a cargo build of this workspace and
`git log --graph --color` of this repository).

Live bytes of a `Grid` at 1,000,000 history lines, 126 columns, uncompressed value 10,000,
after every job has run, measured against the same content with compression off:

| Content | Pass |
|---|---|
| each realistic generator and each recorded stream | at most 40% of off |
| random content (`large_history.rs` today) | never above off; recorded |

The 16-byte index entry per row (16 MB at 1,000,000 rows) and the fixed per-terminal overhead
stay uncompressed and count in the totals. If phase 0 shows that the index dominates what is
left, a smaller index for cold blocks becomes a follow-up, not part of this work.

Also recorded, not gated: the same at 100,000 lines, and the cache's share.

### 6.4 Speed with compression on

| Measure | Pass |
|---|---|
| Parser-side ingest at a 1,000,000-line history, compression on and the worker running (benchmark of compact-history 6.3, log-line workload) | at most 3% slower than off |
| Longest single lock hold by the worker (`next_job` plus copy, or `install`) | at most 0.1 ms (p99 over a 1,000,000-line fill) |
| Worker CPU to keep up with a sustained log-line flood | recorded; it must keep up (the job backlog stays bounded over a 60 s flood) |
| fbterm draw-list build with the view entirely in cold history, first frame (cache cold) | at most 2 ms |
| Same, following frames (cache warm) | at most 1.15x the same content warm |
| fbterm search across a 1,000,000-line log history, no match | at most 2x off |
| fbterm export of a 1,000,000-line history | at most 2x off |
| Resize with reflow at 1,000,000 lines (126 to 100 to 126 columns), realistic content | at most off plus the time to decompress the old history once (measured), with 1 s as a hard ceiling per pass; peak live bytes never above off |
| Switching on for a 1,000,000-line tab | lock holds as above; total time recorded; memory never above the uncompressed size |
| Switching off for a 1,000,000-line tab | lock holds as above; total time recorded |

### 6.5 fbterm

- **Unit and app tests**, using a temporary config directory and in-memory secrets:
  - the settings' defaults, template lines and range notices;
  - lowering `scrollback_lines` drags the uncompressed value down, and the confirmation says so;
  - the field is disabled with the switch off, and with scrollback at 1,000 or less;
  - changes reach every open terminal's normal screen and never the alternate screen's;
  - a user-initiated switch-on requests `release_freed()`, while output-driven compression never
    does;
  - the worker skips a closed tab's jobs.
- **Measurement scenario** in `dev/perf/sessions.sh` (scenario I): one tab with
  `scrollback_lines = 1000000` filled with log lines, RSS and footprint with compression off and
  on (after the backlog drains), plus CPU over 10 s while idle afterwards. Pass: idle CPU 0.00%
  with compression on, so the worker sleeps. RSS is recorded.
- **Screenshots** of the Settings rows (switch off, switch on, the disabled-field notes), taken
  with `FBTERM_SCREENSHOT_WINDOW=settings` and the usual throwaway config, are attached to the
  PR.

### 6.6 Done means

All of 6.1 to 6.5 pass. The results are in the feathercritty PR, in its `benches/RESULTS.md` and
in `dev/perf/README.md`. The parent spec's sections 6 and 7 note the feature. The docs site
lists both settings in `website/docs/files.md`, and the Settings page docs describe the
trade-off.

## 7. Risks

| Risk | Mitigation |
|---|---|
| A stale job installs over a block that changed | Identity = sequence number plus generation, checked on install; fuzzed in the differential test with interference between run and install |
| First entry into cold history hitches | Floor of 1,000 warm rows; per-block decompression chosen fastest among candidates meeting the memory gate; first-frame gate in 6.4 |
| Search and export over cold history get slow | Sequential reads touch each block once; gates in 6.4 |
| The worker falls behind a flood, so memory grows past the target | Backlog-bounded gate in 6.4; jobs are cheap per block; at worst blocks stay warm (today's cost), never wrong |
| Cache memory adds up across many tabs | 256 KiB per history at most, only while cold blocks are read; cleared with the history |
| A dependency brings advisories or license issues | `cargo deny` in both repositories; selection rule prefers pure Rust |
| Upstream merges get harder | All changes in `history.rs` plus one `Config` field; no upstream counterpart |

## 8. Delivery

Branches: `feature/history-compression` in feathercritty (merged into its `main` through a PR
with green CI) and in fbterm off `develop`. fbterm points at the working copy with the
uncommitted `[patch]` entry during development, then bumps `rev` per `CLAUDE.md`.

1. **Phase 0, measurement (no behavior change).** Compressor corpora and the comparison of 5.3,
   baseline numbers for 6.2 to 6.4 with compression off, and the choice recorded.
2. **Phase 1, library.** Cold state, job API, read cache, header bits, reflow integration;
   differential and unit tests; gates 6.1 to 6.4 at library level.
3. **Phase 2, fbterm.** Worker thread, feed hook, settings, Settings rows and copy, docs; gates
   6.4 (fbterm rows) and 6.5.

## 8a. What's left to the builder

- Exact names in the library API and in fbterm.
- The worker's queue structure and wake mechanism, within 5.4's rules.
- Whether `oldest_reflow_info` and `newest_header` need cached header data, decided by
  measurement.
- The exact layout of the compressed block header.
