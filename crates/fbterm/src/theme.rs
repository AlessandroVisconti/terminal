//! Design system v2 "Graphite" as an iced theme, plus the tokens the chrome draws with.

use std::sync::Mutex;

use fbterm_config::schemes::{self, SchemeColors};
use iced::theme::{Palette, palette};
use iced::{Color, Font, Theme, color};

use crate::tab_look::{contrast, mix};
use crate::ui::to_color;

/// UI text face (bundled).
pub const UI_FONT: Font = Font::with_name("Geist");

/// Values the chrome uses directly, beyond what an iced palette carries.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Tokens {
    pub app: Color,
    pub surface: Color,
    pub raised: Color,
    pub border: Color,
    pub hover: Color,
    pub text: Color,
    pub text_secondary: Color,
    pub text_muted: Color,
    pub accent: Color,
    pub accent_soft: Color,
    /// Edge of the selected row (a lighter violet on dark).
    pub accent_edge: Color,
    pub success: Color,
    pub warning: Color,
    pub danger: Color,
    pub danger_soft: Color,
    /// The terminal's default background (the Graphite color scheme's, in both modes).
    pub terminal: Color,
}

pub const DARK: Tokens = Tokens {
    app: color!(0x101013),
    surface: color!(0x151518),
    raised: color!(0x1f1f22),
    border: color!(0x313135),
    hover: Color::from_rgba(1.0, 1.0, 1.0, 0.045),
    text: color!(0xf5f5f7),
    text_secondary: color!(0xb6b6bf),
    text_muted: color!(0x919199),
    accent: color!(0x7c49e3),
    accent_soft: Color::from_rgba(165.0 / 255.0, 132.0 / 255.0, 1.0, 0.14),
    accent_edge: color!(0xa584ff),
    success: color!(0x2bbb71),
    warning: color!(0xf2a635),
    danger: color!(0xed4a49),
    danger_soft: Color::from_rgba(237.0 / 255.0, 74.0 / 255.0, 73.0 / 255.0, 0.14),
    terminal: color!(0x0b0b0e),
};

pub const LIGHT: Tokens = Tokens {
    app: color!(0xfafafb),
    surface: color!(0xfdfdfe),
    raised: color!(0xffffff),
    border: color!(0xdadadf),
    hover: Color::from_rgba(28.0 / 255.0, 28.0 / 255.0, 34.0 / 255.0, 0.04),
    text: color!(0x1c1c22),
    text_secondary: color!(0x4f4f56),
    text_muted: color!(0x686870),
    accent: color!(0x7c49e3),
    accent_soft: Color::from_rgba(124.0 / 255.0, 73.0 / 255.0, 227.0 / 255.0, 0.09),
    accent_edge: color!(0x7c49e3),
    success: color!(0x00884b),
    warning: color!(0xbe7100),
    danger: color!(0xc52b30),
    danger_soft: Color::from_rgba(197.0 / 255.0, 43.0 / 255.0, 48.0 / 255.0, 0.08),
    terminal: color!(0x0b0b0e),
};

/// The iced theme for a token set: the same extended-palette overrides for every scheme.
pub fn chrome_theme(t: &Tokens) -> Theme {
    theme_named("Chrome", t)
}

fn theme_named(name: &str, t: &Tokens) -> Theme {
    let t = *t;
    let base = Palette {
        background: t.app,
        text: t.text,
        primary: t.accent,
        success: t.success,
        warning: t.warning,
        danger: t.danger,
    };
    // Text on the accent: white or the Graphite app color, whichever reads better.
    let ink = color!(0x101013);
    let on_accent = if contrast(Color::WHITE, t.accent) >= contrast(ink, t.accent) {
        Color::WHITE
    } else {
        ink
    };
    Theme::custom_with_fn(name.to_string(), base, move |p| {
        let mut ext = palette::Extended::generate(p);
        ext.background.weak = palette::Pair::new(t.surface, t.text);
        ext.background.neutral = palette::Pair::new(t.raised, t.text);
        ext.background.strong = palette::Pair::new(t.border, t.text);
        // A literal pair: `Pair::new` would nudge the text color away from the exact choice.
        ext.primary.base = palette::Pair {
            color: t.accent,
            text: on_accent,
        };
        ext
    })
}

