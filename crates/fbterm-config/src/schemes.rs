//! Color schemes: the data model, the built-in schemes and the on-disk store.
//!
//! A scheme is one plain TOML file per scheme in `<config>/schemes/`, named
//! `<id>.toml`, with colors as `"#rrggbb"`. Built-in schemes live in code and are
//! never written to disk. Profiles and settings refer to a scheme by its stable id.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::profile::Rgb;
use crate::store::{ConfigError, atomic_write, line_of};

/// The colors of a scheme. `selection` is the selection background, already
/// flattened over `background`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SchemeColors {
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    pub selection: Rgb,
    pub ansi: [Rgb; 16],
}

/// A named color scheme, optionally with a font preference. Serializes as a scheme file
/// (the `builtin` flag is never written; a deserialized scheme is never built-in).
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(try_from = "SchemeFile", into = "SchemeFile")]
pub struct Scheme {
    /// Stable id: a built-in id or `user-<uuid>`.
    pub id: String,
    pub name: String,
    pub colors: SchemeColors,
    pub font_family: Option<String>,
    pub font_size: Option<f32>,
    /// Where the colors came from, for a scheme installed from the gallery or imported
    /// with its credit; written as the `[credit]` table.
    pub credit: Option<Credit>,
    /// Gallery bookkeeping for an installed gallery scheme; written as `[gallery]`.
    pub gallery: Option<GalleryInfo>,
    /// A font that suits the scheme, with its official download page; written as
    /// `[suggested_font]`. It applies nothing by itself (unlike `font_family`): the user picks
    /// it, or turns on `use_theme_fonts`. Not part of [`install_hash`].
    pub suggested_font: Option<SuggestedFont>,
    /// True for the schemes shipped in code; never written to a file.
    pub builtin: bool,
}

/// The `[credit]` table: who made a scheme and under which license. The keys match the
/// published theme catalog.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Credit {
    pub project: String,
    pub author: String,
    pub url: String,
    pub license: String,
    pub copyright: String,
    pub license_file: String,
    pub description: String,
    pub derived: bool,
}

/// The `[suggested_font]` table: a font family that suits a scheme, the font's official
/// download page and its license. fbterm never downloads it; the page opens in the browser on
/// a click. Each field follows the credit character rule and [`MAX_FONT_FIELD_CHARS`].
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct SuggestedFont {
    /// The family name exactly as the font database reports it once installed.
    pub family: String,
    /// The font's official page (`https://` only).
    pub url: String,
    /// SPDX id of the font's license, one of [`FONT_LICENSES`].
    pub license: String,
}

/// SPDX ids of the licenses a suggested font may carry: free to use, as `terminal-themes`
/// requires.
pub const FONT_LICENSES: [&str; 9] = [
    "OFL-1.1",
    "Apache-2.0",
    "MIT",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "Unlicense",
    "CC0-1.0",
    "Zlib",
];

/// The longest `[suggested_font]` field, in characters (the catalog's field cap).
pub const MAX_FONT_FIELD_CHARS: usize = 2_000;

/// An `https://` URL with a host and no whitespace or control characters.
pub fn is_https_url(url: &str) -> bool {
    url.strip_prefix("https://").is_some_and(|rest| {
        !rest.is_empty()
            && !rest.starts_with('/')
            && !rest.chars().any(|c| c.is_whitespace() || c.is_control())
    })
}

/// Why `font` may not be shown or used, if it may not: a blank or overlong field, a character
/// of [`is_unsafe_char`] anywhere (no field may span lines), a URL that is not `https://`, or
/// a license off [`FONT_LICENSES`].
pub fn suggested_font_problem(font: &SuggestedFont) -> Option<String> {
    let fields = [
        ("family", &font.family),
        ("url", &font.url),
        ("license", &font.license),
    ];
    for (field, text) in fields {
        if text.trim().is_empty() {
            return Some(format!("{field} is empty"));
        }
        if text.chars().count() > MAX_FONT_FIELD_CHARS {
            return Some(format!(
                "{field} is longer than {MAX_FONT_FIELD_CHARS} characters"
            ));
        }
        if text.chars().any(is_unsafe_char) {
            return Some(format!("{field} has control or invisible characters"));
        }
    }
    if !is_https_url(&font.url) {
        return Some("url is not https://".into());
    }
    if !FONT_LICENSES.contains(&font.license.as_str()) {
        return Some(format!(
            "license {:?} is not on the allowlist",
            font.license
        ));
    }
    None
}

/// A `[suggested_font]` table as read from a file or catalog: the font, or why it is dropped
/// (not a table, a missing key, a value of the wrong type, or [`suggested_font_problem`]).
/// Keys it does not know are ignored.
pub fn suggested_font_from(value: &toml::Value) -> Result<SuggestedFont, String> {
    let Some(table) = value.as_table() else {
        return Err("not a table".into());
    };
    let text = |key: &str| match table.get(key) {
        None => Err(format!("{key} is missing")),
        Some(toml::Value::String(s)) => Ok(s.clone()),
        Some(_) => Err(format!("{key} is not text")),
    };
    let font = SuggestedFont {
        family: text("family")?,
        url: text("url")?,
        license: text("license")?,
    };
    match suggested_font_problem(&font) {
        Some(why) => Err(why),
        None => Ok(font),
    }
}

/// Drop a `[suggested_font]` that could not be shown or opened safely; the colors stay.
pub fn sanitize_suggested_font(scheme: &mut Scheme) {
    if scheme
        .suggested_font
        .as_ref()
        .is_some_and(|f| suggested_font_problem(f).is_some())
    {
        scheme.suggested_font = None;
    }
}

