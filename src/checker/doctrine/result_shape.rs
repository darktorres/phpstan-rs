//! Result-shape builder (PHASE_BACKLOG Phase 168 168d) - given a parsed
//! SELECT clause (168c) plus the `FROM`/`JOIN` alias map (168b) and
//! 168a's entity metadata, computes one query row's own `Type`, mirroring
//! real Doctrine's default `HYDRATE_OBJECT` hydration rules exactly.

use std::collections::HashMap;

use crate::checker::Checker;
use crate::checker::type_union::union_types;
use crate::registry::ClassInfo;
use crate::types::Type;

use super::dql::{AggregateFn, FieldRef, SelectItem, SelectItemKind};
use super::query_builder::{JoinClause, QueryBuilderChain};

/// `alias -> entity FQN`, built once from `chain.from` plus every join
/// whose own target actually resolves (to a real entity class directly,
/// or via an association path off an already-known alias) - an
/// unresolvable join simply contributes no alias entry, so a later
/// select item referencing that alias correctly resolves to `Mixed`
/// instead (see `field_ref_type`'s own doc comment).
fn build_alias_map(checker: &Checker, chain: &QueryBuilderChain) -> HashMap<String, String> {
    let mut aliases = HashMap::new();
    if let Some((entity, alias)) = &chain.from {
        aliases.insert(alias.clone(), entity.clone());
    }
    for join in &chain.joins {
        if let Some(entity) = resolve_join_target(checker, &aliases, join) {
            aliases.insert(join.alias.clone(), entity);
        }
    }
    aliases
}

fn resolve_join_target(
    checker: &Checker,
    aliases: &HashMap<String, String>,
    join: &JoinClause,
) -> Option<String> {
    // `join(Entity::class, alias)` - 168b already resolved this to a
    // real class name; it names an entity directly when that class is
    // one.
    if entity_info(checker, &join.target).is_some() {
        return Some(join.target.clone());
    }
    // `join('sourceAlias.assocProp', alias)` - the far more common real
    // shape (an association join off an alias already in scope).
    let (source_alias, assoc_prop) = join.target.split_once('.')?;
    let source_entity = aliases.get(source_alias)?;
    let assoc = entity_info(checker, source_entity)?
        .doctrine_entity
        .as_ref()?
        .associations
        .get(assoc_prop)?;
    Some(assoc.target.clone())
}

fn entity_info<'a>(checker: &'a Checker, class: &str) -> Option<&'a ClassInfo> {
    let (_, info) = checker.registry.lookup_class(class)?;
    info.doctrine_entity.as_ref().map(|_| info)
}

/// `None` for a semantically-unresolvable reference (an unknown alias, a
/// field/association 168a's metadata doesn't know) - every caller here
/// falls back to `Mixed` for that one piece rather than failing the
/// whole row, matching real `phpstan-doctrine`'s own
/// `QueryResultTypeWalker`, which falls back to `MixedType` the same way
/// for an unresolvable *semantic* reference. This is deliberately
/// different from 168b/168c's own "undecidable" contract: those two
/// fail the whole query on a real DQL *syntax* problem (an unparseable
/// select item, an unresolvable `QueryBuilder` chain) - a real DQL
/// parser or a live QueryBuilder replay would fail identically. This
/// module runs only after both already succeeded, so anything it can't
/// resolve is a *metadata* gap (this project doesn't know a field's
/// type, not that the query itself is malformed), and real PHPStan's
/// own reference implementation localizes exactly that kind of gap to
/// `Mixed` rather than discarding an otherwise-precise row shape.
fn field_ref_type(
    checker: &Checker,
    aliases: &HashMap<String, String>,
    r: &FieldRef,
) -> Option<Type> {
    match r {
        FieldRef::Alias(alias) => {
            let entity = aliases.get(alias)?;
            Some(Type::Object(entity.clone(), Vec::new()))
        }
        FieldRef::Field(alias, field) => {
            let entity = aliases.get(alias)?;
            entity_info(checker, entity)?
                .doctrine_entity
                .as_ref()?
                .fields
                .get(field)
                .cloned()
        }
        FieldRef::Identity(alias, assoc_prop) => {
            let entity = aliases.get(alias)?;
            let assoc = entity_info(checker, entity)?
                .doctrine_entity
                .as_ref()?
                .associations
                .get(assoc_prop)?;
            let target_info = entity_info(checker, &assoc.target)?;
            let target_entity = target_info.doctrine_entity.as_ref()?;
            let id_field = target_entity.id_field.as_ref()?;
            target_entity.fields.get(id_field).cloned()
        }
    }
}

