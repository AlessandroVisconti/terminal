//! Stylistic sets that only make ligatures, read from a font's `GSUB` table, so ligatures a font
//! keeps in a stylistic set (Geist Mono's `ss11`) show without turning on its letter alternates.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

use iced::advanced::graphics::text::{cosmic_text::fontdb, font_system};
use iced::font::{Family, Stretch, Style, Weight};
use ttf_parser::gsub::SubstitutionSubtable;
use ttf_parser::opentype_layout::{
    ChainedContextLookup, ContextLookup, Coverage, LayoutTable, SequenceLookupRecord,
};
use ttf_parser::{Face, GlyphId};

/// The stylistic sets (`ss01` to `ss20`) of the face at `index` in `data` that only make
/// ligatures, sorted. Empty when the font cannot be read or has no `GSUB` table.
///
/// A set qualifies when it substitutes something and every substitution it reaches, directly or
/// through context rules, joins characters: a ligature entry has two or more components, and a
/// single substitution produces either a blank spacer glyph or a symbol whose ink reaches more
/// than a third of an advance outside its own cell (how monospace fonts draw ligatures across the
/// spacers before them). A restyled character stays inside its cell, so it disqualifies the set.
pub fn ligature_sets(data: &[u8], index: u32) -> Vec<[u8; 4]> {
    let Ok(face) = Face::parse(data, index) else {
        return Vec::new();
    };
    let Some(gsub) = face.tables().gsub else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for feature in gsub.features {
        let tag = feature.tag.to_bytes();
        if &tag[..2] != b"ss" || !tag[2].is_ascii_digit() || !tag[3].is_ascii_digit() {
            continue;
        }
        let mut check = Check {
            face: &face,
            gsub: &gsub,
            seen: HashSet::new(),
            substitutions: 0,
        };
        let joins = feature.lookup_indices.into_iter().all(|i| check.lookup(i));
        if joins && check.substitutions > 0 {
            out.push(tag);
        }
    }
    out.sort();
    out.dedup();
    out
}

struct Check<'a> {
    face: &'a Face<'a>,
    gsub: &'a LayoutTable<'a>,
    seen: HashSet<u16>,
    substitutions: usize,
}

impl Check<'_> {
    /// Whether every substitution of lookup `index` joins characters.
    fn lookup(&mut self, index: u16) -> bool {
        if !self.seen.insert(index) {
            return true;
        }
        let Some(lookup) = self.gsub.lookups.get(index) else {
            return false;
        };
        lookup
            .subtables
            .into_iter::<SubstitutionSubtable>()
            .all(|subtable| self.subtable(subtable))
    }

    fn subtable(&mut self, subtable: SubstitutionSubtable) -> bool {
        match subtable {
            SubstitutionSubtable::Single(single) => {
                let outputs: Vec<GlyphId> = match single {
                    ttf_parser::gsub::SingleSubstitution::Format1 { coverage, delta } => {
                        glyphs(coverage)
                            .map(|g| GlyphId((i32::from(g.0) + i32::from(delta)) as u16))
                            .collect()
                    }
                    ttf_parser::gsub::SingleSubstitution::Format2 { substitutes, .. } => {
                        substitutes.into_iter().collect()
                    }
                };
                self.substitutions += outputs.len();
                outputs.into_iter().all(|g| self.spacer_or_wide(g))
            }
            SubstitutionSubtable::Ligature(ligatures) => {
                ligatures.ligature_sets.into_iter().all(|set| {
                    set.into_iter().all(|ligature| {
                        self.substitutions += 1;
                        !ligature.components.is_empty()
                    })
                })
            }
            SubstitutionSubtable::Context(context) => {
                let records = context_records(context);
                records
                    .into_iter()
                    .all(|r| self.lookup(r.lookup_list_index))
            }
            SubstitutionSubtable::ChainContext(context) => {
                let records = chained_records(context);
                records
                    .into_iter()
                    .all(|r| self.lookup(r.lookup_list_index))
            }
            _ => false,
        }
    }

    /// A blank glyph (no ink), or one whose ink reaches more than a third of an advance outside
    /// its cell. (Geist Mono's `|=` reaches 0.485 of an advance; letters stay inside.)
    fn spacer_or_wide(&self, glyph: GlyphId) -> bool {
        let Some(bbox) = self.face.glyph_bounding_box(glyph) else {
            return true;
        };
        let advance = i32::from(self.face.glyph_hor_advance(glyph).unwrap_or(0)).max(1);
        i32::from(bbox.x_min) < -advance / 3 || i32::from(bbox.x_max) > advance + advance / 3
    }
}

fn glyphs(coverage: Coverage) -> Box<dyn Iterator<Item = GlyphId> + '_> {
    match coverage {
        Coverage::Format1 { glyphs } => Box::new(glyphs.into_iter()),
        Coverage::Format2 { records } => Box::new(
            records
                .into_iter()
                .flat_map(|r| (r.start.0..=r.end.0).map(GlyphId)),
        ),
    }
}