/// The font family a pane using `scheme` is drawn with, when not the global `font_family`:
/// the scheme's own `font_family` if it sets one, else (with `use_theme_fonts` on) its
/// suggested font when `installed` says that family is installed. `None`: the global font.
pub fn theme_font(
    scheme: &Scheme,
    use_theme_fonts: bool,
    installed: impl Fn(&str) -> bool,
) -> Option<&str> {
    if let Some(own) = &scheme.font_family {
        return Some(own);
    }
    scheme
        .suggested_font
        .as_ref()
        .filter(|f| use_theme_fonts && installed(&f.family))
        .map(|f| f.family.as_str())
}

/// The `[gallery]` table: which catalog entry a scheme was installed from.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct GalleryInfo {
    /// The catalog id (the scheme id without [`GALLERY_PREFIX`]).
    pub id: String,
    /// The catalog version installed.
    pub version: u32,
    /// [`install_hash`] of the scheme as installed (name, font and colors), to tell later
    /// edits. Kept under this key so files written before the hash covered the name and font
    /// still load; their value has no format prefix, so they count as edited.
    pub colors_hash: String,
}

/// Scheme ids installed from the gallery start with this.
pub const GALLERY_PREFIX: &str = "gallery-";

/// The format prefix of [`install_hash`]; a stored hash with any other prefix (or none, as
/// written by the first gallery builds, which hashed the colors only) never matches.
const INSTALL_HASH_FORMAT: &str = "2:";

/// A stable fingerprint of what Update would overwrite: the name, the font family and size
/// and the 20 colors in file order, hashed with FNV-1a 64 and written as
/// `2:<16 lowercase hex digits>`. Identical across runs and platforms.
pub fn install_hash(scheme: &Scheme) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |bytes: &[u8]| {
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    };
    // Every variable-length field is length-prefixed and every optional one tagged, so no two
    // different schemes feed the same bytes.
    let text = |feed: &mut dyn FnMut(&[u8]), t: &str| {
        feed(&(t.len() as u64).to_le_bytes());
        feed(t.as_bytes());
    };
    text(&mut feed, &scheme.name);
    match &scheme.font_family {
        None => feed(&[0]),
        Some(f) => {
            feed(&[1]);
            text(&mut feed, f);
        }
    }
    match scheme.font_size {
        None => feed(&[0]),
        Some(size) => {
            feed(&[1]);
            feed(&size.to_bits().to_le_bytes());
        }
    }
    let c = &scheme.colors;
    for rgb in [c.foreground, c.background, c.cursor, c.selection]
        .into_iter()
        .chain(c.ansi)
    {
        feed(&[rgb.r, rgb.g, rgb.b]);
    }
    format!("{INSTALL_HASH_FORMAT}{h:016x}")
}

/// The catalog id of a scheme whose id starts with [`GALLERY_PREFIX`], with or without a
/// `[gallery]` table.
pub fn gallery_id(scheme: &Scheme) -> Option<&str> {
    scheme
        .id
        .strip_prefix(GALLERY_PREFIX)
        .filter(|id| !id.is_empty())
}

/// Drop a `[gallery]` block that does not match the scheme id (its `id` must be the catalog
/// id in a `gallery-` scheme id), so a hand-edited or foreign block is never trusted.
pub fn sanitize_gallery(scheme: &mut Scheme) {
    let agrees = scheme
        .gallery
        .as_ref()
        .is_some_and(|g| gallery_id(scheme) == Some(g.id.as_str()));
    if !agrees {
        scheme.gallery = None;
    }
}

/// Whether the scheme differs from what the gallery installed: no `[gallery]` table, or a
/// stored hash that no longer matches [`install_hash`] (a changed name, font or color, or a
/// hash of an older or unknown format).
pub fn is_edited(scheme: &Scheme) -> bool {
    scheme
        .gallery
        .as_ref()
        .is_none_or(|g| g.colors_hash != install_hash(scheme))
}

/// Whether `c` must never appear in displayed credit or catalog text: control characters and
/// the invisible, line-breaking or bidi formatting characters that can hide or reorder text.
pub fn is_unsafe_char(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{00AD}'
                | '\u{061C}'
                | '\u{180E}'
                | '\u{200B}'..='\u{200F}'
                | '\u{2028}'..='\u{202E}'
                | '\u{2060}'
                | '\u{2066}'..='\u{2069}'
                | '\u{FEFF}'
        )
}

/// The first `[credit]` field holding a character of [`is_unsafe_char`], if any. Only the
/// description may span lines (`\n`).
pub fn unsafe_credit_field(credit: &Credit) -> Option<&'static str> {
    [
        ("project", &credit.project),
        ("author", &credit.author),
        ("url", &credit.url),
        ("license", &credit.license),
        ("copyright", &credit.copyright),
        ("license_file", &credit.license_file),
        ("description", &credit.description),
    ]
    .into_iter()
    .find(|(field, text)| {
        text.chars()
            .any(|c| is_unsafe_char(c) && !(*field == "description" && c == '\n'))
    })
    .map(|(field, _)| field)
}

/// Whether every `[credit]` field is free of [`is_unsafe_char`] characters (the description
/// may span lines). The same rule holds for catalog themes, scheme files and archives.
pub fn credit_is_safe(credit: &Credit) -> bool {
    unsafe_credit_field(credit).is_none()
}

/// Drop a `[credit]` table that could hide or reorder text when shown; the colors stay.
pub fn sanitize_credit(scheme: &mut Scheme) {
    if scheme.credit.as_ref().is_some_and(|c| !credit_is_safe(c)) {
        scheme.credit = None;
    }
}

/// Ids of the built-in schemes, fixed forever.
pub const BUILTIN_IDS: [&str; 4] = ["graphite", "graphite-light", "harbor", "ember"];

