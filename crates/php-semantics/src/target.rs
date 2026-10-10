//! Correlation-preserving PHP target scenarios and applicability evaluation.

use serde::{Deserialize, Serialize};

use crate::Bytes;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ReleaseVersion {
    pub major: u16,
    pub minor: u16,
    pub patch: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VersionRange {
    pub min: ReleaseVersion,
    pub max: ReleaseVersion,
}

impl VersionRange {
    pub fn exact(version: ReleaseVersion) -> Self {
        Self {
            min: version,
            max: version,
        }
    }
    pub fn minor(major: u16, minor: u16) -> Self {
        Self {
            min: ReleaseVersion {
                major,
                minor,
                patch: 0,
            },
            max: ReleaseVersion {
                major,
                minor,
                patch: u16::MAX,
            },
        }
    }
    pub fn valid(self) -> bool {
        self.min <= self.max
    }
    pub fn overlaps(self, other: Self) -> bool {
        self.min <= other.max && other.min <= self.max
    }
    pub fn contains_range(self, other: Self) -> bool {
        self.min <= other.min && other.max <= self.max
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetScenario {
    pub release: VersionRange,
    pub os: Option<Bytes>,
    pub arch: Option<Bytes>,
    pub sapi: Option<Bytes>,
    pub zts: Option<bool>,
    pub debug: Option<bool>,
    /// `None` means the extension inventory is unknown for this scenario.
    pub extensions: Option<Vec<(Bytes, bool)>>,
    /// `None` means runtime settings are unknown for this scenario.
    pub ini: Option<Vec<(Bytes, Bytes)>>,
}

impl TargetScenario {
    pub fn validate(&self) -> Result<(), TargetError> {
        if !self.release.valid() {
            return Err(TargetError::InvalidVersionRange);
        }
        if let Some(extensions) = &self.extensions {
            if has_duplicate_names(extensions.iter().map(|(name, _)| name)) {
                return Err(TargetError::DuplicateExtension);
            }
        }
        if let Some(ini) = &self.ini {
            if has_duplicate_names(ini.iter().map(|(name, _)| name)) {
                return Err(TargetError::DuplicateSetting);
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetPredicate {
    pub release: Option<VersionRange>,
    pub os: Option<Bytes>,
    pub arch: Option<Bytes>,
    pub sapi: Option<Bytes>,
    pub zts: Option<bool>,
    pub debug: Option<bool>,
    pub extensions: Vec<(Bytes, bool)>,
    pub ini: Vec<(Bytes, Bytes)>,
}

impl TargetPredicate {
    pub fn any() -> Self {
        Self {
            release: None,
            os: None,
            arch: None,
            sapi: None,
            zts: None,
            debug: None,
            extensions: vec![],
            ini: vec![],
        }
    }

    pub fn validate(&self) -> Result<(), TargetError> {
        if self.release.is_some_and(|release| !release.valid()) {
            return Err(TargetError::InvalidVersionRange);
        }
        if has_duplicate_names(self.extensions.iter().map(|(name, _)| name)) {
            return Err(TargetError::DuplicateExtension);
        }
        if has_duplicate_names(self.ini.iter().map(|(name, _)| name)) {
            return Err(TargetError::DuplicateSetting);
        }
        Ok(())
    }

    pub fn test(&self, scenario: &TargetScenario) -> Applicability {
        let mut maybe = false;
        if let Some(required) = self.release {
            if !scenario.release.overlaps(required) {
                return Applicability::No;
            }
            if !required.contains_range(scenario.release) {
                maybe = true;
            }
        }
        macro_rules! fact {
            ($required:expr, $actual:expr) => {
                if let Some(required) = &$required {
                    match &$actual {
                        Some(actual) if actual == required => {}
                        Some(_) => return Applicability::No,
                        None => maybe = true,
                    }
                }
            };
        }
        fact!(self.os, scenario.os);
        fact!(self.arch, scenario.arch);
        fact!(self.sapi, scenario.sapi);
        if let Some(required) = self.zts {
            match scenario.zts {
                Some(actual) if actual == required => {}
                Some(_) => return Applicability::No,
                None => maybe = true,
            }
        }
        if let Some(required) = self.debug {
            match scenario.debug {
                Some(actual) if actual == required => {}
                Some(_) => return Applicability::No,
                None => maybe = true,
            }
        }
        for (name, required) in &self.extensions {
            match &scenario.extensions {
                None => maybe = true,
                Some(values) => {
                    match find_ascii(values.iter().map(|(key, value)| (key, value)), name) {
                        Some(actual) if actual == required => {}
                        Some(_) => return Applicability::No,
                        None if !required => {}
                        None => return Applicability::No,
                    }
                }
            }
        }
        for (name, required) in &self.ini {
            match &scenario.ini {
                None => maybe = true,
                Some(values) => {
                    match find_ascii(values.iter().map(|(key, value)| (key, value)), name) {
                        Some(actual) if actual == required => {}
                        Some(_) => return Applicability::No,
                        None => maybe = true,
                    }
                }
            }
        }
        if maybe {
            Applicability::Maybe
        } else {
            Applicability::Yes
        }
    }
}

fn fold_ascii(bytes: &[u8]) -> Vec<u8> {
    bytes.iter().map(u8::to_ascii_lowercase).collect()
}
fn find_ascii<'a, T>(
    items: impl Iterator<Item = (&'a Bytes, &'a T)>,
    name: &Bytes,
) -> Option<&'a T> {
    let key = fold_ascii(&name.0);
    items
        .filter(|(candidate, _)| fold_ascii(&candidate.0) == key)
        .map(|(_, value)| value)
        .next()
}
fn has_duplicate_names<'a>(names: impl Iterator<Item = &'a Bytes>) -> bool {
    let mut seen = Vec::<Vec<u8>>::new();
    names.into_iter().any(|name| {
        let folded = fold_ascii(&name.0);
        if seen.contains(&folded) {
            true
        } else {
            seen.push(folded);
            false
        }
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Applicability {
    Yes,
    No,
    Maybe,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetSet {
    pub scenarios: Vec<TargetScenario>,
}

impl TargetSet {
    pub fn new(scenarios: Vec<TargetScenario>) -> Result<Self, TargetError> {
        for scenario in &scenarios {
            scenario.validate()?;
        }
        Ok(Self { scenarios })
    }

    /// Preserve each correlated scenario; never construct a cross-product of
    /// release, platform, and extension facts.
    pub fn select(&self, predicate: &TargetPredicate) -> TargetSelection {
        let mut applicable = Vec::new();
        let mut uncovered = Vec::new();
        for scenario in &self.scenarios {
            match predicate.test(scenario) {
                Applicability::Yes => applicable.push((scenario.clone(), Applicability::Yes)),
                Applicability::Maybe => {
                    applicable.push((scenario.clone(), Applicability::Maybe));
                    uncovered.push(scenario.clone());
                }
                Applicability::No => uncovered.push(scenario.clone()),
            }
        }
        TargetSelection {
            applicable,
            uncovered,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetSelection {
    /// `Maybe` requires conservative fallback alongside graph evaluation.
    pub applicable: Vec<(TargetScenario, Applicability)>,
    pub uncovered: Vec<TargetScenario>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetError {
    InvalidVersionRange,
    DuplicateExtension,
    DuplicateSetting,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn known_linux(release: VersionRange) -> TargetScenario {
        TargetScenario {
            release,
            os: Some(Bytes(b"linux".to_vec())),
            arch: Some(Bytes(b"x86_64".to_vec())),
            sapi: Some(Bytes(b"cli".to_vec())),
            zts: Some(false),
            debug: Some(false),
            extensions: Some(vec![(Bytes(b"curl".to_vec()), true)]),
            ini: Some(vec![]),
        }
    }

    #[test]
    fn exact_minor_and_patch_ranges_are_distinct() {
        let minor = VersionRange::minor(8, 5);
        let exact = VersionRange::exact(ReleaseVersion {
            major: 8,
            minor: 5,
            patch: 4,
        });
        assert!(minor.contains_range(exact));
        assert!(!exact.contains_range(minor));
        assert!(!minor.overlaps(VersionRange::minor(8, 6)));
    }

    #[test]
    fn unknown_target_fact_keeps_candidate_and_uncovered_remainder() {
        let unknown = TargetScenario {
            release: VersionRange::exact(ReleaseVersion {
                major: 8,
                minor: 5,
                patch: 4,
            }),
            os: None,
            arch: None,
            sapi: Some(Bytes(b"cli".to_vec())),
            zts: None,
            debug: None,
            extensions: None,
            ini: None,
        };
        let targets = TargetSet::new(vec![unknown]).unwrap();
        let predicate = TargetPredicate {
            os: Some(Bytes(b"linux".to_vec())),
            ..TargetPredicate::any()
        };
        let selected = targets.select(&predicate);
        assert_eq!(selected.applicable.len(), 1);
        assert_eq!(selected.applicable[0].1, Applicability::Maybe);
        assert_eq!(selected.uncovered.len(), 1);
    }

    #[test]
    fn correlated_scenarios_do_not_invent_cross_product_targets() {
        let linux = known_linux(VersionRange::minor(8, 4));
        let windows = TargetScenario {
            os: Some(Bytes(b"windows".to_vec())),
            ..known_linux(VersionRange::minor(8, 5))
        };
        let targets = TargetSet::new(vec![linux.clone(), windows.clone()]).unwrap();
        let predicate = TargetPredicate {
            os: Some(Bytes(b"linux".to_vec())),
            release: Some(VersionRange::minor(8, 4)),
            ..TargetPredicate::any()
        };
        let selected = targets.select(&predicate);
        assert_eq!(selected.applicable, vec![(linux, Applicability::Yes)]);
        assert_eq!(selected.uncovered, vec![windows]);
    }
}
