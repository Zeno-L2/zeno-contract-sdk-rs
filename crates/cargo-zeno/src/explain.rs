//! `cargo zeno explain` over the embedded error catalogue.

const CATALOGUE: &str = include_str!("../../../docs/errors.md");

/// The catalogue section of `code`, from its `## ` heading line up to
/// the next heading. The code is matched after uppercasing.
pub fn section(code: &str) -> Option<&'static str> {
    let heading = format!("## {} ", code.to_ascii_uppercase());
    let mut start = None;
    let mut offset = 0;
    for line in CATALOGUE.split_inclusive('\n') {
        if let Some(from) = start {
            if line.starts_with("## ") {
                return Some(CATALOGUE[from..offset].trim_end());
            }
        } else if line.starts_with(&heading) {
            start = Some(offset);
        }
        offset += line.len();
    }
    start.map(|from| CATALOGUE[from..].trim_end())
}

/// The text after `Fix: ` in the section of `code`. Panics when the
/// code has no section or no fix line.
pub fn fix(code: &str) -> &'static str {
    section(code)
        .and_then(|s| s.lines().find_map(|l| l.strip_prefix("Fix: ")))
        .unwrap_or_else(|| panic!("docs/errors.md has no fix line for {code}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::path::Path;

    fn codes_in(text: &str, out: &mut BTreeSet<String>) {
        let b = text.as_bytes();
        for i in 0..b.len().saturating_sub(4) {
            if b[i] == b'Z' && b[i + 1..i + 5].iter().all(u8::is_ascii_digit) {
                out.insert(text[i..i + 5].to_string());
            }
        }
    }

    fn scan(dir: &Path, out: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                scan(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                codes_in(&std::fs::read_to_string(&path).unwrap(), out);
            }
        }
    }

    #[test]
    fn the_catalogue_and_the_sources_hold_the_same_codes() {
        let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let mut in_source = BTreeSet::new();
        for entry in std::fs::read_dir(&crates).unwrap() {
            let src = entry.unwrap().path().join("src");
            if src.is_dir() {
                scan(&src, &mut in_source);
            }
        }
        let mut in_catalogue = BTreeSet::new();
        for line in CATALOGUE.lines() {
            if let Some(rest) = line.strip_prefix("## ") {
                codes_in(&rest[..rest.len().min(5)], &mut in_catalogue);
            }
        }
        let missing_section: Vec<_> = in_source.difference(&in_catalogue).collect();
        let unused_section: Vec<_> = in_catalogue.difference(&in_source).collect();
        assert!(
            missing_section.is_empty() && unused_section.is_empty(),
            "in source without a section: {missing_section:?}; sections without source: {unused_section:?}"
        );
    }

    #[test]
    fn section_returns_one_heading_and_matches_any_case() {
        let s = section("Z0101").unwrap();
        assert!(s.starts_with("## Z0101 "));
        assert!(!s[3..].contains("\n## "));
        assert_eq!(section("z0101"), Some(s));
    }

    #[test]
    fn fix_reads_the_fix_line() {
        assert_eq!(
            fix("Z0204"),
            "run `cargo generate-lockfile` and commit the file."
        );
        assert!(fix("Z0301").starts_with("remove f32/f64 use"));
    }

    #[test]
    fn every_section_has_a_cause_and_a_fix_and_the_contract_codes_are_unbroken() {
        let mut headings = 0;
        for line in CATALOGUE.lines().filter(|l| l.starts_with("## ")) {
            let code = &line[3..8];
            let s = section(code).unwrap();
            assert!(s.lines().any(|l| l.starts_with("Cause: ")), "{code}");
            assert!(s.lines().any(|l| l.starts_with("Fix: ")), "{code}");
            headings += 1;
        }
        assert!(headings >= 22);
        for n in 101..=122 {
            assert!(section(&format!("Z0{n}")).is_some(), "Z0{n}");
        }
    }

    #[test]
    fn an_unknown_code_has_no_section() {
        assert_eq!(section(&format!("Z{}", 9999)), None);
    }
}
