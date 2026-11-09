//! The OpenType features a text run is shaped with.

use std::sync::Arc;

use fbterm_config::font_features::FeatureSetting;

/// The font features of a terminal: whether ligatures are on, then extra entries from the
/// `font_features` setting.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Default)]
pub struct FontFeatures {
    pub ligatures: bool,
    pub extra: Arc<[FeatureSetting]>,
}

const ON: [&[u8; 4]; 3] = [b"liga", b"calt", b"clig"];
const OFF: [&[u8; 4]; 4] = [b"liga", b"calt", b"clig", b"dlig"];

/// The features for one run, in order (shaping lets a later entry for a tag win): with ligatures
/// on, the standard ligature features and the font's ligature-only stylistic sets (`detected`),
/// except for a cell drawn on its own (`alone`), where no ligature can form; with them off, every
/// ligature feature off. Then the extra entries.
pub(crate) fn run_features(f: &FontFeatures, detected: &[[u8; 4]], alone: bool) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = if f.ligatures {
        let sets = if alone { &[][..] } else { detected };
        ON.iter()
            .map(|t| (u32::from_be_bytes(**t), 1))
            .chain(sets.iter().map(|t| (u32::from_be_bytes(*t), 1)))
            .collect()
    } else {
        OFF.iter().map(|t| (u32::from_be_bytes(**t), 0)).collect()
    };
    out.extend(f.extra.iter().map(|e| (u32::from_be_bytes(e.tag), e.value)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use fbterm_config::font_features::FeatureSetting;

    fn tag(t: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*t)
    }

    fn on(extra: &[FeatureSetting]) -> FontFeatures {
        FontFeatures {
            ligatures: true,
            extra: extra.into(),
        }
    }

    #[test]
    fn on_turns_on_standard_ligatures_and_detected_sets() {
        let got = run_features(&on(&[]), &[*b"ss11"], false);
        assert_eq!(
            got,
            [
                (tag(b"liga"), 1),
                (tag(b"calt"), 1),
                (tag(b"clig"), 1),
                (tag(b"ss11"), 1)
            ]
        );
    }

    #[test]
    fn cells_drawn_alone_never_get_detected_sets() {
        let got = run_features(&on(&[]), &[*b"ss11"], true);
        assert!(!got.iter().any(|&(t, _)| t == tag(b"ss11")));
    }

    #[test]
    fn off_turns_every_ligature_feature_off() {
        let off = FontFeatures {
            ligatures: false,
            extra: [].into(),
        };
        let got = run_features(&off, &[*b"ss11"], false);
        assert_eq!(
            got,
            [
                (tag(b"liga"), 0),
                (tag(b"calt"), 0),
                (tag(b"clig"), 0),
                (tag(b"dlig"), 0)
            ]
        );
    }

    #[test]
    fn extra_features_come_last_and_win() {
        let extra = [
            FeatureSetting {
                tag: *b"calt",
                value: 0,
            },
            FeatureSetting {
                tag: *b"ss02",
                value: 1,
            },
        ];
        let got = run_features(&on(&extra), &[], false);
        assert_eq!(got.last(), Some(&(tag(b"ss02"), 1)));
        let calt: Vec<u32> = got
            .iter()
            .filter(|f| f.0 == tag(b"calt"))
            .map(|f| f.1)
            .collect();
        assert_eq!(
            calt,
            [1, 0],
            "the later entry wins when shaping applies them in order"
        );
    }
}