fn context_records(context: ContextLookup) -> Vec<SequenceLookupRecord> {
    match context {
        ContextLookup::Format1 { sets, .. } | ContextLookup::Format2 { sets, .. } => sets
            .into_iter()
            .flat_map(|set| set.into_iter().flat_map(|rule| rule.lookups.into_iter()))
            .collect(),
        ContextLookup::Format3 { lookups, .. } => lookups.into_iter().collect(),
    }
}

fn chained_records(context: ChainedContextLookup) -> Vec<SequenceLookupRecord> {
    match context {
        ChainedContextLookup::Format1 { sets, .. } | ChainedContextLookup::Format2 { sets, .. } => {
            sets.into_iter()
                .flat_map(|set| set.into_iter().flat_map(|rule| rule.lookups.into_iter()))
                .collect()
        }
        ChainedContextLookup::Format3 { lookups, .. } => lookups.into_iter().collect(),
    }
}

/// Detected sets per face of the font system.
type FaceSets = HashMap<fontdb::ID, Arc<[[u8; 4]]>>;

/// The ligature-only stylistic sets of the face `font` resolves to in iced's font system,
/// cached per face. Empty when no face matches.
pub fn detected_sets(font: iced::Font) -> Arc<[[u8; 4]]> {
    static CACHE: OnceLock<Mutex<FaceSets>> = OnceLock::new();
    let Ok(mut system) = font_system().write() else {
        return Arc::from([]);
    };
    let db = system.raw().db();
    let family = match font.family {
        Family::Name(name) => fontdb::Family::Name(name),
        Family::Serif => fontdb::Family::Serif,
        Family::SansSerif => fontdb::Family::SansSerif,
        Family::Cursive => fontdb::Family::Cursive,
        Family::Fantasy => fontdb::Family::Fantasy,
        Family::Monospace => fontdb::Family::Monospace,
    };
    let query = fontdb::Query {
        families: &[family],
        weight: fontdb::Weight(weight(font.weight)),
        stretch: stretch(font.stretch),
        style: match font.style {
            Style::Normal => fontdb::Style::Normal,
            Style::Italic => fontdb::Style::Italic,
            Style::Oblique => fontdb::Style::Oblique,
        },
    };
    let Some(id) = db.query(&query) else {
        return Arc::from([]);
    };
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    cache
        .entry(id)
        .or_insert_with(|| {
            db.with_face_data(id, ligature_sets)
                .unwrap_or_default()
                .into()
        })
        .clone()
}

fn weight(w: Weight) -> u16 {
    match w {
        Weight::Thin => 100,
        Weight::ExtraLight => 200,
        Weight::Light => 300,
        Weight::Normal => 400,
        Weight::Medium => 500,
        Weight::Semibold => 600,
        Weight::Bold => 700,
        Weight::ExtraBold => 800,
        Weight::Black => 900,
    }
}

fn stretch(s: Stretch) -> fontdb::Stretch {
    match s {
        Stretch::UltraCondensed => fontdb::Stretch::UltraCondensed,
        Stretch::ExtraCondensed => fontdb::Stretch::ExtraCondensed,
        Stretch::Condensed => fontdb::Stretch::Condensed,
        Stretch::SemiCondensed => fontdb::Stretch::SemiCondensed,
        Stretch::Normal => fontdb::Stretch::Normal,
        Stretch::SemiExpanded => fontdb::Stretch::SemiExpanded,
        Stretch::Expanded => fontdb::Stretch::Expanded,
        Stretch::ExtraExpanded => fontdb::Stretch::ExtraExpanded,
        Stretch::UltraExpanded => fontdb::Stretch::UltraExpanded,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const GEIST: &[u8] = include_bytes!("../../fbterm/assets/fonts/GeistMono-Regular.ttf");
    const GEIST_BOLD: &[u8] = include_bytes!("../../fbterm/assets/fonts/GeistMono-Bold.ttf");

    #[test]
    fn geist_mono_regular_selects_ss11_only() {
        assert_eq!(ligature_sets(GEIST, 0), [*b"ss11"]);
    }

    #[test]
    fn geist_mono_bold_selects_ss11_only() {
        assert_eq!(ligature_sets(GEIST_BOLD, 0), [*b"ss11"]);
    }

    #[test]
    fn unparsable_font_selects_nothing() {
        assert!(ligature_sets(b"not a font", 0).is_empty());
        assert!(ligature_sets(&GEIST[..64], 0).is_empty());
    }

    #[test]
    fn detected_sets_resolves_by_family() {
        iced::advanced::graphics::text::font_system()
            .write()
            .expect("font system")
            .load_font(std::borrow::Cow::Borrowed(GEIST));
        assert!(detected_sets(iced::Font::with_name("Geist Mono")).contains(b"ss11"));
        assert!(detected_sets(iced::Font::with_name("No Such Font 123")).is_empty());
    }
}
