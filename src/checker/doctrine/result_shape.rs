//! Result-shape builder (PHASE_BACKLOG Phase 168 168d) - given a parsed
//! SELECT clause (168c) plus the `FROM`/`JOIN` alias map (168b) and
//! 168a's entity metadata, computes one query row's own `Type`, mirroring
//! real Doctrine's default `HYDRATE_OBJECT` hydration rules exactly.

use std::collections::HashMap;

use crate::checker::Checker;
use crate::checker::type_union::union_types;
use crate::doctrine_functions::DoctrineFunctionKind;
use crate::registry::ClassInfo;
use crate::types::Type;

use super::dql::{AggregateFn, FieldRef, SelectItem, SelectItemKind};
use super::query_builder::{JoinClause, QueryBuilderChain};

/// A declared custom DQL function's own result type - see
/// `DoctrineFunctionKind`'s own doc comment for why each category maps
/// to exactly this type, not a narrower one.
fn doctrine_custom_function_type(kind: DoctrineFunctionKind) -> Type {
    match kind {
        DoctrineFunctionKind::Numeric => Type::Union(vec![Type::Int, Type::Float]),
        DoctrineFunctionKind::String | DoctrineFunctionKind::Datetime => Type::String,
    }
}

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
    Some(assoc.target.to_string())
}

/// The row key real Doctrine assigns a select item with no `AS` alias -
/// confirmed directly against `SqlWalker::walkSelectExpression()`: a bare
/// `alias.field` (`AST\PathExpression`) keys by its own field name
/// (`resultAlias = fieldIdentificationVariable ?: fieldName`), while
/// every other scalar-expression shape (aggregate, function, subquery,
/// `CASE`, literal) keys by `scalarResultCounter++` - a running counter
/// scoped to just those items, not the item's raw position in the whole
/// select list.
fn unaliased_scalar_key(kind: &SelectItemKind, index: usize, scalar_counter: &mut usize) -> String {
    match kind {
        SelectItemKind::Field(FieldRef::Field(_, field)) => field.clone(),
        // A bare, unaliased whole-entity item mixed into a select list
        // alongside other items - real Doctrine keys this by its own
        // `entityMappings` slot, not `scalarResultCounter`, a mixed-
        // result shape this project doesn't otherwise model (`IDENTITY()`
        // is a DQL function, not a path expression, so it takes the
        // `scalarResultCounter` branch below like any other function).
        SelectItemKind::Field(FieldRef::Alias(_)) => index.to_string(),
        _ => {
            let key = scalar_counter.to_string();
            *scalar_counter += 1;
            key
        }
    }
}

fn entity_info<'a>(checker: &'a Checker, class: &str) -> Option<&'a ClassInfo> {
    let (_, info) = checker.registry.lookup_class(class)?;
    info.doctrine_entity.as_ref().map(|_| info)
}

/// A whole-entity select item's own row type (PHASE_BACKLOG Phase 168
/// 168j) - a plain `Object(entity, [])` for the overwhelming majority
/// (an ordinary, non-hierarchical entity), or, when `entity` is the
/// *root* of a single-table/joined-table inheritance hierarchy (a real,
/// non-empty `#[ORM\DiscriminatorMap]`), a union across every one of its
/// mapped concrete subclasses instead - matching what a real,
/// unfiltered polymorphic `FROM BaseEntity` query actually returns
/// (any *one* row is genuinely one concrete subclass instance, never
/// the abstract base type itself). Falls back to the plain base-entity
/// object when `entity` isn't itself resolvable at all - defensive
/// only, since every caller already confirmed `entity` names a known
/// entity before reaching here.
fn entity_row_type(checker: &Checker, entity: &str) -> Type {
    let discriminator_map = entity_info(checker, entity)
        .and_then(|i| i.doctrine_entity.as_ref())
        .map(|e| &e.discriminator_map);
    match discriminator_map {
        Some(map) if !map.is_empty() => {
            let mut subclasses: Vec<&std::sync::Arc<str>> = map.values().collect();
            // Deterministic order - a `HashMap`'s own iteration order is
            // unspecified, and this union must render identically
            // across runs.
            subclasses.sort();
            subclasses
                .into_iter()
                .map(|class| Type::Object(class.to_string(), Vec::new()))
                .reduce(union_types)
                .unwrap_or_else(|| Type::Object(entity.to_string(), Vec::new()))
        }
        _ => Type::Object(entity.to_string(), Vec::new()),
    }
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
            Some(entity_row_type(checker, entity))
        }
        FieldRef::Field(alias, field) => {
            let entity = aliases.get(alias)?;
            entity_info(checker, entity)?
                .doctrine_entity
                .as_ref()?
                .fields
                .get(field.as_str())
                .cloned()
        }
        // `IDENTITY()`'s own real Doctrine `SqlWalker` implementation
        // (`IdentityFunction::getSql`) doesn't implement `Expression
        // WithReturnType`, so the generic `FunctionNode` fallback in
        // `walkSelectExpression` hardcodes its `ResultSetMapping` type to
        // `Types::STRING` unconditionally - it never goes through the
        // target entity's own id-column type conversion the way a plain
        // `alias.field` select does (confirmed directly against
        // `vendor/doctrine-orm`), so this project has no honest way to
        // claim the target's own (possibly value-object-wrapped) id
        // field type here even when it's otherwise known.
        FieldRef::Identity(..) => None,
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
        // A custom, project-registered DQL function (PHASE_BACKLOG
        // Phase 168 168h) - `Mixed` when this project's own
        // `[[doctrine_custom_functions]]` config doesn't declare it,
        // exactly like real `phpstan-doctrine`'s identical config
        // dependency for the same reason (see `doctrine_functions`'s own
        // module doc comment).
        SelectItemKind::Custom(name) => checker
            .registry
            .doctrine_custom_functions
            .get(&name.to_ascii_lowercase())
            .map(|kind| doctrine_custom_function_type(*kind))
            .unwrap_or(Type::Mixed),
        // A scalar subquery (PHASE_BACKLOG Phase 168 168k) - its own
        // `FROM`/`JOIN` scope is entirely independent of the outer
        // query's own `aliases` map (only an uncorrelated subquery is
        // recognized at all - see `dql`'s own module doc comment), so
        // its inner items resolve against their own, freshly-built
        // alias map, then reuse the identical single-scalar-column
        // requirement `getSingleColumnResult()`/`getSingleScalarResult()`
        // (168f) already have - a scalar subquery is only valid DQL
        // when it resolves to exactly one scalar column.
        SelectItemKind::Subquery(inner_chain, inner_items) => {
            single_scalar_column_type(checker, inner_chain, inner_items).unwrap_or(Type::Mixed)
        }
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
    // or multiple) - each item keys its own row entry by its own
    // `AS`-alias when it has one, otherwise by `unaliased_scalar_key`'s
    // own rule (see its doc comment - a raw select-list position is
    // never the real key for any of this project's modeled item shapes).
    let mut scalar_counter = 0usize;
    let fields = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let key = item
                .alias
                .clone()
                .unwrap_or_else(|| unaliased_scalar_key(&item.kind, i, &mut scalar_counter));
            (key, select_item_type(checker, &aliases, item), false)
        })
        .collect();
    Some(Type::ArrayShape(fields))
}

