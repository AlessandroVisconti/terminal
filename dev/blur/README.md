# macOS blur style: layers and materials

The `macos_blur_style` setting (spec `docs/superpowers/specs/2026-11-09-macos-blur-style-design.md`,
section 8 for the owner's clarification) picks how each window's blur view (`NSVisualEffectView`,
behind-window blending, always active) is drawn, in `chrome::blur_look`:

| Style | View | Material | Fill | Appearance |
|---|---|---|---|---|
| `native` | the plain system view, set up as 0.8.3 did | under-window-background (21) | the system's | the window's own |
| `classic` | `FbtermClearBlurView` (`blur_layers.rs`) | titlebar (3) | hidden: only the blurred backdrop stays | dark aqua or aqua from the scheme of the active tab's first pane |

Both keep the window's own corners: the view sits inside the frame view, under the window's
corner mask, and no style asks the window library for its blur (the private window-server call
that shrank the corners on macOS 26+).

## The complaint, and the measurement

With the system view, `background_opacity` makes little visible difference: the view paints its
own fill over the blur, whatever the terminal background's opacity. The old blur had no fill, so
the terminal background at its opacity was the only tint. Classic restores that.

`FBTERM_DEMO_BLUR_DUMP=1` (debug builds) logs, once, from the first blurred window: the layer tree
the view builds for every material in both appearances (class, name, hidden, opacity, background
color, filters, compositing filter), then the tree as used. Measured on macOS 27.2 (SDK 27.2):

Every blurring material builds the same shape: the view's layer, one layer named after the
material, and under it four layers:

1. `CABackdropLayer` "backdrop": the blur. Filters `sdrNormalize`, `gaussianBlur` and, for most
   materials, `colorSaturate`; the deprecated materials use `blur` and `saturate`, and some light
   variants name it `gaussianBlur 2`.
2. `CALayer` "fill": a flat color over the blur. This is the tint.
3. `CALayer` "tone": an opaque gray with a lighten (dark) or darken (light) blend mode.
4. `CAChameleonLayer` "desktop tint" or "Chameleon": the desktop-color tint at 5% opacity.

| Material | Backdrop filters | Fill, dark | Fill, light | Tone, dark / light |
|---|---|---|---|---|
| titlebar | blur only, no saturation | gray 0.24, alpha 1, layer 80% | 0.99, alpha 1, layer 80% | none |
| header-view | blur only, no saturation | 0.12, alpha 1, layer 80% | white, alpha 1, layer 80% | none |
| hud-window | blur + saturation | 0.16 at 40% | 0.96 at 48% | 0.08 lighten / 0.96 darken |
| full-screen-ui | blur + saturation | 0.16 at 50% | 0.96 at 48% | 0.095 lighten / 0.96 darken |
| popover | blur + saturation | 0.16 at 60% | 0.96 at 60% | 0.11 / 0.945 |
| menu | blur + saturation | 0.16 at 70% | 0.96 at 72% | 0.125 / 0.93 |
| sidebar, tool-tip, under-window-background (native), appearance-based | blur + saturation | 0.16 at 80% | 0.96 at 84% | 0.14 / 0.915 |
| light, medium-light, dark, ultra-dark (deprecated) | `blur` + `saturate` | white 30% / 40%, 0.12 at 45% / 80% (same in both appearances) | same | 0.94 / 0.84 darken, 0.16 / 0.10 lighten |
| sheet | none: an opaque fill | 0.12 at 90% | white | none |
| window-background, content-background, under-page-background | none: a SwiftUI hosting layer with an opaque color | opaque | opaque | none |
| selection | none: a vibrant color-matrix layer | | | |

So native (under-window-background) lays an 80% gray fill over the blur, and the terminal's
opacity changes only what is left above it.

## What classic does

- **The fill hidden.** `blur_layers::layers_to_hide` (pure, unit-tested on the measured trees)
  finds the one layer whose filters include a blur, by filter, not by class name, and hides every
  other layer that is neither it nor one of its ancestors: fill, tone and tint. The view's own
  layer and the material's layer stay. `FbtermClearBlurView` (an `objc2` subclass of the system
  view) hides them again after `layout`, `updateLayer`, `viewDidChangeEffectiveAppearance`,
  `viewDidChangeBackingProperties` and `viewDidMoveToWindow`, so a resize, an appearance change, a
  move to another display or a re-tune that rebuilds the layers cannot bring the fill back.
- **Titlebar material.** With the fill hidden, what remains is the backdrop alone. Titlebar's
  backdrop (like header-view's) is the only one without a saturation filter, so it is a plain blur,
  the closest to the old one.
- **Appearance** from the scheme of the active tab's first pane. With the fill hidden it changes
  nothing visible on titlebar; it matters for the fallback.
- **Fallback.** If the tree has no blurring layer, or more than one (a future macOS), classic leaves
  the layers as drawn, switches to hud-window (the lightest fill measured: 40% dark, 48% light, and
  a tone that barely changes anything) and logs once, naming the classes it found. A view whose
  layers are not built yet is not a fallback case: its hooks clear it once they are built.

Switching between the styles swaps the view (plain for native, classic for classic) in place, so
native stays exactly the 0.8.3 view.

## Trying other materials

With a debug build and `macos_blur_style = "classic"`, `FBTERM_DEMO_BLUR=<material>[+fill][:<alpha>]`
replaces classic's material and alpha, keeping the fill hidden unless `+fill` is given:

```
FBTERM_CONFIG_DIR=<dir> FBTERM_SECRETS=memory FBTERM_DEMO_BLUR=hud-window target/debug/fbterm
```

(with `background_opacity = 0.8`, `background_blur = true` and `macos_blur_style = "classic"` in
`<dir>/settings.toml`). Worth comparing:

- `titlebar` (the default) and `header-view`: plain blur, no saturation.
- `hud-window`, `full-screen-ui`, `under-window-background`: blur with added saturation (more vivid
  colors behind the window). Their blur radius may differ too.
- `hud-window+fill`: the fallback as it would look.
- `under-window-background+fill`: native's material and fill, in the scheme's appearance.
- any of them with `:0.85` or lower: a partly transparent view, letting the sharp desktop through.

If another material wins on a real desktop, change `BlurLook::CLASSIC_MATERIAL` in
`crates/fbterm/src/chrome.rs` and its test.
