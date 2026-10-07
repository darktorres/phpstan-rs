use super::super::*;
pub(super) fn build_registry(src: &str) -> Registry {
    let parsed = syntax::parse(src).unwrap();
    Registry::build(&parsed.arenas, parsed.statements())
}
