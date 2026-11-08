//! Stylistic sets that only make ligatures, read from a font's `GSUB` table, so ligatures a font
//! keeps in a stylistic set (Geist Mono's `ss11`) show without turning on its letter alternates.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

use iced::advanced::graphics::text::{cosmic_text::fontdb, font_system};
use iced::font::{Family, Stretch, Style, Weight};
use skrifa::instance::{LocationRef, Size};
use skrifa::metrics::GlyphMetrics;
use skrifa::outline::OutlineGlyphCollection;
use skrifa::raw::tables::gsub::{Gsub, SingleSubst, SubstitutionSubtables};
use skrifa::raw::tables::layout::{ChainedSequenceContext, SequenceContext, SequenceLookupRecord};
use skrifa::raw::{FontRef, TableProvider};
use skrifa::{GlyphId, MetadataProvider};

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
    let tags: Vec<[u8; 4]> = features
        .feature_records()
        .iter()
        .map(|r| r.feature_tag().to_be_bytes())
        .collect();
    if has_default_ligatures(&tags) {
        return Vec::new();
    }
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
        let mut check = Check::new(&font, &gsub, &metrics);
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

/// Whether the font has standard ligature features of its own (`liga`, `calt`, `clig`). Such a
/// font shows its ligatures by default, so no stylistic set is added: a set there would restyle
/// the ligatures it already draws, and the font would look different from its default.
fn has_default_ligatures(tags: &[[u8; 4]]) -> bool {
    tags.iter()
        .any(|t| matches!(t, b"liga" | b"calt" | b"clig"))
}

struct Check<'a> {
    gsub: &'a Gsub<'a>,
    metrics: &'a GlyphMetrics<'a>,
    outlines: OutlineGlyphCollection<'a>,
    glyph_count: u32,
    seen: HashSet<u16>,
    substitutions: usize,
}

impl<'a> Check<'a> {
    fn new(font: &FontRef<'a>, gsub: &'a Gsub<'a>, metrics: &'a GlyphMetrics<'a>) -> Self {
        Self {
            gsub,
            metrics,
            outlines: font.outline_glyphs(),
            glyph_count: font.maxp().map_or(0, |m| u32::from(m.num_glyphs())),
            seen: HashSet::new(),
            substitutions: 0,
        }
    }

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
            // No bounds: blank only for a real glyph with an (empty) outline. A glyph that cannot
            // be read, or a font without outlines, gives no evidence, so it disqualifies the set.
            return glyph.to_u32() < self.glyph_count && self.outlines.get(glyph).is_some();
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
    let Some(id) = face_for(font) else {
        return Arc::from([]);
    };
    if let Some(sets) = lock(cache()).get(&id) {
        return sets.clone();
    }
    // Only the face's source is taken under the font system's lock; reading and parsing the
    // font happen after it is released, so drawing elsewhere never waits on a large font.
    let source = lock_system().raw().db().face_source(id);
    let sets: Arc<[[u8; 4]]> = match source {
        Some((fontdb::Source::Binary(data), index))
        | Some((fontdb::Source::SharedFile(_, data), index)) => {
            ligature_sets((*data).as_ref(), index).into()
        }
        Some((fontdb::Source::File(path), index)) => std::fs::read(path)
            .map(|data| ligature_sets(&data, index))
            .unwrap_or_default()
            .into(),
        None => Arc::from([]),
    };
    lock(cache()).insert(id, sets.clone());
    sets
}

/// Detects the ligature-only sets of `fonts` (each in its regular, bold, italic and bold italic
/// face) on a background thread, so the first frame drawn with them finds the answer cached.
/// Returns the thread, or `None` when it could not be started (drawing then detects as it goes).
pub fn warm_ligature_sets(fonts: Vec<iced::Font>) -> Option<std::thread::JoinHandle<()>> {
    std::thread::Builder::new()
        .name("fbterm-ligature-sets".into())
        .spawn(move || {
            for font in fonts {
                for (weight, style) in [
                    (font.weight, font.style),
                    (Weight::Bold, font.style),
                    (font.weight, Style::Italic),
                    (Weight::Bold, Style::Italic),
                ] {
                    detected_sets(iced::Font {
                        weight,
                        style,
                        ..font
                    });
                }
            }
        })
        .ok()
}

