mod absorb_rejects;
mod accessory_and_refinement_types;
mod binding_references;
mod borrowed_interning;
mod canonical;
mod comparator;
mod comparison;
mod concat;
mod constants;
mod constructors;
mod display_and_misc;
mod intersections;
mod keyword_hints;
mod kinds;
mod map_children;
mod mentions;
mod narrowing;
mod node_flags;
mod parsing;
mod reserved_type_words;
mod shape_index;
mod shape_relation;
mod shortcuts;
mod subtyping;
mod subtyping_properties;
mod templates_and_substitution;
mod relation_memo;
mod union_many;
mod unions;

/// The structural relation's subtype answer: `sub` is provably a `sup`
/// with no class knowledge.
fn is_subtype(sub: &crate::Type, sup: &crate::Type) -> bool {
    crate::Relation::structural()
        .is_super_type_of(sup, sub)
        .yes()
}

/// The structural relation's `accepts` answer: a `declared` slot takes
/// a `given` value.
fn accepts(declared: &crate::Type, given: &crate::Type) -> bool {
    crate::Relation::structural().accepts(declared, given).yes()
}
