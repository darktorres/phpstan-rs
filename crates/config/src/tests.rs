use serde::Deserialize;

use super::*;

#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(default)]
struct Global {
    paths: Vec<String>,
    level: Option<u8>,
}

impl Section for Global {
    const PATH: &'static [&'static str] = &[];
}

#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
struct Extras {
    check_boolean_trap: bool,
}

impl Section for Extras {
    const PATH: &'static [&'static str] = &["ext", "extras"];
}

#[derive(Debug, Default, Deserialize, PartialEq)]
#[serde(transparent)]
struct Rules(Vec<String>);

impl Section for Rules {
    const PATH: &'static [&'static str] = &["rules", "list"];
}

fn schema() -> Schema {
    Schema::new()
        .section::<Global>()
        .section::<Extras>()
        .section::<Rules>()
        .moved(
            "check_boolean_trap",
            &["ext", "extras", "check_boolean_trap"],
        )
}

#[test]
fn a_struct_section_knows_its_keys_and_a_list_section_has_none() {
    assert_eq!(fields_of::<Global>(), &["paths", "level"]);
    assert!(fields_of::<Rules>().is_empty());
}

#[test]
fn every_section_reads_its_own_part() {
    let file = ConfigFile::parse(
        "paths = [\"src\"]\nlevel = 5\n[ext.extras]\ncheck_boolean_trap = true\n[rules]\nlist = [\"a\"]\n",
        &schema(),
    )
    .unwrap();
    file.validate(&schema()).unwrap();
    assert_eq!(
        file.section::<Global>().unwrap(),
        Global {
            paths: vec!["src".to_string()],
            level: Some(5)
        }
    );
    assert!(file.section::<Extras>().unwrap().check_boolean_trap);
    assert_eq!(
        file.section::<Rules>().unwrap(),
        Rules(vec!["a".to_string()])
    );
}

#[test]
fn an_absent_section_is_its_default() {
    let file = ConfigFile::parse("", &schema()).unwrap();
    assert_eq!(file.section::<Extras>().unwrap(), Extras::default());
}

#[test]
fn a_key_no_section_owns_is_reported_by_its_full_path() {
    let file = ConfigFile::parse(
        "bogus = 1\n[ext.extras]\ncheck_boolean_trap = true\n[ext.unknown]\nx = 1\n",
        &schema(),
    )
    .unwrap();
    let error = file.validate(&schema()).unwrap_err();
    assert!(error.contains("unknown config key `bogus`"), "{error}");
    assert!(
        error.contains("unknown config key `ext.unknown`"),
        "{error}"
    );
}

#[test]
fn a_mistyped_value_names_its_section() {
    let file =
        ConfigFile::parse("[ext.extras]\ncheck_boolean_trap = \"yes\"\n", &schema()).unwrap();
    let error = file.validate(&schema()).unwrap_err();
    assert!(error.contains("config at `ext.extras`"), "{error}");
}

#[test]
fn an_old_key_moves_with_a_warning() {
    let file = ConfigFile::parse("check_boolean_trap = true\n", &schema()).unwrap();
    file.validate(&schema()).unwrap();
    assert!(file.section::<Extras>().unwrap().check_boolean_trap);
    assert_eq!(
        file.warnings(),
        &["config key `check_boolean_trap` is deprecated; use `ext.extras.check_boolean_trap`"]
    );
}

#[test]
fn an_old_key_and_its_new_place_both_set_is_an_error() {
    let error = ConfigFile::parse(
        "check_boolean_trap = true\n[ext.extras]\ncheck_boolean_trap = false\n",
        &schema(),
    )
    .unwrap_err();
    assert!(error.contains("sets both"), "{error}");
}

#[test]
fn set_overrides_by_dotted_path_and_reads_toml_values() {
    let mut file = ConfigFile::parse("level = 1\n", &schema()).unwrap();
    file.set(&schema(), "level=6").unwrap();
    file.set(&schema(), "ext.extras.check_boolean_trap=true")
        .unwrap();
    file.set(&schema(), "paths=[\"a\", \"b\"]").unwrap();
    file.validate(&schema()).unwrap();
    let global = file.section::<Global>().unwrap();
    assert_eq!(global.level, Some(6));
    assert_eq!(global.paths, vec!["a", "b"]);
    assert!(file.section::<Extras>().unwrap().check_boolean_trap);
}

#[test]
fn set_accepts_an_old_key() {
    let mut file = ConfigFile::empty();
    file.set(&schema(), "check_boolean_trap=true").unwrap();
    assert!(file.section::<Extras>().unwrap().check_boolean_trap);
    assert_eq!(file.warnings().len(), 1);
}

#[test]
fn set_needs_an_assignment() {
    let mut file = ConfigFile::empty();
    assert!(file.set(&schema(), "level").is_err());
}
