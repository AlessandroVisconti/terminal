//! OpenType feature entries from the `font_features` setting.

/// One OpenType feature: a four-letter tag and its value (0 off, 1 on, more for variants).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FeatureSetting {
    pub tag: [u8; 4],
    pub value: u32,
}

/// Parses `font_features` entries: a four-letter tag (ASCII letters and digits), optionally
/// prefixed with `+` (on) or `-` (off), or followed by `=<number>`. Returns the features in
/// order and the entries that do not parse, verbatim.
pub fn parse_features(entries: &[String]) -> (Vec<FeatureSetting>, Vec<String>) {
    let mut parsed = Vec::new();
    let mut rejected = Vec::new();
    for entry in entries {
        match parse_one(entry) {
            Some(f) => parsed.push(f),
            None => rejected.push(entry.clone()),
        }
    }
    (parsed, rejected)
}

fn parse_one(entry: &str) -> Option<FeatureSetting> {
    let (rest, prefixed) = match entry.as_bytes().first() {
        Some(b'+') => (&entry[1..], Some(1)),
        Some(b'-') => (&entry[1..], Some(0)),
        _ => (entry, None),
    };
    let (tag, value) = match rest.split_once('=') {
        Some(_) if prefixed.is_some() => return None,
        Some((tag, value)) => (tag, value.parse().ok()?),
        None => (rest, prefixed.unwrap_or(1)),
    };
    let tag: [u8; 4] = tag.as_bytes().try_into().ok()?;
    tag.iter()
        .all(u8::is_ascii_alphanumeric)
        .then_some(FeatureSetting { tag, value })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_parse_and_bad_ones_are_returned() {
        let entries: Vec<String> = [
            "ss11", "+calt", "-liga", "cv01=2", "toolong", "x", "ss1=", "",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let (parsed, rejected) = parse_features(&entries);
        let want = [(*b"ss11", 1), (*b"calt", 1), (*b"liga", 0), (*b"cv01", 2)];
        let got: Vec<([u8; 4], u32)> = parsed.iter().map(|f| (f.tag, f.value)).collect();
        assert_eq!(got, want);
        assert_eq!(rejected, ["toolong", "x", "ss1=", ""]);
    }

    #[test]
    fn a_tag_is_four_ascii_alphanumerics() {
        let entries: Vec<String> = ["ss-1", "ä123", "+-ss1", "abcd=x"]
            .into_iter()
            .map(String::from)
            .collect();
        let (parsed, rejected) = parse_features(&entries);
        assert!(parsed.is_empty());
        assert_eq!(rejected.len(), 4);
    }
}