/// The `[colors]` table of a scheme file.
#[derive(Serialize, Deserialize)]
pub(crate) struct ColorsFile {
    foreground: Rgb,
    background: Rgb,
    cursor: Rgb,
    selection: Rgb,
    black: Rgb,
    red: Rgb,
    green: Rgb,
    yellow: Rgb,
    blue: Rgb,
    magenta: Rgb,
    cyan: Rgb,
    white: Rgb,
    bright_black: Rgb,
    bright_red: Rgb,
    bright_green: Rgb,
    bright_yellow: Rgb,
    bright_blue: Rgb,
    bright_magenta: Rgb,
    bright_cyan: Rgb,
    bright_white: Rgb,
}

/// The shape of a scheme file.
#[derive(Serialize, Deserialize)]
struct SchemeFile {
    id: String,
    name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    font_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    font_size: Option<f32>,
    colors: ColorsFile,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    credit: Option<Credit>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    gallery: Option<GalleryInfo>,
    /// Read as any value so a malformed table drops only itself (see
    /// [`suggested_font_from`]), never the scheme.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    suggested_font: Option<FontSlot>,
}

/// A `[suggested_font]` value: the table in its own shape (written in field order), or
/// anything else, which is dropped with a notice.
#[derive(Serialize, Deserialize)]
#[serde(untagged)]
enum FontSlot {
    Font(SuggestedFont),
    Other(toml::Value),
}

impl FontSlot {
    fn check(&self) -> Result<SuggestedFont, String> {
        match self {
            Self::Font(font) => match suggested_font_problem(font) {
                Some(why) => Err(why),
                None => Ok(font.clone()),
            },
            Self::Other(value) => suggested_font_from(value),
        }
    }
}

impl From<Scheme> for SchemeFile {
    fn from(scheme: Scheme) -> Self {
        Self {
            colors: ColorsFile::from(&scheme.colors),
            id: scheme.id,
            name: scheme.name,
            font_family: scheme.font_family,
            font_size: scheme.font_size,
            credit: scheme.credit,
            gallery: scheme.gallery,
            suggested_font: scheme.suggested_font.map(FontSlot::Font),
        }
    }
}

impl TryFrom<SchemeFile> for Scheme {
    type Error = String;

    /// Checks what the file format cannot: a non-empty id and name, a positive font size.
    fn try_from(file: SchemeFile) -> Result<Self, String> {
        if file.id.is_empty() || file.name.trim().is_empty() {
            return Err("a scheme needs a non-empty id and name".into());
        }
        if file.font_size.is_some_and(|f| !f.is_finite() || f <= 0.0) {
            return Err("font_size must be a positive number".into());
        }
        Ok(Scheme {
            id: file.id,
            name: file.name,
            colors: file.colors.into(),
            font_family: file.font_family,
            font_size: file.font_size,
            credit: file.credit,
            gallery: file.gallery,
            suggested_font: file.suggested_font.and_then(|v| v.check().ok()),
            builtin: false,
        })
    }
}

impl From<&SchemeColors> for ColorsFile {
    fn from(c: &SchemeColors) -> Self {
        let a = &c.ansi;
        Self {
            foreground: c.foreground,
            background: c.background,
            cursor: c.cursor,
            selection: c.selection,
            black: a[0],
            red: a[1],
            green: a[2],
            yellow: a[3],
            blue: a[4],
            magenta: a[5],
            cyan: a[6],
            white: a[7],
            bright_black: a[8],
            bright_red: a[9],
            bright_green: a[10],
            bright_yellow: a[11],
            bright_blue: a[12],
            bright_magenta: a[13],
            bright_cyan: a[14],
            bright_white: a[15],
        }
    }
}

impl From<ColorsFile> for SchemeColors {
    fn from(c: ColorsFile) -> Self {
        Self {
            foreground: c.foreground,
            background: c.background,
            cursor: c.cursor,
            selection: c.selection,
            ansi: [
                c.black,
                c.red,
                c.green,
                c.yellow,
                c.blue,
                c.magenta,
                c.cyan,
                c.white,
                c.bright_black,
                c.bright_red,
                c.bright_green,
                c.bright_yellow,
                c.bright_blue,
                c.bright_magenta,
                c.bright_cyan,
                c.bright_white,
            ],
        }
    }
}

const fn hex(v: u32) -> Rgb {
    Rgb {
        r: (v >> 16) as u8,
        g: (v >> 8) as u8,
        b: v as u8,
    }
}

/// A built-in scheme: `(id, name)`, its suggested font `(family, page)` (all OFL-1.1) and
/// its colors.
fn builtin(
    (id, name): (&str, &str),
    font: (&str, &str),
    foreground: u32,
    background: u32,
    cursor: u32,
    selection: u32,
    ansi: [u32; 16],
) -> Scheme {
    Scheme {
        id: id.to_string(),
        name: name.to_string(),
        colors: SchemeColors {
            foreground: hex(foreground),
            background: hex(background),
            cursor: hex(cursor),
            selection: hex(selection),
            ansi: ansi.map(hex),
        },
        font_family: None,
        font_size: None,
        credit: None,
        gallery: None,
        suggested_font: Some(SuggestedFont {
            family: font.0.to_string(),
            url: font.1.to_string(),
            license: "OFL-1.1".to_string(),
        }),
        builtin: true,
    }
}

