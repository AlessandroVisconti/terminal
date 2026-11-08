//! Grid text shaped with its OpenType features through cosmic-text, kept across frames.

use std::collections::HashMap;
use std::sync::Arc;

use iced::Font;
use iced::advanced::graphics::text::cosmic_text::{self, Align, Buffer, FeatureTag, Shaping, Wrap};
use iced::advanced::graphics::text::{font_system, to_attributes};

/// What a shaped buffer depends on. `join` is for runs of several cells: a cell drawn on its
/// own (a grapheme cluster) must reach the shaper unchanged.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct ShapeKey {
    text: String,
    font: Font,
    size: u32,
    line_height: u32,
    width: u32,
    features: Vec<(u32, u32)>,
    join: bool,
}

impl ShapeKey {
    pub(crate) fn new(
        text: &str,
        font: Font,
        size: f32,
        line_height: f32,
        width: f32,
        features: Vec<(u32, u32)>,
        join: bool,
    ) -> Self {
        Self {
            text: text.to_owned(),
            font,
            size: size.to_bits(),
            line_height: line_height.to_bits(),
            width: width.to_bits(),
            features,
            join,
        }
    }
}

/// Shaped buffers by what they depend on. The renderer only holds weak references to the
/// buffers it draws, until the frame is presented, so a buffer stays here at least until the
/// frame after the one that last used it.
#[derive(Default)]
pub(crate) struct ShapeCache {
    entries: HashMap<ShapeKey, (Arc<Buffer>, u64)>,
    frame: u64,
    shaped: usize,
    /// Ligature-only stylistic sets per font (face lookups need the font system's lock).
    sets: HashMap<Font, Arc<[[u8; 4]]>>,
}

impl ShapeCache {
    /// The ligature-only stylistic sets of `font`'s face, looked up once per font.
    pub(crate) fn sets(&mut self, font: Font) -> Arc<[[u8; 4]]> {
        self.sets
            .entry(font)
            .or_insert_with(|| crate::ligature_sets::detected_sets(font))
            .clone()
    }

    /// The buffer for `key`, shaped now if no frame since the previous one used it.
    pub(crate) fn buffer(&mut self, key: ShapeKey) -> Arc<Buffer> {
        let frame = self.frame;
        if let Some((buffer, last)) = self.entries.get_mut(&key) {
            *last = frame;
            return buffer.clone();
        }
        let buffer = Arc::new(shape(&key));
        self.shaped += 1;
        self.entries.insert(key, (buffer.clone(), frame));
        buffer
    }

    /// Ends a frame: entries unused in the frame that just ended and the one before are dropped.
    pub(crate) fn end_frame(&mut self) {
        self.frame += 1;
        let frame = self.frame;
        self.entries.retain(|_, (_, last)| *last + 1 >= frame);
    }

    /// How many buffers were shaped so far (tests).
    #[cfg(test)]
    pub(crate) fn shaped_count(&self) -> usize {
        self.shaped
    }
}

/// `text` with a word joiner (U+2060) between adjacent visible characters. cosmic-text shapes
/// each line-break segment on its own, and Unicode allows a break after a hyphen, so `->` would
/// reach the shaper as `-` and `>` and never form a ligature. A word joiner forbids the break;
/// shapers skip it when matching ligature rules and draw it with no width.
fn joined(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 4);
    let mut prev_visible = false;
    for c in text.chars() {
        let visible = c != ' ';
        if prev_visible && visible {
            out.push('\u{2060}');
        }
        out.push(c);
        prev_visible = visible;
    }
    out
}