/// One select item's own real-Doctrine-hydration type - infallible
/// (see `field_ref_type`'s own doc comment for why: a metadata gap here
/// localizes to `Mixed` for this one item, never a whole-query failure).
/// `Aggregate`'s per-function return type is transcribed directly from
/// real `phpstan-doctrine`'s own `QueryResultTypeWalker::
/// walkAggregateExpression`, not inferred: `COUNT` is always `int<0,
/// max>` (`IntegerRangeType::fromInterval(0, null)` there) *regardless*
/// of whether its own operand resolves - real Doctrine's `COUNT` never
/// actually depends on its operand's type at all. `SUM`/`AVG`/`MIN`/
/// `MAX` all four resolve to the operand's own type instead (the real
/// walker calls the identical `walkSimpleArithmeticExpression` on the
/// operand for all four - there's no dedicated `SUM`/`AVG` output type),
/// falling back to `Mixed` when the operand itself doesn't resolve.
fn select_item_type(
    checker: &Checker,
    aliases: &HashMap<String, String>,
    item: &SelectItem,
) -> Type {
    match &item.kind {
        SelectItemKind::Field(r) => field_ref_type(checker, aliases, r).unwrap_or(Type::Mixed),
        SelectItemKind::Aggregate { func, operand } => match func {
            AggregateFn::Count => Type::IntRange(Some(0), None),
            AggregateFn::Sum | AggregateFn::Avg | AggregateFn::Min | AggregateFn::Max => operand
                .as_ref()
                .and_then(|r| field_ref_type(checker, aliases, r))
                .unwrap_or(Type::Mixed),
        },
        SelectItemKind::Case {
            then_arms,
            else_arm,
        } => {
            let arm_types = then_arms
                .iter()
                .chain(else_arm.iter())
                .map(|r| field_ref_type(checker, aliases, r).unwrap_or(Type::Mixed));
            // `then_arms` always has at least one entry (168c's own
            // `parse_case` requires at least one `WHEN ... THEN ...`
            // before accepting `END`), so this always has a first
            // element to reduce from.
            arm_types.reduce(union_types).unwrap_or(Type::Mixed)
        }
        // Real Doctrine constructs the named class from the query's own
        // positional results directly - exact once the class is named,
        // regardless of whether this project can otherwise resolve it
        // (the same leniency an ordinary `new UnknownClass()` expression
        // already gets elsewhere in this project's own type system).
        SelectItemKind::New(class) => Type::Object(checker.name_ctx.resolve(class), Vec::new()),
        // A custom, project-registered DQL function - PHASE_BACKLOG
        // Phase 168 168h's own config surface isn't built yet.
        SelectItemKind::Custom(_) => Type::Mixed,
    }
}

/// Builds one query row's own `Type` from its already-parsed select
/// items and the resolved `QueryBuilder` chain. `None` only when
/// `items` is empty (a `select()` with zero arguments is invalid DQL to
/// begin with) - every other case always produces a real `Type`, a
/// metadata gap on any individual piece localizing to `Mixed` for that
/// piece rather than failing the whole row (see `field_ref_type`'s own
/// doc comment for why this differs from 168b/168c's own real "whole-
/// query-undecidable" contract, which only ever fires on an actual DQL
/// syntax problem, before this function is ever reached).
pub(crate) fn build_result_row_type(
    checker: &Checker,
    chain: &QueryBuilderChain,
    items: &[SelectItem],
) -> Option<Type> {
    if items.is_empty() {
        return None;
    }
    let aliases = build_alias_map(checker, chain);

    // Exactly one item, a bare whole-entity select with no scalar
    // siblings - the row *is* the entity object directly, not wrapped.
    if let [
        SelectItem {
            kind: SelectItemKind::Field(alias_ref @ FieldRef::Alias(_)),
            alias: None,
        },
    ] = items
    {
        return Some(field_ref_type(checker, &aliases, alias_ref).unwrap_or(Type::Mixed));
    }

    // Every other shape (an entity plus scalars, or scalar-only, single
    // or multiple) - real Doctrine's mixed/positional hydration: each
    // item keys its own row entry by its own `AS`-alias when it has one,
    // otherwise by its absolute position in the select list (matching
    // real Doctrine, not a separately-counted "unaliased items only"
    // index).
    let fields = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let key = item.alias.clone().unwrap_or_else(|| i.to_string());
            (key, select_item_type(checker, &aliases, item), false)
        })
        .collect();
    Some(Type::ArrayShape(fields))
}

