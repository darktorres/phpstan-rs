//! Doctrine ORM entity metadata (ROADMAP Phase 168 168a) - extracted
//! once, here, from a class's own `#[ORM\...]` attributes, reusing the
//! native property types `ClassInfo::from_decl` has already resolved
//! (this project's own already-indexed property types answer "what type
//! is this entity field" for free - no from-scratch Doctrine mapping-
//! metadata resolver needed, unlike real PHPStan's own `phpstan-doctrine`,
//! which actually runs Doctrine's real `ClassMetadataFactory` at analysis
//! time). Every real Piwigo entity is 100% PHP-8-attribute-mapped
//! (confirmed directly - zero `@ORM\...` legacy docblock annotations
//! anywhere in the corpus), so only the attribute style is supported.
//!
//! Entity inheritance (ROADMAP Phase 168 168j) - `#[ORM\
//! DiscriminatorMap]`'s own value -> concrete-subclass map is parsed on
//! the root entity; a `FROM BaseEntity alias` with a non-empty map
//! produces a union across every mapped subclass (`checker::doctrine::
//! result_shape`'s own consumer), matching what a real, unfiltered
//! polymorphic query actually returns. Confirmed absent from the real
//! corpus this phase targets, not assumed absent - built anyway, since
//! corpus incidence affects verification confidence, never whether a
//! real, well-defined Doctrine feature gets built at all.

use rustc_hash::FxHashMap;
use std::sync::Arc;

use crate::ast::{ArrayItem, Attribute, ClassDecl, Expr, ExprKind, Literal};
use crate::intern::intern;
use crate::registry::name_context::NameContext;
use crate::types::Type;

use super::PropertySig;

/// One Doctrine association property (`#[ORM\ManyToOne]`/`OneToOne`/
/// `OneToMany`/`ManyToMany`).
#[derive(Debug, Clone)]
pub struct DoctrineAssociation {
    /// The target entity's fully-qualified class name.
    pub target: Arc<str>,
    /// `OneToMany`/`ManyToMany` - the property holds a collection of
    /// `target`, not a single instance.
    pub is_collection: bool,
}

/// One `#[ORM\Entity]`-mapped class's metadata, keyed by property name -
/// see this module's own doc comment for what's deliberately not modeled
/// (entity inheritance).
#[derive(Debug, Clone, Default)]
pub struct DoctrineEntity {
    /// The `#[ORM\Id]`-attributed property name, if this entity has
    /// exactly one (a composite identifier - more than one `#[ORM\Id]`
    /// property, real and valid Doctrine, e.g. `ImageCategoryEntity` -
    /// deliberately left `None` here rather than picking one arbitrarily;
    /// `IDENTITY()` on a composite-key association is already a DQL
    /// error in real Doctrine, so this project has nothing incorrect to
    /// produce either way).
    pub id_field: Option<Arc<str>>,
    /// Every mapped scalar field's own already-resolved native type
    /// (reused from `ClassInfo::properties`, not re-resolved here) -
    /// keyed by property name, association properties included too
    /// (their "field type" is the association's own declared type,
    /// consulted only via `associations` below in practice).
    pub fields: FxHashMap<Arc<str>, Type>,
    pub associations: FxHashMap<Arc<str>, DoctrineAssociation>,
    /// `#[ORM\Table(name: '...')]`'s own argument - `None` when absent
    /// (real Doctrine would then fall back to its own default naming
    /// strategy, generally the bare class name, which this project
    /// doesn't attempt to replicate - every real corpus entity checked
    /// declares this explicitly). Raw SQL query reflection's own table-
    /// name cross-reference (`checker::sql_reflection::resolve`) is the
    /// only consumer - Phase 168's own DQL-side machinery never needed
    /// a real DB table/column name at all, since DQL resolves through
    /// property names.
    pub table_name: Option<Arc<str>>,
    /// Property name -> its own real DB column name
    /// (`#[ORM\Column(name: '...')]`'s argument, or the property's own
    /// name unchanged when absent - real Doctrine's own default naming
    /// strategy, confirmed: every corpus `#[ORM\Column]` with no `name:`
    /// argument maps a property whose name is already identical to its
    /// real DB column). Only scalar `fields` are present here, not
    /// `associations` - a raw SQL column is never itself another
    /// entity's own object identity, so there's nothing this cross-
    /// reference would need an association's own column name for.
    pub column_names: FxHashMap<Arc<str>, Arc<str>>,
    /// `#[ORM\Entity(repositoryClass: ...)]`'s own argument, if present -
    /// resolved to a fully-qualified class name.
    pub repository_class: Option<Arc<str>>,
    /// `#[ORM\DiscriminatorMap([...])]`'s own value -> concrete-subclass
    /// map, non-empty only on the root entity of a single-table/joined-
    /// table inheritance hierarchy. Empty for every ordinary, non-
    /// hierarchical entity - the overwhelming majority. The key is an
    /// arbitrary literal discriminator tag (developer-chosen, not a
    /// source identifier), so it stays `String`; the value is a real
    /// class FQN.
    pub discriminator_map: FxHashMap<String, Arc<str>>,
}