fn shape(key: &ShapeKey) -> Buffer {
    let size = f32::from_bits(key.size);
    let line_height = f32::from_bits(key.line_height);
    let mut features = cosmic_text::FontFeatures::new();
    for &(tag, value) in &key.features {
        features.set(FeatureTag::new(&tag.to_be_bytes()), value);
    }
    let attrs = to_attributes(key.font).font_features(features);
    let mut system = font_system().write().unwrap_or_else(|e| e.into_inner());
    let fs = system.raw();
    let mut buffer = Buffer::new(fs, cosmic_text::Metrics::new(size, line_height));
    buffer.set_wrap(fs, Wrap::None);
    buffer.set_size(fs, Some(f32::from_bits(key.width)), Some(line_height));
    let text = if key.join {
        joined(&key.text)
    } else {
        key.text.clone()
    };
    // Left-aligned whatever the direction: cosmic-text right-aligns a right-to-left line by
    // default, which would push a Hebrew or Arabic cell into the next one.
    buffer.set_text(fs, &text, &attrs, Shaping::Advanced, Some(Align::Left));
    buffer.shape_until_scroll(fs, false);
    buffer
}

#[cfg(test)]
mod tests {
    use super::*;

    const GEIST: &[u8] = include_bytes!("../../../fbterm/assets/fonts/GeistMono-Regular.ttf");

    fn load_geist() {
        iced::advanced::graphics::text::font_system()
            .write()
            .expect("font system")
            .load_font(std::borrow::Cow::Borrowed(GEIST));
    }

    fn key(text: &str, features: Vec<(u32, u32)>) -> ShapeKey {
        ShapeKey::new(
            text,
            Font::with_name("Geist Mono"),
            13.0,
            16.0,
            8.0 * 3.0,
            features,
            true,
        )
    }

    fn tag(t: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*t)
    }

    fn glyph_ids(buffer: &Buffer) -> Vec<u16> {
        buffer
            .layout_runs()
            .flat_map(|r| r.glyphs.iter().map(|g| g.glyph_id))
            .collect()
    }

    #[test]
    fn geist_ss11_shapes_an_arrow() {
        load_geist();
        let font = skrifa::FontRef::new(GEIST).unwrap();
        let arrow = skrifa::GlyphNames::new(&font)
            .iter()
            .find(|(_, name)| name.as_str() == "hyphen_greater.liga")
            .map(|(glyph, _)| glyph.to_u32() as u16)
            .unwrap();
        let mut cache = ShapeCache::default();
        let on = vec![(tag(b"liga"), 1), (tag(b"calt"), 1), (tag(b"ss11"), 1)];
        assert!(glyph_ids(&cache.buffer(key("->", on.clone()))).contains(&arrow));
        assert!(glyph_ids(&cache.buffer(key("a->b", on))).contains(&arrow));
        let off = vec![(tag(b"liga"), 0), (tag(b"calt"), 0)];
        assert!(!glyph_ids(&cache.buffer(key("->", off))).contains(&arrow));
    }

    #[test]
    fn right_to_left_text_starts_at_its_cell() {
        load_geist();
        let mut cache = ShapeCache::default();
        for text in ["\u{5e9}", "\u{627}"] {
            let key = ShapeKey::new(
                text,
                Font::with_name("Geist Mono"),
                13.0,
                16.0,
                16.0,
                vec![],
                false,
            );
            let buffer = cache.buffer(key);
            let x = buffer
                .layout_runs()
                .flat_map(|r| r.glyphs.iter().map(|g| g.x))
                .fold(f32::MAX, f32::min);
            assert!(x < 1.0, "{text:?} starts at x={x}");
        }
    }

    #[test]
    fn cache_reuses_unchanged_runs() {
        load_geist();
        let mut cache = ShapeCache::default();
        cache.buffer(key("abc", vec![]));
        cache.end_frame();
        cache.buffer(key("abc", vec![]));
        assert_eq!(cache.shaped_count(), 1);
        cache.end_frame();
        cache.end_frame();
        cache.end_frame();
        cache.buffer(key("abc", vec![]));
        assert_eq!(
            cache.shaped_count(),
            2,
            "an entry unused for a whole frame is dropped"
        );
    }

    #[test]
    fn new_features_reshape() {
        load_geist();
        let mut cache = ShapeCache::default();
        cache.buffer(key("->", vec![(tag(b"ss11"), 1)]));
        cache.buffer(key("->", vec![(tag(b"ss11"), 0)]));
        assert_eq!(cache.shaped_count(), 2);
    }
}
