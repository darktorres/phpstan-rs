//! `banned_constructs` names and the `restricted_symbols` presets. The
//! checks themselves are fixtures under `tests/fixtures/project/banned`.

use crate::banned::{BannedConstruct, restricted_symbol_preset};

#[test]
fn an_unknown_construct_or_preset_is_rejected() {
    assert!(BannedConstruct::parse("gotos").is_err());
    assert!(restricted_symbol_preset("nope").is_err());
    let entries = restricted_symbol_preset("multibyte-strings").unwrap();
    assert_eq!(entries.len(), 12);
    assert!(
        entries
            .iter()
            .any(|e| e.name == "strlen" && e.message.contains("mb_strlen()"))
    );
}
