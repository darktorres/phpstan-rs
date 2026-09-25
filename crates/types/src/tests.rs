mod accessory_and_refinement_types;
mod display_and_misc;
mod parsing;
mod subtyping;
mod subtyping_properties;
mod templates_and_substitution;

/// The structural relation's subtype answer: `sub` is provably a `sup`
/// with no class knowledge.
fn is_subtype(sub: &crate::Type, sup: &crate::Type) -> bool {
    crate::Relation::structural().subtype(sub, sup)
}

/// The structural relation's `accepts` answer: a `declared` slot takes
/// a `given` value.
fn accepts(declared: &crate::Type, given: &crate::Type) -> bool {
    crate::Relation::structural().accepts(declared, given).yes()
}