// Fractions of the way from the scheme background to its foreground. Tuned so that Graphite
// derives within 0.03 per channel of `DARK` (see the `graphite_derives_close_to_dark` test).
const APP_STEP: f32 = 0.02;
const SURFACE_STEP: f32 = 0.045;
const RAISED_STEP: f32 = 0.085;
const BORDER_STEP: f32 = 0.165;
const SECONDARY_STEP: f32 = 0.73;
const MUTED_STEP: f32 = 0.575;

const VIOLET: Color = color!(0x7c49e3);

/// `t` of the way from `from` to `to`, opaque.
fn toward(from: Color, to: Color, t: f32) -> Color {
    mix(to, from, t)
}

/// The extreme (white or black) that contrasts most with `bg`.
fn far_end(bg: Color) -> Color {
    if contrast(Color::WHITE, bg) >= contrast(Color::BLACK, bg) {
        Color::WHITE
    } else {
        Color::BLACK
    }
}

/// `c`, moved toward the extreme that contrasts most with `bg` until it meets `min`.
fn ensure(c: Color, bg: Color, min: f32) -> Color {
    if contrast(c, bg) >= min {
        return c;
    }
    let end = far_end(bg);
    (1..=50)
        .map(|i| toward(c, end, i as f32 / 50.0))
        .find(|m| contrast(*m, bg) >= min)
        .unwrap_or(end)
}

/// `bg`, moved away from the extreme it is nearest until text can reach 7:1 on it. A no-op
/// for any scheme whose background already allows that.
fn roomy(bg: Color) -> Color {
    let end = far_end(bg);
    let other = if end == Color::WHITE {
        Color::BLACK
    } else {
        Color::WHITE
    };
    if contrast(end, bg) >= 7.0 {
        return bg;
    }
    (1..=100)
        .map(|i| toward(bg, other, i as f32 / 100.0))
        .find(|m| contrast(far_end(*m), *m) >= 7.0)
        .unwrap_or(other)
}

fn with_alpha(c: Color, a: f32) -> Color {
    Color { a, ..c }
}

/// The chrome tokens of a scheme, derived from its colors alone. Pure; see [`tokens_for`] for
/// the interned form the app uses.
pub fn derive(colors: &SchemeColors) -> Tokens {
    let dark = schemes::is_dark(colors);
    let bg = roomy(to_color(colors.background));
    let fg = to_color(colors.foreground);
    let app = roomy(toward(bg, fg, APP_STEP));
    let neutral = |step: f32| toward(bg, fg, step);

    let text = ensure(fg, app, 7.0);
    let text_secondary = ensure(neutral(SECONDARY_STEP), app, 4.5);
    let text_muted = ensure(neutral(MUTED_STEP), app, 4.5);

    let chromatic = |c: Color| c.r.max(c.g).max(c.b) - c.r.min(c.g).min(c.b) >= 0.12;
    let cursor = to_color(colors.cursor);
    let accent = [
        Some(cursor).filter(|c| chromatic(*c)),
        Some(to_color(colors.ansi[4])),
        Some(to_color(colors.ansi[12])),
    ]
    .into_iter()
    .flatten()
    .find(|c| contrast(*c, app) >= 3.0)
    .unwrap_or_else(|| ensure(VIOLET, app, 3.0));

    let reference = if dark { &DARK } else { &LIGHT };
    let status = |i: usize, fallback: Color| {
        let c = to_color(colors.ansi[i]);
        let c = if contrast(c, app) >= 3.0 { c } else { fallback };
        ensure(c, app, 3.0)
    };
    let success = status(2, reference.success);
    let warning = status(3, reference.warning);
    let danger = status(1, reference.danger);

    Tokens {
        app,
        surface: neutral(SURFACE_STEP),
        raised: neutral(RAISED_STEP),
        border: neutral(BORDER_STEP),
        hover: with_alpha(fg, 0.045),
        text,
        text_secondary,
        text_muted,
        accent,
        accent_soft: with_alpha(accent, if dark { 0.14 } else { 0.09 }),
        accent_edge: if dark {
            toward(accent, Color::WHITE, 0.3)
        } else {
            accent
        },
        success,
        warning,
        danger,
        danger_soft: with_alpha(danger, if dark { 0.14 } else { 0.08 }),
        terminal: to_color(colors.background),
    }
}

/// Process-wide interning of derived token sets, so callers can hold `&'static Tokens`.
/// Bounded by the number of distinct color sets seen in one run.
static INTERNED: Mutex<Vec<&'static Tokens>> = Mutex::new(Vec::new());