/// The four built-in schemes, in `BUILTIN_IDS` order.
pub fn builtins() -> Vec<Scheme> {
    vec![
        // The design system's dark Graphite palette; selection is violet-400 at 30%
        // over the background.
        builtin(
            ("graphite", "Graphite"),
            // fbterm's own font, the design system's mono.
            ("Geist Mono", "https://vercel.com/font"),
            0xf5f5f7,
            0x0b0b0e,
            0xa584ff,
            0x392f56,
            [
                0x262629, 0xed4a49, 0x2bbb71, 0xf2a635, 0x70b3f7, 0xa584ff, 0x59c5f5, 0xd0d0d9,
                0x919199, 0xfd736d, 0x52cd86, 0xfac053, 0x9ccbfa, 0xbea8ff, 0x8fd6f7, 0xf5f5f7,
            ],
        ),
        // Bright black is darker than black and bright white is the ink color, as is
        // usual on a light ground. The design system's light tokens: app canvas, ink text, violet-600 cursor;
        // status colors darkened until they pass on the light canvas.
        builtin(
            ("graphite-light", "Graphite Light"),
            // The same design system, so the same font.
            ("Geist Mono", "https://vercel.com/font"),
            0x1c1c22,
            0xfafafb,
            0x7c49e3,
            0xe1d7f6,
            [
                0x4f4f56, 0xc52b30, 0x00774a, 0x8f5700, 0x1a64c4, 0x7c49e3, 0x0b6f94, 0x686870,
                0x3a3a42, 0xb01f2a, 0x006b41, 0x7d4c00, 0x1556ad, 0x6738c1, 0x0a6384, 0x1c1c22,
            ],
        ),
        // Cool blue-green: deep teal-black ground, aqua cursor.
        builtin(
            ("harbor", "Harbor"),
            // A crisp, cool modern mono for the blue-green palette.
            ("JetBrains Mono", "https://www.jetbrains.com/lp/mono/"),
            0xe3f2f4,
            0x0a1a20,
            0x4fd1c5,
            0x1d4d4e,
            [
                0x17313a, 0xf0716c, 0x4cc9a0, 0xe5c15d, 0x5aa9e6, 0x9d8cf0, 0x3fd0d8, 0xc3dadd,
                0x5f8089, 0xff918c, 0x73ddb8, 0xf2d683, 0x85c3f2, 0xb9adf7, 0x78e2e8, 0xf4fbfc,
            ],
        ),
        // Warm red-orange on a coal-brown ground; its look makes a production host
        // stand out.
        builtin(
            ("ember", "Ember"),
            // A warm, humanist mono for the coal and ember palette.
            ("IBM Plex Mono", "https://github.com/IBM/plex"),
            0xf6ece4,
            0x1a0f0c,
            0xff8a4c,
            0x5a311e,
            [
                0x3a2520, 0xff5f56, 0x9fcf5a, 0xffb347, 0x6fa8dc, 0xd98cb3, 0x4fc3b0, 0xd9c5b8,
                0x8a6a5e, 0xff857c, 0xb8e07a, 0xffc872, 0x94c0e8, 0xe8a8c8, 0x7ad6c6, 0xfff6ee,
            ],
        ),
    ]
}

fn luminance(c: Rgb) -> f32 {
    let lin = |v: u8| {
        let v = f32::from(v) / 255.0;
        if v <= 0.040_45 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
}

/// WCAG 2 contrast ratio between two colors (1.0 to 21.0).
pub fn contrast_ratio(a: Rgb, b: Rgb) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// Whether a scheme meets the readability bar the built-ins are held to: foreground
/// against background at least 7.0, ANSI colors 1 to 6 and 9 to 14 against the
/// background at least 4.5, and the selection against the foreground at least 4.5.
pub fn contrast_ok(colors: &SchemeColors) -> bool {
    let bg = colors.background;
    contrast_ratio(colors.foreground, bg) >= 7.0
        && (1..=6)
            .chain(9..=14)
            .all(|i| contrast_ratio(colors.ansi[i], bg) >= 4.5)
        && contrast_ratio(colors.selection, colors.foreground) >= 4.5
}

/// A fresh id for a user scheme: `user-<uuid v4>`.
pub fn new_user_id() -> String {
    format!("user-{}", Uuid::new_v4())
}

/// The largest scheme file read: a real one is well under 2 KiB.
pub const MAX_FILE_BYTES: u64 = 64 * 1024;

/// Read at most `MAX_FILE_BYTES` from `reader`; `None` when there is more, however the
/// size was reported (a file can grow after its length was checked).
fn read_capped(reader: impl std::io::Read) -> std::io::Result<Option<Vec<u8>>> {
    use std::io::Read;
    let mut buf = Vec::new();
    reader.take(MAX_FILE_BYTES + 1).read_to_end(&mut buf)?;
    Ok((buf.len() as u64 <= MAX_FILE_BYTES).then_some(buf))
}

/// Read one scheme file. The result is never marked built-in.
pub fn read_file(path: &Path) -> Result<Scheme, ConfigError> {
    read_file_noted(path).map(|(scheme, _)| scheme)
}

/// [`read_file`], with the notice for a `[suggested_font]` table that was dropped (the
/// scheme itself loads).
pub fn read_file_noted(path: &Path) -> Result<(Scheme, Option<ConfigError>), ConfigError> {
    let err = |m: String| ConfigError::new(path, None, m);
    let meta = std::fs::metadata(path).map_err(|e| err(e.to_string()))?;
    // A pipe, device or socket reports no size and can be endless: never read one.
    if !meta.is_file() {
        return Err(err("Not a color scheme file.".into()));
    }
    let file = std::fs::File::open(path).map_err(|e| err(e.to_string()))?;
    let bytes = read_capped(file).map_err(|e| err(e.to_string()))?;
    let Some(bytes) = bytes else {
        return Err(err("Not a color scheme file (too large).".into()));
    };
    let text =
        String::from_utf8(bytes).map_err(|_| err("Not a color scheme file (not text).".into()))?;
    let file: SchemeFile = toml::from_str(&text).map_err(|e| {
        let line = e.span().map(|s| line_of(&text, s.start));
        ConfigError::new(path, line, e.message().to_string())
    })?;
    let note = file
        .suggested_font
        .as_ref()
        .and_then(|v| v.check().err())
        .map(|why| ConfigError::new(path, None, format!("suggested_font ignored: {why}")));
    let mut scheme = Scheme::try_from(file).map_err(|m| ConfigError::new(path, None, m))?;
    sanitize_gallery(&mut scheme);
    sanitize_credit(&mut scheme);
    Ok((scheme, note))
}

/// Write a scheme file atomically. The `builtin` flag is never written.
pub fn write_file(path: &Path, scheme: &Scheme) -> Result<(), ConfigError> {
    let text =
        toml::to_string_pretty(scheme).map_err(|e| ConfigError::new(path, None, e.to_string()))?;
    atomic_write(path, text.as_bytes())
}

/// Whether `id` can name a scheme file: lowercase letters, digits, `-` and `_`, at most 100
/// characters. Lowercase only and length-capped, so ids name the same file on case-insensitive
/// file systems and stay short.
pub fn is_plain_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 100
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}

