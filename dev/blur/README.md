# macOS blur style: materials

The `macos_blur_style` setting (spec `docs/superpowers/specs/2026-11-09-macos-blur-style-design.md`)
picks the parameters of each window's blur view (`NSVisualEffectView`, behind-window blending,
always active), in `chrome::blur_look`:

| Style | Material | Appearance | Alpha |
|---|---|---|---|
| `native` | under-window-background (21) | the window's own (follows the system) | 1.0 |
| `classic` | full-screen UI (15) | dark aqua for a dark scheme, aqua for a light one | 1.0 |

`native` is exactly the view 0.8.3 shipped. Both styles keep the window's own corners: the view
sits inside the frame view, under the window's corner mask, and no style asks the window library
for its blur (the private window-server call that shrank the corners on macOS 26+).

## Why classic is what it is

Before 0.8.3, blur came from the window server's own background blur (radius 80): a plain blur
with no tint and no added saturation. A blur view always adds some of both, and the amount
depends on the material and on the appearance it is drawn in. Two things made the native look
differ:

- **The material.** Under-window-background is meant to sit behind a whole window's content, so
  it lays a strong system wash over what shows through.
- **The appearance.** A material's tint follows the view's effective appearance, which follows the
  system's light or dark mode, not the terminal's scheme: a light system under a dark scheme
  washes the background out to a pale gray.

Classic therefore fixes the appearance to the variant of the scheme the window shows
(`fbterm_config::schemes::is_dark`: the background contrasts more with white than with black,
taken from the active tab's focused pane through `App::scheme_for`), and uses the least tinted
public material that still blurs behind the window. That pick was made by reasoning, not
measurement: agents may only capture fbterm's own window, and such a capture does not include what
the blur shows through. The owner judges it on a real desktop.

Alpha stays 1.0: lowering it mixes the unblurred desktop back in (sharper, but also less blur),
which is a different look rather than a clearer blur. Try it with the override below if wanted.

## Candidates

With a debug build and `macos_blur_style = "classic"`, `FBTERM_DEMO_BLUR=<material>[:<alpha>]`
replaces classic's material and alpha (alpha above 0, at most 1), keeping the scheme's
appearance. For example:

```
FBTERM_CONFIG_DIR=$(mktemp -d) FBTERM_SECRETS=memory FBTERM_DEMO_BLUR=sidebar cargo run -p fbterm
```

(with `background_opacity = 0.8`, `background_blur = true` and `macos_blur_style = "classic"` in
that directory's `settings.toml`). Worth comparing, roughly from least to most tinted:

| Name | Expected look |
|---|---|
| `full-screen-ui` | the default: light tint, close to a plain blur |
| `titlebar` | light tint, slightly more than full-screen UI |
| `sidebar` | light to medium tint; in a dark appearance close to full-screen UI |
| `header-view` | medium tint |
| `content-background` | medium tint |
| `window-background` | strong wash (close to native) |
| `under-window-background` | native's material, but in the scheme's appearance |
| `hud-window` | dark gray in either appearance |
| `popover`, `menu`, `sheet`, `tool-tip`, `selection`, `under-page-background` | strong tints, listed for completeness |

The older materials AppKit has deprecated but still draws are accepted too, for comparison only:
`appearance-based`, `light`, `dark`, `medium-light`, `ultra-dark`. Adding `:0.85` (or lower) to
any name shows what a partly transparent view looks like.

If another material wins on a real desktop, change `BlurLook::CLASSIC_MATERIAL` in
`crates/fbterm/src/chrome.rs` and its test.