/// `getSingleScalarResult()` (`SingleScalarHydrator` -> `gatherScalarRowData`,
/// which really does apply the item's own registered type conversion,
/// confirmed against `vendor/doctrine-orm`): the select must have exactly
/// one item, and that item's own resolved type must not be an entity
/// object (a real Doctrine `HYDRATE_SINGLE_SCALAR` query only ever
/// targets a genuine scalar column - selecting a whole entity is
/// undefined behavior in real Doctrine, so this project doesn't guess a
/// shape for it either). `None` when the requirement isn't met - the
/// caller falls back to its own pre-Phase-168 status quo (`Mixed`), not a
/// scope cut.
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

/// Whether `ty` is, or anywhere contains, an entity/value-object `Type::
/// Object` member.
fn contains_object(ty: &Type) -> bool {
    match ty {
        Type::Object(..) => true,
        Type::Union(members) | Type::Intersection(members) => members.iter().any(contains_object),
        _ => false,
    }
}

/// `getSingleColumnResult()` - unlike `getSingleScalarResult()` above,
/// this one hydrates via `ScalarColumnHydrator`, a raw `Statement::
/// fetchFirstColumn()` that applies no type conversion at all (confirmed
/// directly against `vendor/doctrine-orm`), so a field mapped through a
/// custom Doctrine `Type` (a value-object id, a wrapped scalar) never
/// actually comes back as that PHP type here, only its own raw driver
/// value - `contains_object` catches both a bare custom-typed field and
/// the equally common `?EntityId $id = null` (pre-`GeneratedValue`)
/// nullable-object-union shape. A plain, un-wrapped native scalar (no
/// custom `Type`) genuinely does coincide with its own raw driver value,
/// so precision is kept for that case rather than blanket-falling back
/// to `Mixed` regardless of shape.
pub(crate) fn raw_scalar_column_type(
    checker: &Checker,
    chain: &QueryBuilderChain,
    items: &[SelectItem],
) -> Option<Type> {
    let [item] = items else { return None };
    let aliases = build_alias_map(checker, chain);
    let ty = select_item_type(checker, &aliases, item);
    if contains_object(&ty) { None } else { Some(ty) }
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
        .map(|(name, ty)| (name.to_string(), ty.clone(), false))
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
                Type::ObjectShape(vec![(id_field.to_string(), id_ty, false)])
            })
            .unwrap_or(Type::Mixed);
        fields.push((name.to_string(), shape, false));
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
    match ty {
        Type::Object(ref class, _) if entity_info(checker, class).is_some() => {
            entity_array_shape(checker, class)
        }
        // A polymorphic (168j, entity-inheritance) row - every mapped
        // subclass member converts independently.
        Type::Union(members) => Type::Union(
            members
                .into_iter()
                .map(|m| to_array_mode(checker, m))
                .collect(),
        ),
        other => other,
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
/// (`{alias}_{field}`-keyed) alongside every plain scalar/aggregate item
/// under `unaliased_scalar_key`'s own rule when unaliased, exactly like
/// `build_result_row_type`'s general case - both confirmed directly
/// against `AbstractHydrator::gatherScalarRowData`'s own body (the
/// `{alias}_{field}` form for a non-scalar/entity-field column, the
/// scalar-item `fieldName` otherwise). Zero real corpus usage (confirmed)
/// - built for completeness, not because any known call site needs it.
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
    let mut scalar_counter = 0usize;
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
                let mut scalar_fields: Vec<(&std::sync::Arc<str>, &Type)> = de
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
                let key = item
                    .alias
                    .clone()
                    .unwrap_or_else(|| unaliased_scalar_key(&item.kind, i, &mut scalar_counter));
                fields.push((key, select_item_type(checker, &aliases, item), false));
            }
        }
    }
    Some(Type::ArrayShape(fields))
}

#[cfg(test)]
mod tests;
