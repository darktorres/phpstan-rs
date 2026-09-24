//! `[[architecture]]` rule validation. The checks themselves are
//! fixtures under `tests/fixtures/project/architecture`.

use crate::architecture::ArchitectureRule;

#[test]
fn an_invalid_pattern_fails_validation() {
    let rule: ArchitectureRule = toml::from_str("classes = [{ namespace = \"/(/\" }]").unwrap();
    assert!(rule.validate().is_err());
}