/// The chrome tokens for a scheme. `graphite` and `graphite-light` give [`DARK`] and
/// [`LIGHT`] value for value (with `terminal` set to the scheme's background); every other
/// scheme gets [`derive`]d tokens. Equal results are the same `&'static` value.
pub fn tokens_for(id: &str, colors: &SchemeColors) -> &'static Tokens {
    let tokens = match id {
        "graphite" => Tokens {
            terminal: to_color(colors.background),
            ..DARK
        },
        "graphite-light" => Tokens {
            terminal: to_color(colors.background),
            ..LIGHT
        },
        _ => derive(colors),
    };
    let mut interned = INTERNED.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(found) = interned.iter().find(|t| ***t == tokens) {
        return found;
    }
    let leaked: &'static Tokens = Box::leak(Box::new(tokens));
    interned.push(leaked);
    leaked
}

/// The tokens of the built-in Graphite scheme, for tests that draw chrome without an app.
#[cfg(test)]
pub fn graphite_tokens() -> &'static Tokens {
    let graphite = schemes::builtins()
        .into_iter()
        .find(|s| s.id == "graphite")
        .expect("Graphite is built in");
    tokens_for("graphite", &graphite.colors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dark_palette_uses_graphite_values() {
        let p = chrome_theme(&DARK).palette();
        assert_eq!(p.background, color!(0x101013));
        assert_eq!(p.text, color!(0xf5f5f7));
        assert_eq!(p.primary, color!(0x7c49e3));
        assert_eq!(p.success, color!(0x2bbb71));
        assert_eq!(p.danger, color!(0xed4a49));
    }

    fn builtin(id: &str) -> fbterm_config::schemes::Scheme {
        fbterm_config::schemes::builtins()
            .into_iter()
            .find(|s| s.id == id)
            .unwrap()
    }

    fn rgb(c: fbterm_config::profile::Rgb) -> Color {
        Color::from_rgb8(c.r, c.g, c.b)
    }

    fn gray(v: u8) -> fbterm_config::profile::Rgb {
        fbterm_config::profile::Rgb { r: v, g: v, b: v }
    }

    fn hex(v: u32) -> fbterm_config::profile::Rgb {
        fbterm_config::profile::Rgb {
            r: (v >> 16) as u8,
            g: (v >> 8) as u8,
            b: v as u8,
        }
    }

    #[test]
    fn graphite_maps_to_dark_exactly() {
        let g = builtin("graphite");
        let want = Tokens {
            terminal: rgb(g.colors.background),
            ..DARK
        };
        assert_eq!(*tokens_for("graphite", &g.colors), want);
        assert_eq!(want.terminal, color!(0x0b0b0e));
    }

    #[test]
    fn graphite_light_maps_to_light_with_its_own_terminal() {
        let g = builtin("graphite-light");
        let want = Tokens {
            terminal: rgb(g.colors.background),
            ..LIGHT
        };
        assert_eq!(*tokens_for("graphite-light", &g.colors), want);
        assert_ne!(want.terminal, color!(0x0b0b0e));
    }

    #[test]
    fn derive_is_deterministic_and_interned() {
        let h = builtin("harbor");
        let a = tokens_for("harbor", &h.colors);
        let b = tokens_for("harbor", &h.colors);
        assert!(std::ptr::eq(a, b));
        assert_eq!(*a, derive(&h.colors));
        let e = builtin("ember");
        assert!(!std::ptr::eq(a, tokens_for("ember", &e.colors)));
    }

    fn assert_floors(label: &str, t: &Tokens) {
        let c = |x: Color| contrast(x, t.app);
        assert!(c(t.text) >= 7.0, "{label} text {}", c(t.text));
        assert!(c(t.text_secondary) >= 4.5, "{label} secondary");
        assert!(c(t.text_muted) >= 4.5, "{label} muted");
        for (n, x) in [
            ("accent", t.accent),
            ("success", t.success),
            ("warning", t.warning),
            ("danger", t.danger),
        ] {
            assert!(c(x) >= 3.0, "{label} {n} {}", c(x));
        }
        for x in [
            t.app,
            t.surface,
            t.raised,
            t.border,
            t.hover,
            t.text,
            t.text_secondary,
            t.text_muted,
            t.accent,
            t.accent_soft,
            t.accent_edge,
            t.success,
            t.warning,
            t.danger,
            t.danger_soft,
            t.terminal,
        ] {
            assert!(
                [x.r, x.g, x.b, x.a].iter().all(|v| v.is_finite()),
                "{label}"
            );
        }
    }

    #[test]
    fn every_builtin_and_catalog_theme_meets_floors() {
        for s in fbterm_config::schemes::builtins() {
            assert_floors(&s.id, tokens_for(&s.id, &s.colors));
        }
        let catalog = fbterm_config::catalog::parse_catalog(include_bytes!(
            "../tests/fixtures/terminal-themes-catalog.toml"
        ))
        .unwrap();
        assert!(catalog.themes.len() > 10);
        for t in &catalog.themes {
            assert_floors(&t.id, tokens_for(&t.scheme.id, &t.scheme.colors));
        }
    }

    fn accent_scheme(
        cursor: u32,
        blue: u32,
        bright_blue: u32,
    ) -> fbterm_config::schemes::SchemeColors {
        let mut c = builtin("graphite").colors;
        c.cursor = hex(cursor);
        c.ansi[4] = hex(blue);
        c.ansi[12] = hex(bright_blue);
        c
    }

    #[test]
    fn accent_fallbacks_in_order() {
        let ok = |v| Color::from_rgb8((v >> 16) as u8, (v >> 8) as u8, v as u8);
        // Chromatic cursor wins.
        let t = derive(&accent_scheme(0xa584ff, 0x70b3f7, 0x8ec5ff));
        assert_eq!(t.accent, ok(0xa584ff));
        // Gray cursor falls to ANSI blue.
        let t = derive(&accent_scheme(0x999999, 0x70b3f7, 0x8ec5ff));
        assert_eq!(t.accent, ok(0x70b3f7));
        // Low-contrast ANSI blue (near the dark background) falls to bright blue.
        let t = derive(&accent_scheme(0x999999, 0x14141c, 0x8ec5ff));
        assert_eq!(t.accent, ok(0x8ec5ff));
        // Both blues low-contrast: violet.
        let t = derive(&accent_scheme(0x999999, 0x14141c, 0x15151d));
        assert_eq!(t.accent, ok(0x7c49e3));
    }

    #[test]
    fn degenerate_scheme_still_meets_floors() {
        let colors = fbterm_config::schemes::SchemeColors {
            foreground: gray(0x80),
            background: gray(0x80),
            cursor: gray(0x80),
            selection: gray(0x80),
            ansi: [gray(0x80); 16],
        };
        assert_floors("degenerate", &derive(&colors));
    }

    #[test]
    fn graphite_derives_close_to_dark() {
        let g = builtin("graphite");
        let d = derive(&g.colors);
        for (n, a, b) in [
            ("app", d.app, DARK.app),
            ("surface", d.surface, DARK.surface),
            ("raised", d.raised, DARK.raised),
            ("border", d.border, DARK.border),
            ("text", d.text, DARK.text),
            ("text_secondary", d.text_secondary, DARK.text_secondary),
            ("text_muted", d.text_muted, DARK.text_muted),
        ] {
            for (x, y) in [(a.r, b.r), (a.g, b.g), (a.b, b.b)] {
                assert!((x - y).abs() <= 0.03, "{n}: {a:?} vs {b:?}");
            }
        }
    }

    #[test]
    fn primary_text_contrasts_with_the_accent() {
        for t in [&DARK, &LIGHT] {
            let p = chrome_theme(t).extended_palette().primary.base;
            let other = if p.text == Color::WHITE {
                color!(0x101013)
            } else {
                Color::WHITE
            };
            assert!(contrast(p.text, t.accent) >= contrast(other, t.accent));
        }
    }

    #[test]
    fn terminal_token_is_the_terminal_background() {
        for id in ["graphite", "graphite-light", "harbor", "ember"] {
            let s = builtin(id);
            assert_eq!(
                tokens_for(id, &s.colors).terminal,
                rgb(s.colors.background),
                "{id}"
            );
        }
    }

    #[test]
    fn light_palette_comes_from_the_light_tokens() {
        let p = chrome_theme(&LIGHT).palette();
        assert_eq!(p.background, color!(0xfafafb));
        assert_eq!(p.text, color!(0x1c1c22));
        assert_eq!(p.primary, color!(0x7c49e3));
    }
}