fn resolved_attr_name(ctx: &NameContext, attr: &Attribute) -> String {
    ctx.resolve(&attr.name)
}

fn is_orm_attribute(ctx: &NameContext, attr: &Attribute, short_name: &str) -> bool {
    resolved_attr_name(ctx, attr) == format!("Doctrine\\ORM\\Mapping\\{short_name}")
}

fn find_orm_attribute<'a>(
    ctx: &NameContext,
    attrs: &'a [Attribute],
    short_name: &str,
) -> Option<&'a Attribute> {
    attrs.iter().find(|a| is_orm_attribute(ctx, a, short_name))
}

/// A named argument's own value, when it's a class-constant-fetch
/// expression (`SomeClass::class`) - the only shape any real
/// `repositoryClass`/`targetEntity` argument in the corpus ever uses.
/// Anything else (a dynamic expression) yields `None` rather than a
/// guess - the same "don't guess" leniency this project applies
/// everywhere else a statically-unresolvable value could otherwise be
/// wrongly trusted.
fn class_const_arg<'a>(attr: &'a Attribute, name: &str) -> Option<&'a str> {
    attr.args.iter().find_map(|arg| {
        if arg.name.as_deref() != Some(name) {
            return None;
        }
        class_name_of_class_const(&arg.value)
    })
}

/// A named argument's own value, when it's a plain string literal
/// (`#[ORM\Table(name: 'categories')]`/`#[ORM\Column(name: '...')]`'s
/// only real corpus shape); unlike `ast::literal_string_value`, a
/// concatenation of literals is not folded.
fn string_arg<'a>(attr: &'a Attribute, name: &str) -> Option<&'a str> {
    attr.args.iter().find_map(|arg| {
        if arg.name.as_deref() != Some(name) {
            return None;
        }
        match &arg.value.kind {
            ExprKind::Literal(Literal::String(s)) => Some(s.as_ref()),
            _ => None,
        }
    })
}

fn class_name_of_class_const(e: &Expr) -> Option<&str> {
    match &e.kind {
        ExprKind::StaticAccess(class_name, member) if member.eq_ignore_ascii_case("class") => {
            Some(class_name.as_str())
        }
        _ => None,
    }
}

/// `#[ORM\DiscriminatorMap(['cat' => CatEntity::class, ...])]`'s own
/// array-literal argument, resolved to a plain (discriminator value ->
/// concrete subclass FQN) map - the argument may be named (`value:
/// [...]`, matching real Doctrine's own attribute constructor
/// signature) or positional, so both are checked. A key that isn't a
/// literal string, or a value that isn't a `SomeClass::class` reference,
/// is simply skipped rather than guessed - the same "don't guess"
/// leniency this module applies everywhere else.
fn discriminator_map_arg(ctx: &NameContext, attr: &Attribute) -> FxHashMap<String, Arc<str>> {
    let array = attr
        .args
        .iter()
        .find(|a| a.name.as_deref() == Some("value"))
        .or_else(|| attr.args.iter().find(|a| a.name.is_none()))
        .and_then(|a| match &a.value.kind {
            ExprKind::ArrayLiteral(items) => Some(items),
            _ => None,
        });
    let Some(items) = array else {
        return FxHashMap::default();
    };
    items
        .iter()
        .filter_map(|item: &ArrayItem| {
            let ExprKind::Literal(Literal::String(value)) = &item.key.as_ref()?.kind else {
                return None;
            };
            let class = class_name_of_class_const(&item.value)?;
            Some((value.to_string(), intern(&ctx.resolve(class))))
        })
        .collect()
}

/// One mapped property's own raw attribute list, alongside the FQN
/// association target this project can already tell from its
/// already-resolved native type (`PropertySig::ty_native`) - the same
/// type `ClassInfo::from_decl` resolved for completely unrelated reasons
/// (real property-access type-checking), reused here rather than
/// re-parsing `#[ORM\ManyToOne(targetEntity: ...)]`'s own argument for
/// the common case where the property's own native type already names
/// the target class directly (every real corpus association does).
fn association_target_from_native_type(ty: &Type) -> Option<String> {
    match ty {
        Type::Object(class, _) => Some(class.clone()),
        Type::Union(members) => members.iter().find_map(association_target_from_native_type),
        _ => None,
    }
}