/// `getSingleColumnResult()`/`getSingleScalarResult()`'s own shared
/// requirement (PHASE_BACKLOG Phase 168 168f): the select must have
/// exactly one item, and that item's own resolved type must not be an
/// entity object (a real Doctrine `HYDRATE_SCALAR_COLUMN`/
/// `HYDRATE_SINGLE_SCALAR` query only ever targets a genuine scalar
/// column - selecting a whole entity through either method is undefined
/// behavior in real Doctrine, so this project doesn't guess a shape for
/// it either). `None` when the requirement isn't met - callers fall
/// back to their own pre-Phase-168 status quo (`list<mixed>`/`Mixed`),
/// not a scope cut.
pub(crate) fn single_scalar_column_type(
    checker: &Checker,
    chain: &QueryBuilderChain,
    items: &[SelectItem],
) -> Option<Type> {
    let [item] = items else { return None };
    let aliases = build_alias_map(checker, chain);
    match select_item_type(checker, &aliases, item) {
        Type::Object(..) => None,
        ty => Some(ty),
    }
}

/// An entity's own `HYDRATE_ARRAY` shape - every mapped scalar field
/// flattens directly into the shape; a to-one association converts to a
/// nested shape carrying just its own target's identifier (real
/// Doctrine's own default, unfetched-association array shape). A
/// to-many (`OneToMany`/`ManyToMany`) association's own array-hydration
/// shape is a separate, recursive-list rule this project doesn't model
/// yet - omitted from the shape entirely rather than guessed, the same
/// "don't guess" leniency as every other unresolved piece in this
/// phase. A deeper limitation, disclosed rather than silently assumed
/// solved: a to-one association that *is* eagerly joined and separately
/// selected in the same query still gets this same identifier-only
/// shape, not the fully recursive one real Doctrine would produce for
/// an actually-hydrated join - the real corpus's own joins never select
/// the joined alias itself (confirmed directly), so this gap has no
/// known real false-positive/negative to trace yet.
fn entity_array_shape(checker: &Checker, entity: &str) -> Type {
    let Some(de) = entity_info(checker, entity).and_then(|i| i.doctrine_entity.as_ref()) else {
        return Type::Mixed;
    };
    let mut fields: Vec<(String, Type, bool)> = de
        .fields
        .iter()
        .filter(|(name, _)| !de.associations.contains_key(*name))
        .map(|(name, ty)| (name.clone(), ty.clone(), false))
        .collect();
    for (name, assoc) in &de.associations {
        if assoc.is_collection {
            continue;
        }
        let target = entity_info(checker, &assoc.target).and_then(|i| i.doctrine_entity.as_ref());
        let shape = target
            .and_then(|t| t.id_field.as_ref())
            .map(|id_field| {
                let id_ty = target
                    .and_then(|t| t.fields.get(id_field))
                    .cloned()
                    .unwrap_or(Type::Mixed);
                Type::ObjectShape(vec![(id_field.clone(), id_ty, false)])
            })
            .unwrap_or(Type::Mixed);
        fields.push((name.clone(), shape, false));
    }
    // Deterministic order - a `HashMap`'s own iteration order is
    // unspecified, and this shape must render identically across runs.
    fields.sort_by(|a, b| a.0.cmp(&b.0));
    Type::ObjectShape(fields)
}