/// The cached sets of `font`'s face, if detected already.
#[cfg(test)]
fn cached(font: iced::Font) -> Option<Arc<[[u8; 4]]>> {
    face_for(font).and_then(|id| lock(cache()).get(&id).cloned())
}

fn cache() -> &'static Mutex<FaceSets> {
    static CACHE: OnceLock<Mutex<FaceSets>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

fn lock_system() -> std::sync::RwLockWriteGuard<'static, iced::advanced::graphics::text::FontSystem>
{
    font_system().write().unwrap_or_else(|e| e.into_inner())
}

/// The face of iced's font system that `font` resolves to (family, weight, stretch, style).
fn face_for(font: iced::Font) -> Option<fontdb::ID> {
    let mut system = lock_system();
    let family = match font.family {
        Family::Name(name) => fontdb::Family::Name(name),
        Family::Serif => fontdb::Family::Serif,
        Family::SansSerif => fontdb::Family::SansSerif,
        Family::Cursive => fontdb::Family::Cursive,
        Family::Fantasy => fontdb::Family::Fantasy,
        Family::Monospace => fontdb::Family::Monospace,
    };
    system.raw().db().query(&fontdb::Query {
        families: &[family],
        weight: fontdb::Weight(weight(font.weight)),
        stretch: stretch(font.stretch),
        style: match font.style {
            Style::Normal => fontdb::Style::Normal,
            Style::Italic => fontdb::Style::Italic,
            Style::Oblique => fontdb::Style::Oblique,
        },
    })
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
    fn an_unreadable_glyph_is_not_a_spacer() {
        let font = FontRef::new(GEIST).unwrap();
        let gsub = font.gsub().unwrap();
        let metrics = GlyphMetrics::new(&font, Size::unscaled(), LocationRef::default());
        let check = Check::new(&font, &gsub, &metrics);
        assert!(
            !check.spacer_or_wide(GlyphId::new(60_000)),
            "past the glyph count"
        );
    }

    #[test]
    fn fonts_with_default_ligatures_get_no_extra_sets() {
        assert!(has_default_ligatures(&[*b"ss01", *b"calt"]));
        assert!(has_default_ligatures(&[*b"liga"]));
        assert!(has_default_ligatures(&[*b"clig", *b"ss02"]));
        assert!(
            !has_default_ligatures(&[*b"ss11", *b"ccmp", *b"case"]),
            "Geist Mono's kind"
        );
    }

    #[test]
    fn warming_fills_the_cache_for_every_style() {
        let mut system = iced::advanced::graphics::text::font_system()
            .write()
            .expect("font system");
        system.load_font(std::borrow::Cow::Borrowed(GEIST));
        system.load_font(std::borrow::Cow::Borrowed(GEIST_BOLD));
        drop(system);
        let regular = iced::Font::with_name("Geist Mono");
        warm_ligature_sets(vec![regular])
            .expect("thread")
            .join()
            .expect("warming thread");
        let bold = iced::Font {
            weight: iced::font::Weight::Bold,
            ..regular
        };
        assert!(cached(regular).is_some_and(|s| s.contains(b"ss11")));
        assert!(cached(bold).is_some_and(|s| s.contains(b"ss11")));
    }

    #[test]
    fn bold_resolves_to_its_own_face() {
        let mut system = iced::advanced::graphics::text::font_system()
            .write()
            .expect("font system");
        system.load_font(std::borrow::Cow::Borrowed(GEIST));
        system.load_font(std::borrow::Cow::Borrowed(GEIST_BOLD));
        drop(system);
        let regular = iced::Font::with_name("Geist Mono");
        let bold = iced::Font {
            weight: iced::font::Weight::Bold,
            ..regular
        };
        assert_ne!(face_for(regular), face_for(bold));
        assert!(face_for(bold).is_some());
        assert!(detected_sets(bold).contains(b"ss11"));
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