/// Extracts this class's own Doctrine entity metadata, `None` when it
/// carries no `#[ORM\Entity]` attribute at all - the overwhelming
/// majority of classes in any real project, including this project's
/// own.
pub(crate) fn compute_doctrine_entity(
    c: &ClassDecl,
    ctx: &NameContext,
    properties: &FxHashMap<Arc<str>, PropertySig>,
) -> Option<DoctrineEntity> {
    let entity_attr = find_orm_attribute(ctx, &c.attrs, "Entity")?;
    let repository_class =
        class_const_arg(entity_attr, "repositoryClass").map(|name| intern(&ctx.resolve(name)));
    let table_name = find_orm_attribute(ctx, &c.attrs, "Table")
        .and_then(|attr| string_arg(attr, "name"))
        .map(intern);
    let discriminator_map = find_orm_attribute(ctx, &c.attrs, "DiscriminatorMap")
        .map(|attr| discriminator_map_arg(ctx, attr))
        .unwrap_or_default();

    // Every property's own raw attrs, from both a plain body declaration
    // and constructor property promotion - `PropertyDecl`/`Param` are
    // different AST shapes with no shared trait, so this collects both
    // into one (name -> attrs) map up front rather than duplicating the
    // attribute-matching logic below for each source.
    let mut attrs_by_property: FxHashMap<&str, &[Attribute]> = FxHashMap::default();
    for p in &c.properties {
        attrs_by_property.insert(&p.name, &p.attrs);
    }
    if let Some(ctor) = c
        .methods
        .iter()
        .find(|m| m.function.name == "__construct".into())
    {
        for p in &ctor.function.params {
            if p.promoted_visibility.is_some() {
                attrs_by_property.insert(&p.name, &p.attrs);
            }
        }
    }

    let mut id_fields: Vec<Arc<str>> = Vec::new();
    let mut fields: FxHashMap<Arc<str>, Type> = FxHashMap::default();
    let mut column_names: FxHashMap<Arc<str>, Arc<str>> = FxHashMap::default();
    let mut associations: FxHashMap<Arc<str>, DoctrineAssociation> = FxHashMap::default();
    for (name, attrs) in &attrs_by_property {
        let Some(sig) = properties.get(*name) else {
            continue;
        };
        fields.insert(intern(name), sig.ty_native.clone());
        if let Some(column_attr) = find_orm_attribute(ctx, attrs, "Column") {
            let column_name = string_arg(column_attr, "name").unwrap_or(name);
            column_names.insert(intern(name), intern(column_name));
        }
        if find_orm_attribute(ctx, attrs, "Id").is_some() {
            id_fields.push(intern(name));
        }
        let to_one = find_orm_attribute(ctx, attrs, "ManyToOne")
            .or_else(|| find_orm_attribute(ctx, attrs, "OneToOne"));
        let to_many = find_orm_attribute(ctx, attrs, "OneToMany")
            .or_else(|| find_orm_attribute(ctx, attrs, "ManyToMany"));
        if let Some(assoc_attr) = to_one {
            if let Some(target) = association_target_from_native_type(&sig.ty_native) {
                associations.insert(
                    intern(name),
                    DoctrineAssociation {
                        target: intern(&target),
                        is_collection: false,
                    },
                );
            } else if let Some(target) = class_const_arg(assoc_attr, "targetEntity") {
                associations.insert(
                    intern(name),
                    DoctrineAssociation {
                        target: intern(&ctx.resolve(target)),
                        is_collection: false,
                    },
                );
            }
        } else if let Some(assoc_attr) = to_many {
            // A `OneToMany`/`ManyToMany` property is collection-typed
            // (`Collection`/`array`), which carries no target class name
            // of its own - unlike the to-one case above, the attribute's
            // own explicit `targetEntity: ...` argument is the only real
            // source here.
            if let Some(target) = class_const_arg(assoc_attr, "targetEntity") {
                associations.insert(
                    intern(name),
                    DoctrineAssociation {
                        target: intern(&ctx.resolve(target)),
                        is_collection: true,
                    },
                );
            }
        }
    }

    Some(DoctrineEntity {
        id_field: match id_fields.len() {
            1 => id_fields.into_iter().next(),
            _ => None,
        },
        fields,
        associations,
        table_name,
        column_names,
        repository_class,
        discriminator_map,
    })
}
