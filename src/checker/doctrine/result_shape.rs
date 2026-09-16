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

#[cfg(test)]
mod tests;