/// Ids become file names, so only plain characters are allowed.
fn check_user_id(dir: &Path, id: &str) -> Result<PathBuf, ConfigError> {
    let path = dir.join(format!("{id}.toml"));
    if BUILTIN_IDS.contains(&id) {
        return Err(ConfigError::new(
            &path,
            None,
            format!("{id:?} is a built-in scheme"),
        ));
    }
    let plain = is_plain_id(id);
    if !plain {
        return Err(ConfigError::new(
            &path,
            None,
            format!("invalid scheme id {id:?}"),
        ));
    }
    Ok(path)
}

/// Whether a user scheme from elsewhere (an imported archive) could be a scheme file: a plain,
/// non-built-in id, at most [`MAX_FILE_BYTES`] once written, and loading back unchanged
/// through the scheme file checks.
pub fn is_valid_user(scheme: &Scheme) -> bool {
    if scheme.builtin || check_user_id(Path::new(""), &scheme.id).is_err() {
        return false;
    }
    let Ok(text) = toml::to_string_pretty(scheme) else {
        return false;
    };
    if text.len() as u64 > MAX_FILE_BYTES {
        return false;
    }
    toml::from_str::<Scheme>(&text).is_ok_and(|back| back == *scheme)
}

/// Save a user scheme as `<dir>/<id>.toml`. Built-ins are refused.
pub fn save_user(dir: &Path, scheme: &Scheme) -> Result<PathBuf, ConfigError> {
    let path = check_user_id(dir, &scheme.id)?;
    if scheme.builtin {
        return Err(ConfigError::new(
            &path,
            None,
            "built-in schemes are not saved".into(),
        ));
    }
    write_file(&path, scheme)?;
    Ok(path)
}

/// Delete a user scheme's file. Built-ins are refused; a missing file is fine.
pub fn delete_user(dir: &Path, id: &str) -> Result<(), ConfigError> {
    let path = check_user_id(dir, id)?;
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(ConfigError::new(&path, None, e.to_string())),
    }
}

/// Every scheme available: the built-ins, then the user files.
#[derive(Clone, PartialEq, Debug)]
pub struct SchemeSet {
    schemes: Vec<Scheme>,
}