/// Converts an entity object type to its own `HYDRATE_ARRAY` shape when
/// `ty` names a known entity, leaving every other type unchanged -
/// applied once per field on top of `build_result_row_type`'s own
/// `HYDRATE_OBJECT` computation, rather than duplicating every select-
/// item-kind match arm a second time for the array-mode case.
fn to_array_mode(checker: &Checker, ty: Type) -> Type {
    match &ty {
        Type::Object(class, _) if entity_info(checker, class).is_some() => {
            entity_array_shape(checker, class)
        }
        _ => ty,
    }
}

/// `getArrayResult()` (PHASE_BACKLOG Phase 168 168f) - the identical row
/// shape `build_result_row_type` computes, except every entity member
/// converts to its own `entity_array_shape` instead of a real object
/// (real Doctrine's `HYDRATE_ARRAY` mode). Beyond real PHPStan (168e's
/// own stub check confirms `getArrayResult()` has no dynamic-return-type
/// extension there at all), so this is transcribed directly from
/// Doctrine ORM's own `ArrayHydrator` source, not checked against a
/// reference implementation's `dumpType()` output.
pub(crate) fn build_array_result_row_type(
    checker: &Checker,
    chain: &QueryBuilderChain,
    items: &[SelectItem],
) -> Option<Type> {
    build_result_row_type(checker, chain, items).map(|ty| match ty {
        Type::ArrayShape(fields) => Type::ArrayShape(
            fields
                .into_iter()
                .map(|(k, v, opt)| (k, to_array_mode(checker, v), opt))
                .collect(),
        ),
        other => to_array_mode(checker, other),
    })
}

/// `getScalarResult()` (PHASE_BACKLOG Phase 168 168f) - flattens every
/// selected entity's own scalar fields directly into one flat row
/// (`{alias}_{field}`-keyed, matching real Doctrine's own column-naming
/// convention for a plain, unaliased entity field), alongside every
/// plain scalar/aggregate item under its own key exactly like
/// `build_result_row_type`'s general case. Zero real corpus usage
/// (confirmed) - built for completeness, not because any known call site
/// needs it, and with correspondingly lower confidence than the other
/// three: `AbstractHydrator::gatherScalarRowData`'s own exact key-naming
/// rule was not read in full before writing this (this module's key
/// format above is inferred from `ScalarHydrator`'s own doc comment,
/// "almost the same as a regular SQL result set" - a real column-naming
/// convention, but not independently confirmed against the method body
/// itself), so treat this specific shape as needing verification against
/// real Doctrine before trusting it on a real query.
pub(crate) fn build_scalar_result_row_type(
    checker: &Checker,
    chain: &QueryBuilderChain,
    items: &[SelectItem],
) -> Option<Type> {
    if items.is_empty() {
        return None;
    }
    let aliases = build_alias_map(checker, chain);
    let mut fields: Vec<(String, Type, bool)> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        match &item.kind {
            SelectItemKind::Field(FieldRef::Alias(entity_alias)) => {
                let Some(entity) = aliases.get(entity_alias) else {
                    fields.push((i.to_string(), Type::Mixed, false));
                    continue;
                };
                let Some(de) =
                    entity_info(checker, entity).and_then(|e| e.doctrine_entity.as_ref())
                else {
                    fields.push((i.to_string(), Type::Mixed, false));
                    continue;
                };
                let mut scalar_fields: Vec<(&String, &Type)> = de
                    .fields
                    .iter()
                    .filter(|(name, _)| !de.associations.contains_key(*name))
                    .collect();
                scalar_fields.sort_by(|a, b| a.0.cmp(b.0));
                for (name, ty) in scalar_fields {
                    fields.push((format!("{entity_alias}_{name}"), ty.clone(), false));
                }
            }
            _ => {
                let key = item.alias.clone().unwrap_or_else(|| i.to_string());
                fields.push((key, select_item_type(checker, &aliases, item), false));
            }
        }
    }
    Some(Type::ArrayShape(fields))
}

#[cfg(test)]
mod tests;
