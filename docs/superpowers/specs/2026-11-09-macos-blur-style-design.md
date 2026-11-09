# macOS blur style: design

Status: draft for the owner's review, 2026-11-09. Written while the owner was away, from their
request; the calls marked **(call)** are the author's and are open to change.

## 1. Intent

Since 0.8.3, a blurred fbterm window on macOS blurs through the system's blur view
(`NSVisualEffectView`, behind-window blending, under-window-background material) instead of the
window library's private window-server blur, because the private call kept the pre-macOS-26
window shape and shrank the corners (`CLAUDE.md`, `chrome::system_backdrop`). The owner finds the
transparency looks different now: the system view adds a light system tint over what shows
through, where the old blur was a clear, fixed-strength blur. They want users on macOS to choose:

- **Native**: the system's own look, as now.
- **Classic**: fbterm's earlier, clearer look, recreated without the private call, so the
  corners stay right.

## 2. Goals

1. A setting `macos_blur_style = "native" | "classic"`, default `"native"` **(call: keeps the
   shipped look for everyone who does not choose)**.
2. A Settings row on macOS only, under Background blur: a two-option segmented control
   ("Native", "Classic"), disabled with the same rule as the blur switch (blur off or opacity
   100%), with a short note.
3. Classic is tuned to look as close as measurable to the old private blur, using only the
   system blur view's public properties (material, appearance, state, alpha) and the window's
   own drawing.
4. Changing the style applies to open windows (re-tune each window's blur view in place)
   **(call: if the view cannot be re-tuned live, fall back to the existing "applies to new windows
   after a restart" note that blur already uses)**.

## 3. Non-goals and anti-goals

- No change on Linux or Windows; the row is not shown there.
- No return of the private window-server blur call on macOS, for either style (the corner bug).
  The existing test that macOS windows never ask the window library for blur stays and covers
  both styles.
- No new private API. Public AppKit only.
- Anti-goals: corners that differ from the system's current shape in either style; a style that
  changes text contrast so much that the four built-in schemes become hard to read.

## 4. How classic is tuned

The difference has two likely sources: the material's tint, and the view's appearance (the tint
follows the effective appearance, so a light system appearance under a dark scheme washes the
background out). Classic therefore:

- sets the blur view's appearance from the active scheme's variant (dark scheme → dark aqua,
  light → aqua), and updates it when the scheme changes;
- uses the material and alpha that come closest to the old blur by measurement.

**No measurement by screen capture.** The repo's rule for agents is own-window screenshots only,
never full-screen capture, and an own-window screenshot does not include what the blur shows
through. So classic is chosen by reasoning **(call)**: the least-tinted public material that still
blurs behind the window, appearance from the scheme's variant, and alpha only if needed. The owner
judges it on a real desktop. To make that quick, a debug-only aid
`FBTERM_DEMO_BLUR=<material>[:<alpha>]` (debug builds only, like the other `FBTERM_DEMO_*` aids,
documented in CLAUDE.md) overrides classic's material and alpha, so the owner can try candidates
without rebuilding; the builder lists the candidate materials worth trying in the PR text.

## 5. Copy

- Row label: `Blur style`
- Options: `Native`, `Classic`
- Note: `Native uses the macOS look. Classic is fbterm's earlier, clearer blur.`
- Template line: `#macos_blur_style = "native"  # or "classic"; macOS only`
- Docs: one row in `website/docs/files.md`, and the blur paragraph wherever the docs describe
  background blur.

## 6. Tests

- Settings: default native; `"classic"` parses; an unknown value falls back to native with a
  notice naming the key (existing validate pattern); template line present.
- Settings window: row present only on macOS (cfg), disabled with blur off or opacity 100%,
  selecting a style saves it.
- Chrome: a pure function maps (style, scheme variant) to the blur view's parameters; unit tests
  pin native = today's parameters exactly, classic = the chosen parameters, and the debug override parses; the existing
  "macOS never asks the window library for blur" test covers both styles.
- Screenshots of the Settings row (throwaway config).

## 7. Left to the builder

Where the per-window blur view handle is kept for live re-tuning; the exact helper names; whether
alpha is needed at all once appearance and material are right.

## 8. Owner's clarification (2026-11-09)

The real complaint: with the system blur view, the `background_opacity` setting makes very little
visible difference, because the view paints its own material fill (tint) over the blurred backdrop
regardless of how transparent the terminal background is. The old blur had no fill, so the
terminal background at its opacity was the only tint and the opacity setting clearly changed the
look. **Classic's goal is therefore: a blur with no material fill of its own, so the opacity setting
drives the look as it did before 0.8.3, with the current corners.** Approach, in order:

1. Keep the blur view, and hide its fill/tint sublayers, keeping only the backdrop (blur) layer:
   walk the view's layer tree with public CALayer API, identify the backdrop layer by its blur
   filter, and hide the others; re-apply after the view relayouts or changes appearance. If the
   tree does not have the expected shape (a future macOS), leave the view untouched (fall back to
   step 2) and log once.
2. Fallback: the material whose layer tree carries the least fill.

Measure instead of guessing: a debug-only dump of each material's layer tree (layer class, opacity,
background color, filters, compositing filter) under both appearances, run once by the builder;
record the findings in `dev/blur/README.md` and choose from them. The owner judges the final look
on a real desktop.