impl SchemeSet {
    /// Load the built-ins and every `*.toml` file in `dir`, sorted by file name. A
    /// missing directory means no user schemes. A file that does not parse, uses a
    /// built-in id or repeats an earlier id is skipped with an error. A scheme whose
    /// `[suggested_font]` table was dropped loads, with an error saying so.
    pub fn load(dir: &Path) -> (Self, Vec<ConfigError>) {
        let mut schemes = builtins();
        let mut errors = Vec::new();
        let mut files: Vec<PathBuf> = match std::fs::read_dir(dir) {
            Ok(entries) => entries
                .filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "toml") && p.is_file())
                .collect(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => {
                errors.push(ConfigError::new(dir, None, e.to_string()));
                Vec::new()
            }
        };
        files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
        for path in files {
            let read = read_file_noted(&path).map(|(scheme, note)| {
                errors.extend(note);
                scheme
            });
            match read {
                Err(e) => errors.push(e),
                Ok(s) if BUILTIN_IDS.contains(&s.id.as_str()) => errors.push(ConfigError::new(
                    &path,
                    None,
                    format!("{:?} is a built-in scheme id", s.id),
                )),
                Ok(s) if schemes.iter().any(|o| o.id == s.id) => errors.push(ConfigError::new(
                    &path,
                    None,
                    format!("duplicate scheme id {:?}", s.id),
                )),
                Ok(s) => match check_user_id(dir, &s.id) {
                    Err(e) => errors.push(ConfigError::new(&path, None, e.message)),
                    Ok(expected) if expected.file_name() != path.file_name() => {
                        errors.push(ConfigError::new(
                            &path,
                            None,
                            format!("file name must be {}.toml to match id {:?}", s.id, s.id),
                        ));
                    }
                    Ok(_) => schemes.push(s),
                },
            }
        }
        (Self { schemes }, errors)
    }

    pub fn get(&self, id: &str) -> Option<&Scheme> {
        self.schemes.iter().find(|s| s.id == id)
    }

    pub fn all(&self) -> &[Scheme] {
        &self.schemes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheme_serde_round_trips_and_validates() {
        let mut scheme = builtins().remove(2);
        scheme.font_family = Some("Geist Mono".into());
        scheme.font_size = Some(13.5);
        let text = toml::to_string_pretty(&scheme).unwrap();
        assert!(!text.contains("builtin"));
        let back: Scheme = toml::from_str(&text).unwrap();
        assert_eq!(
            back,
            Scheme {
                builtin: false,
                ..scheme
            }
        );

        let bad = text.replace("font_size = 13.5", "font_size = -1.0");
        assert!(toml::from_str::<Scheme>(&bad).is_err());
    }

    #[test]
    fn valid_user_schemes_follow_the_file_rules() {
        let mut ok = builtins().remove(1);
        ok.builtin = false;
        ok.id = "user-1".into();
        assert!(is_valid_user(&ok));

        let builtin = builtins().remove(0);
        assert!(!is_valid_user(&builtin));
        let builtin_id = Scheme {
            id: "graphite".into(),
            ..ok.clone()
        };
        assert!(!is_valid_user(&builtin_id));
        for id in ["", "../x", "User-1", "a/b", &"x".repeat(101)] {
            let bad = Scheme {
                id: id.into(),
                ..ok.clone()
            };
            assert!(!is_valid_user(&bad), "{id:?}");
        }
        let nameless = Scheme {
            name: " ".into(),
            ..ok.clone()
        };
        assert!(!is_valid_user(&nameless));
        let bad_size = Scheme {
            font_size: Some(f32::NAN),
            ..ok.clone()
        };
        assert!(!is_valid_user(&bad_size));
        let huge = Scheme {
            font_family: Some("f".repeat(MAX_FILE_BYTES as usize)),
            ..ok
        };
        assert!(!is_valid_user(&huge));
    }

    #[test]
    fn growing_read_is_bounded() {
        // An endless reader stops one byte past the cap.
        assert!(read_capped(std::io::repeat(b' ')).unwrap().is_none());
        let at_cap = vec![b'a'; MAX_FILE_BYTES as usize];
        assert_eq!(
            read_capped(&at_cap[..]).unwrap().unwrap().len(),
            at_cap.len()
        );
        let over = vec![b'a'; MAX_FILE_BYTES as usize + 1];
        assert!(read_capped(&over[..]).unwrap().is_none());
    }

    fn sample_credit() -> Credit {
        Credit {
            project: "Nord".into(),
            author: "Arctic Ice Studio".into(),
            url: "https://example.org/nord".into(),
            license: "MIT".into(),
            copyright: "Copyright (c) 2016".into(),
            license_file: "LICENSE".into(),
            description: "An arctic palette.".into(),
            derived: true,
        }
    }

    fn gallery_scheme() -> Scheme {
        let mut s = builtins().remove(0);
        s.builtin = false;
        s.id = "gallery-nord".into();
        s.credit = Some(sample_credit());
        s.gallery = Some(GalleryInfo {
            id: "nord".into(),
            version: 2,
            colors_hash: install_hash(&s),
        });
        s
    }

    #[test]
    fn credit_and_gallery_round_trip() {
        let scheme = gallery_scheme();
        let text = toml::to_string_pretty(&scheme).unwrap();
        assert!(text.contains("[credit]") && text.contains("[gallery]"));
        assert!(text.contains("license_file = \"LICENSE\"") && text.contains("derived = true"));
        let back: Scheme = toml::from_str(&text).unwrap();
        assert_eq!(back, scheme);
    }

    #[test]
    fn old_files_without_tables_load() {
        let text = toml::to_string_pretty(&builtins().remove(0)).unwrap();
        assert!(!text.contains("[credit]") && !text.contains("[gallery]"));
        let back: Scheme = toml::from_str(&text).unwrap();
        assert!(back.credit.is_none() && back.gallery.is_none());
    }

    #[test]
    fn file_with_tables_loads_in_old_shape() {
        /// A scheme file as read before the tables existed (no `deny_unknown_fields`).
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct OldSchemeFile {
            id: String,
            name: String,
            font_family: Option<String>,
            font_size: Option<f32>,
            colors: ColorsFile,
        }
        let text = toml::to_string_pretty(&gallery_scheme()).unwrap();
        let old: OldSchemeFile = toml::from_str(&text).unwrap();
        assert_eq!(old.id, "gallery-nord");
    }

    #[test]
    fn install_hash_is_stable() {
        assert_eq!(install_hash(&builtins().remove(0)), "2:27f2b40e716e5a72");
    }

    #[test]
    fn install_hash_covers_name_and_font() {
        let base = gallery_scheme();
        let hash = install_hash(&base);
        assert!(hash.starts_with("2:") && hash.len() == 18, "{hash}");
        let mut renamed = base.clone();
        renamed.name = "Nord (mine)".into();
        let mut family = base.clone();
        family.font_family = Some("Geist Mono".into());
        let mut size = base.clone();
        size.font_size = Some(15.0);
        let mut both = base.clone();
        both.font_family = Some(String::new());
        for other in [&renamed, &family, &size, &both] {
            assert_ne!(install_hash(other), hash);
            assert!(is_edited(&Scheme {
                gallery: base.gallery.clone(),
                ..other.clone()
            }));
        }
        // An empty family is not the same as none.
        assert_ne!(install_hash(&both), hash);
        // The credit and the gallery block are not part of it.
        let mut credited = base.clone();
        credited.credit = None;
        credited.gallery = None;
        assert_eq!(install_hash(&credited), hash);
    }

    #[test]
    fn older_or_unknown_hash_formats_count_as_edited() {
        let mut s = gallery_scheme();
        let current = install_hash(&s);
        // The first gallery builds wrote the bare colors hash, with no prefix.
        for stored in [
            "210775489270ef4a".to_owned(),
            format!("3:{}", &current[2..]),
            current[2..].to_owned(),
            String::new(),
        ] {
            s.gallery.as_mut().unwrap().colors_hash = stored;
            assert!(is_edited(&s));
        }
    }

    #[test]
    fn unsafe_credit_characters_are_found() {
        assert!(credit_is_safe(&sample_credit()));
        let mut multi = sample_credit();
        multi.description = "Line one.\nLine two.".into();
        assert!(credit_is_safe(&multi));
        for c in [
            '\u{0007}', '\u{00AD}', '\u{061C}', '\u{180E}', '\u{200B}', '\u{200F}', '\u{2028}',
            '\u{2029}', '\u{202E}', '\u{2060}', '\u{2066}', '\u{2069}', '\u{FEFF}',
        ] {
            assert!(is_unsafe_char(c), "{:04X}", c as u32);
            let mut bad = sample_credit();
            bad.author = format!("Some{c}one");
            assert_eq!(unsafe_credit_field(&bad), Some("author"));
            assert!(!credit_is_safe(&bad));
        }
        let mut newline = sample_credit();
        newline.project = "Two\nlines".into();
        assert_eq!(unsafe_credit_field(&newline), Some("project"));
        assert!(!is_unsafe_char('e') && !is_unsafe_char('\u{00E9}') && !is_unsafe_char(' '));
    }

    #[test]
    fn read_file_drops_an_unsafe_credit() {
        let dir = std::env::temp_dir().join(format!("fbterm-cred-{}", new_user_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = gallery_scheme();
        s.credit.as_mut().unwrap().author = "evil\u{202E}txt.exe".into();
        let path = dir.join("gallery-nord.toml");
        std::fs::write(&path, toml::to_string_pretty(&s).unwrap()).unwrap();
        let back = read_file(&path).unwrap();
        assert!(back.credit.is_none());
        assert_eq!(back.colors, s.colors);
        // A safe credit stays.
        std::fs::write(&path, toml::to_string_pretty(&gallery_scheme()).unwrap()).unwrap();
        assert_eq!(read_file(&path).unwrap().credit, Some(sample_credit()));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn gallery_id_without_block_counts_as_edited() {
        let mut s = gallery_scheme();
        s.gallery = None;
        assert_eq!(gallery_id(&s), Some("nord"));
        assert!(is_edited(&s));
        s.id = "user-1".into();
        assert_eq!(gallery_id(&s), None);
    }

    #[test]
    fn edited_colors_count_as_edited() {
        let mut s = gallery_scheme();
        s.colors.cursor = hex(0x123456);
        assert!(is_edited(&s));
    }

    #[test]
    fn sanitize_gallery_keeps_only_matching_blocks() {
        let mut ok = gallery_scheme();
        sanitize_gallery(&mut ok);
        assert!(ok.gallery.is_some());

        let mut user = gallery_scheme();
        user.id = "user-x".into();
        sanitize_gallery(&mut user);
        assert!(user.gallery.is_none() && user.credit.is_some());

        let mut other = gallery_scheme();
        other.gallery.as_mut().unwrap().id = "bar".into();
        sanitize_gallery(&mut other);
        assert!(other.gallery.is_none());
    }

    #[test]
    fn read_file_drops_a_mismatched_gallery_block() {
        let dir = std::env::temp_dir().join(format!("fbterm-san-{}", new_user_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = gallery_scheme();
        s.id = "user-x".into();
        let path = dir.join("user-x.toml");
        std::fs::write(&path, toml::to_string_pretty(&s).unwrap()).unwrap();
        assert!(read_file(&path).unwrap().gallery.is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn untouched_gallery_scheme_is_not_edited() {
        assert!(!is_edited(&gallery_scheme()));
    }

    #[test]
    fn is_valid_user_accepts_gallery_ids() {
        assert!(is_valid_user(&gallery_scheme()));
    }

    fn sample_font() -> SuggestedFont {
        SuggestedFont {
            family: "JetBrains Mono".into(),
            url: "https://www.jetbrains.com/lp/mono/".into(),
            license: "OFL-1.1".into(),
        }
    }

    /// A user scheme file's text with `table` as its `[suggested_font]` body.
    fn file_with_font(table: &str) -> String {
        let mut s = builtins().remove(0);
        s.builtin = false;
        s.id = "user-font".into();
        s.suggested_font = None;
        format!(
            "{}\n[suggested_font]\n{table}\n",
            toml::to_string_pretty(&s).unwrap()
        )
    }

    #[test]
    fn suggested_font_parses_and_round_trips() {
        let text = file_with_font(
            "family = \"JetBrains Mono\"\nurl = \"https://www.jetbrains.com/lp/mono/\"\n\
             license = \"OFL-1.1\"",
        );
        let scheme: Scheme = toml::from_str(&text).unwrap();
        assert_eq!(scheme.suggested_font, Some(sample_font()));
        let again = toml::to_string_pretty(&scheme).unwrap();
        assert!(again.contains("[suggested_font]"), "{again}");
        assert_eq!(toml::from_str::<Scheme>(&again).unwrap(), scheme);
        assert!(is_valid_user(&scheme));
        // A future key in the table is ignored, as elsewhere in scheme files.
        let future = file_with_font(
            "family = \"JetBrains Mono\"\nurl = \"https://www.jetbrains.com/lp/mono/\"\n\
             license = \"OFL-1.1\"\nweight = 400",
        );
        let scheme: Scheme = toml::from_str(&future).unwrap();
        assert_eq!(scheme.suggested_font, Some(sample_font()));
    }

    #[test]
    fn bad_suggested_font_is_dropped_and_the_scheme_kept() {
        let long = "y".repeat(MAX_FONT_FIELD_CHARS + 1);
        let bad_tables = [
            // A missing key.
            "family = \"JetBrains Mono\"\nurl = \"https://www.jetbrains.com/lp/mono/\"".to_owned(),
            "url = \"https://a.example/\"\nlicense = \"OFL-1.1\"".to_owned(),
            // Not https.
            "family = \"A\"\nurl = \"http://a.example/\"\nlicense = \"OFL-1.1\"".to_owned(),
            "family = \"A\"\nurl = \"javascript:alert(1)\"\nlicense = \"OFL-1.1\"".to_owned(),
            "family = \"A\"\nurl = \"https://\"\nlicense = \"OFL-1.1\"".to_owned(),
            // Wrong types.
            "family = 1\nurl = \"https://a.example/\"\nlicense = \"OFL-1.1\"".to_owned(),
            "family = \"A\"\nurl = [\"https://a.example/\"]\nlicense = \"OFL-1.1\"".to_owned(),
            // A license off the list, a blank family, hidden characters, too long.
            "family = \"A\"\nurl = \"https://a.example/\"\nlicense = \"GPL-3.0-only\"".to_owned(),
            "family = \" \"\nurl = \"https://a.example/\"\nlicense = \"OFL-1.1\"".to_owned(),
            "family = \"A\\u202eB\"\nurl = \"https://a.example/\"\nlicense = \"OFL-1.1\""
                .to_owned(),
            "family = \"A\\nB\"\nurl = \"https://a.example/\"\nlicense = \"OFL-1.1\"".to_owned(),
            format!("family = \"{long}\"\nurl = \"https://a.example/\"\nlicense = \"OFL-1.1\""),
        ];
        let dir = std::env::temp_dir().join(format!("fbterm-font-{}", new_user_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("user-font.toml");
        for table in &bad_tables {
            let text = file_with_font(table);
            let scheme: Scheme = toml::from_str(&text).expect(table);
            assert!(scheme.suggested_font.is_none(), "{table}");
            std::fs::write(&path, &text).unwrap();
            let (read, note) = read_file_noted(&path).unwrap();
            assert!(read.suggested_font.is_none(), "{table}");
            assert_eq!(read.colors, builtins()[0].colors);
            let note = note.expect(table);
            assert!(note.message.contains("suggested_font"), "{}", note.message);
            // The set keeps the scheme and reports the dropped table.
            let (set, errors) = SchemeSet::load(&dir);
            assert!(set.get("user-font").is_some(), "{table}");
            assert_eq!(errors.len(), 1, "{errors:?}");
        }
        // Not a table at all.
        let mut s = builtins().remove(0);
        s.builtin = false;
        s.id = "user-font".into();
        s.suggested_font = None;
        let text = toml::to_string_pretty(&s).unwrap().replacen(
            "name = ",
            "suggested_font = \"Fira Code\"\nname = ",
            1,
        );
        let scheme: Scheme = toml::from_str(&text).unwrap();
        assert!(scheme.suggested_font.is_none());
        // Every license on the list is accepted.
        for license in FONT_LICENSES {
            let text = file_with_font(&format!(
                "family = \"A\"\nurl = \"https://a.example/\"\nlicense = \"{license}\""
            ));
            let scheme: Scheme = toml::from_str(&text).unwrap();
            assert!(scheme.suggested_font.is_some(), "{license}");
        }
        // A good table gives no note.
        std::fs::write(
            &path,
            file_with_font("family = \"A\"\nurl = \"https://a.example/\"\nlicense = \"OFL-1.1\""),
        )
        .unwrap();
        assert!(read_file_noted(&path).unwrap().1.is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sanitize_suggested_font_drops_an_unsafe_table() {
        let mut s = gallery_scheme();
        s.suggested_font = Some(SuggestedFont {
            family: "Fira\u{200B}Code".into(),
            ..sample_font()
        });
        sanitize_suggested_font(&mut s);
        assert!(s.suggested_font.is_none());
        let mut ok = gallery_scheme();
        ok.suggested_font = Some(sample_font());
        sanitize_suggested_font(&mut ok);
        assert_eq!(ok.suggested_font, Some(sample_font()));
    }

    #[test]
    fn builtins_have_suggestions() {
        for s in builtins() {
            let font = s
                .suggested_font
                .as_ref()
                .unwrap_or_else(|| panic!("{}", s.id));
            assert_eq!(suggested_font_problem(font), None, "{}", s.id);
            assert!(FONT_LICENSES.contains(&font.license.as_str()));
        }
        let graphite = builtins().remove(0);
        assert_eq!(
            graphite.suggested_font.unwrap().family,
            crate::settings::Settings::default().font_family,
            "the default scheme suggests the default font"
        );
    }

    #[test]
    fn install_hash_ignores_the_suggestion() {
        let base = gallery_scheme();
        let mut suggested = base.clone();
        suggested.suggested_font = Some(sample_font());
        assert_eq!(install_hash(&suggested), install_hash(&base));
        assert!(!is_edited(&suggested));
    }

    #[test]
    fn theme_font_resolution() {
        let mut s = gallery_scheme();
        s.suggested_font = Some(sample_font());
        let installed = |f: &str| f == "JetBrains Mono";
        let missing = |_: &str| false;
        // Off: never the suggestion.
        assert_eq!(theme_font(&s, false, installed), None);
        // On and installed: the suggestion.
        assert_eq!(theme_font(&s, true, installed), Some("JetBrains Mono"));
        // On but missing: the global font (none from the scheme).
        assert_eq!(theme_font(&s, true, missing), None);
        // The scheme's own font wins either way.
        s.font_family = Some("Fira Code".into());
        assert_eq!(theme_font(&s, true, installed), Some("Fira Code"));
        assert_eq!(theme_font(&s, false, installed), Some("Fira Code"));
        // No suggestion: only the own font counts.
        let mut plain = gallery_scheme();
        plain.suggested_font = None;
        assert_eq!(theme_font(&plain, true, installed), None);
    }
}
