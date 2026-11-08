//! Stylistic sets that only make ligatures, read from a font's `GSUB` table, so ligatures a font
//! keeps in a stylistic set (Geist Mono's `ss11`) show without turning on its letter alternates.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

use iced::advanced::graphics::text::{cosmic_text::fontdb, font_system};
use iced::font::{Family, Stretch, Style, Weight};
use skrifa::GlyphId;
use skrifa::instance::{LocationRef, Size};
use skrifa::metrics::GlyphMetrics;
use skrifa::raw::tables::gsub::{Gsub, SingleSubst, SubstitutionSubtables};
use skrifa::raw::tables::layout::{ChainedSequenceContext, SequenceContext, SequenceLookupRecord};
use skrifa::raw::{FontRef, TableProvider};

/// The stylistic sets (`ss01` to `ss20`) of the face at `index` in `data` that only make
/// ligatures, sorted. Empty when the font cannot be read or has no `GSUB` table.
///
/// A set qualifies when it substitutes something and every substitution it reaches, directly or
/// through context rules, joins characters: a ligature entry has two or more components, and a
/// single substitution produces either a blank spacer glyph or a symbol whose ink reaches more
/// than a third of an advance outside its own cell (how monospace fonts draw ligatures across the
/// spacers before them). A restyled character stays inside its cell, so it disqualifies the set.
pub fn ligature_sets(data: &[u8], index: u32) -> Vec<[u8; 4]> {
    let Ok(font) = FontRef::from_index(data, index) else {
        return Vec::new();
    };
    let Ok(gsub) = font.gsub() else {
        return Vec::new();
    };
    let Ok(features) = gsub.feature_list() else {
        return Vec::new();
    };
    let metrics = GlyphMetrics::new(&font, Size::unscaled(), LocationRef::default());
    let mut out = Vec::new();
    for record in features.feature_records() {
        let tag = record.feature_tag().to_be_bytes();
        if &tag[..2] != b"ss" || !tag[2].is_ascii_digit() || !tag[3].is_ascii_digit() {
            continue;
        }
        let Ok(feature) = record.feature(features.offset_data()) else {
            continue;
        };
        let mut check = Check {
            gsub: &gsub,
            metrics: &metrics,
            seen: HashSet::new(),
            substitutions: 0,
        };
        let joins = feature
            .lookup_list_indices()
            .iter()
            .all(|i| check.lookup(i.get()));
        if joins && check.substitutions > 0 {
            out.push(tag);
        }
    }
    out.sort();
    out.dedup();
    out
}

struct Check<'a> {
    gsub: &'a Gsub<'a>,
    metrics: &'a GlyphMetrics<'a>,
    seen: HashSet<u16>,
    substitutions: usize,
}

impl Check<'_> {
    /// Whether every substitution of lookup `index` joins characters.
    fn lookup(&mut self, index: u16) -> bool {
        if !self.seen.insert(index) {
            return true;
        }
        let Some(subtables) = self
            .gsub
            .lookup_list()
            .ok()
            .and_then(|list| list.lookups().get(usize::from(index)).ok())
            .and_then(|lookup| lookup.subtables().ok())
        else {
            return false;
        };
        match subtables {
            SubstitutionSubtables::Single(tables) => tables.iter().all(|t| match t {
                Ok(single) => self.single(&single),
                Err(_) => false,
            }),
            SubstitutionSubtables::Ligature(tables) => tables.iter().all(|t| {
                let Ok(table) = t else { return false };
                table.ligature_sets().iter().all(|set| {
                    let Ok(set) = set else { return false };
                    set.ligatures().iter().all(|ligature| {
                        self.substitutions += 1;
                        ligature.is_ok_and(|l| !l.component_glyph_ids().is_empty())
                    })
                })
            }),
            SubstitutionSubtables::Contextual(tables) => tables.iter().all(|t| match t {
                Ok(context) => context_records(&context).is_some_and(|records| {
                    records
                        .into_iter()
                        .all(|r| self.lookup(r.lookup_list_index()))
                }),
                Err(_) => false,
            }),
            SubstitutionSubtables::ChainContextual(tables) => tables.iter().all(|t| match t {
                Ok(context) => chained_records(&context).is_some_and(|records| {
                    records
                        .into_iter()
                        .all(|r| self.lookup(r.lookup_list_index()))
                }),
                Err(_) => false,
            }),
            SubstitutionSubtables::EmptyExtension => true,
            _ => false,
        }
    }

    fn single(&mut self, single: &SingleSubst) -> bool {
        let outputs: Vec<GlyphId> = match single {
            SingleSubst::Format1(t) => {
                let Ok(coverage) = t.coverage() else {
                    return false;
                };
                let delta = i32::from(t.delta_glyph_id());
                coverage
                    .iter()
                    .map(|g| GlyphId::new(((i32::from(g.to_u16()) + delta) & 0xFFFF) as u32))
                    .collect()
            }
            SingleSubst::Format2(t) => t
                .substitute_glyph_ids()
                .iter()
                .map(|g| g.get().into())
                .collect(),
        };
        self.substitutions += outputs.len();
        outputs.into_iter().all(|g| self.spacer_or_wide(g))
    }

    /// A blank glyph (no ink), or one whose ink reaches more than a third of an advance outside
    /// its cell. (Geist Mono's `|=` reaches 0.485 of an advance; letters stay inside.)
    fn spacer_or_wide(&self, glyph: GlyphId) -> bool {
        let Some(bounds) = self.metrics.bounds(glyph) else {
            return true;
        };
        if bounds.x_max <= bounds.x_min {
            return true;
        }
        let advance = self.metrics.advance_width(glyph).unwrap_or(0.0).max(1.0);
        bounds.x_min < -advance / 3.0 || bounds.x_max > advance + advance / 3.0
    }
}

fn context_records(context: &SequenceContext) -> Option<Vec<SequenceLookupRecord>> {
    let mut out = Vec::new();
    match context {
        SequenceContext::Format1(t) => {
            for set in t.seq_rule_sets().iter().flatten() {
                for rule in set.ok()?.seq_rules().iter() {
                    out.extend_from_slice(rule.ok()?.seq_lookup_records());
                }
            }
        }
        SequenceContext::Format2(t) => {
            for set in t.class_seq_rule_sets().iter().flatten() {
                for rule in set.ok()?.class_seq_rules().iter() {
                    out.extend_from_slice(rule.ok()?.seq_lookup_records());
                }
            }
        }
        SequenceContext::Format3(t) => out.extend_from_slice(t.seq_lookup_records()),
    }
    Some(out)
}

fn chained_records(context: &ChainedSequenceContext) -> Option<Vec<SequenceLookupRecord>> {
    let mut out = Vec::new();
    match context {
        ChainedSequenceContext::Format1(t) => {
            for set in t.chained_seq_rule_sets().iter().flatten() {
                for rule in set.ok()?.chained_seq_rules().iter() {
                    out.extend_from_slice(rule.ok()?.seq_lookup_records());
                }
            }
        }
        ChainedSequenceContext::Format2(t) => {
            for set in t.chained_class_seq_rule_sets().iter().flatten() {
                for rule in set.ok()?.chained_class_seq_rules().iter() {
                    out.extend_from_slice(rule.ok()?.seq_lookup_records());
                }
            }
        }
        ChainedSequenceContext::Format3(t) => out.extend_from_slice(t.seq_lookup_records()),
    }
    Some(out)
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
