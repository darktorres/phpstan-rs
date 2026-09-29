//! Doctrine ORM entity metadata - extracted
//! once, here, from a class's own `#[ORM\...]` attributes, reusing the
//! native property types `ClassReflection::from_decl` has already resolved
//! (this project's own already-indexed property types answer "what type
//! is this entity field" for free - no from-scratch Doctrine mapping-
//! metadata resolver needed, unlike real PHPStan's own `phpstan-doctrine`,
//! which actually runs Doctrine's real `ClassMetadataFactory` at analysis
//! time). Every real Piwigo entity is 100% PHP-8-attribute-mapped
//! (confirmed directly - zero `@ORM\...` legacy docblock annotations
//! anywhere in the corpus), so only the attribute style is supported.
//!
//! Entity inheritance - `#[ORM\
//! DiscriminatorMap]`'s own value -> concrete-subclass map is parsed on
//! the root entity; a `FROM BaseEntity alias` with a non-empty map
//! produces a union across every mapped subclass (`checker::doctrine::
//! result_shape`'s own consumer), matching what a real, unfiltered
//! polymorphic query actually returns. Confirmed absent from the real
//! corpus this phase targets, not assumed absent - built anyway, since
//! corpus incidence affects verification confidence, never whether a
//! real, well-defined Doctrine feature gets built at all.

use rustc_hash::FxHashMap;
use syntax::name::Name;

use syntax::ast::{
    ArrayItem, Attribute, ClassDecl, Expr, ExprKind, Literal, QualifiedName, StaticTarget,
};
use types::Type;

use super::PropertyReflection;
use crate::registry::name_context::class_ref;

/// One Doctrine association property (`#[ORM\ManyToOne]`/`OneToOne`/
/// `OneToMany`/`ManyToMany`).
#[derive(Debug, Clone)]
pub struct DoctrineAssociation {
    /// The target entity's fully-qualified class name.
    pub target: Name,
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
    pub id_field: Option<Name>,
    /// Every mapped scalar field's own already-resolved native type
    /// (reused from `ClassReflection::properties`, not re-resolved here) -
    /// keyed by property name, association properties included too
    /// (their "field type" is the association's own declared type,
    /// consulted only via `associations` below in practice).
    pub fields: FxHashMap<Name, Type>,
    pub associations: FxHashMap<Name, DoctrineAssociation>,
    /// `#[ORM\Table(name: '...')]`'s own argument - `None` when absent
    /// (real Doctrine would then fall back to its own default naming
    /// strategy, generally the bare class name, which this project
    /// doesn't attempt to replicate - every real corpus entity checked
    /// declares this explicitly). Raw SQL query reflection's own table-
    /// name cross-reference (`checker::sql_reflection::resolve`) is the
    /// only consumer - the DQL-side machinery never needed
    /// a real DB table/column name at all, since DQL resolves through
    /// property names.
    pub table_name: Option<Name>,
    /// Property name -> its own real DB column name
    /// (`#[ORM\Column(name: '...')]`'s argument, or the property's own
    /// name unchanged when absent - real Doctrine's own default naming
    /// strategy, confirmed: every corpus `#[ORM\Column]` with no `name:`
    /// argument maps a property whose name is already identical to its
    /// real DB column). Only scalar `fields` are present here, not
    /// `associations` - a raw SQL column is never itself another
    /// entity's own object identity, so there's nothing this cross-
    /// reference would need an association's own column name for.
    pub column_names: FxHashMap<Name, Name>,
    /// `#[ORM\Entity(repositoryClass: ...)]`'s own argument, if present -
    /// resolved to a fully-qualified class name.
    pub repository_class: Option<Name>,
    /// `#[ORM\DiscriminatorMap([...])]`'s own value -> concrete-subclass
    /// map, non-empty only on the root entity of a single-table/joined-
    /// table inheritance hierarchy. Empty for every ordinary, non-
    /// hierarchical entity - the overwhelming majority. The key is an
    /// arbitrary literal discriminator tag (developer-chosen, not a
    /// source identifier), so it stays `String`; the value is a real
    /// class FQN.
    pub discriminator_map: FxHashMap<String, Name>,
}

fn is_orm_attribute(attr: &Attribute, short_name: &str) -> bool {
    attr.name
        .resolved_class()
        .and_then(|resolved| resolved.strip_prefix("Doctrine\\ORM\\Mapping\\"))
        == Some(short_name)
}

fn find_orm_attribute<'a>(attrs: &'a [Attribute], short_name: &str) -> Option<&'a Attribute> {
    attrs.iter().find(|a| is_orm_attribute(a, short_name))
}

/// A named argument's own value, when it's a class-constant-fetch
/// expression (`SomeClass::class`) - the only shape any real
/// `repositoryClass`/`targetEntity` argument in the corpus ever uses.
/// Anything else (a dynamic expression) yields `None` rather than a
/// guess - the same "don't guess" leniency this project applies
/// everywhere else a statically-unresolvable value could otherwise be
/// wrongly trusted.
fn class_const_arg<'a>(attr: &'a Attribute, name: &str) -> Option<&'a QualifiedName> {
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
            ExprKind::Literal(Literal::String(s)) => s.as_str(),
            _ => None,
        }
    })
}

fn class_name_of_class_const(e: &Expr) -> Option<&QualifiedName> {
    match &e.kind {
        ExprKind::ClassNameFetch(StaticTarget::Name(class_name, _)) => Some(class_name),
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
fn discriminator_map_arg(attr: &Attribute) -> FxHashMap<String, Name> {
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
            Some((value.to_string(), class_ref(class)))
        })
        .collect()
}

/// One mapped property's own raw attribute list, alongside the FQN
/// association target this project can already tell from its
/// already-resolved native type (`PropertyReflection::ty_native`) - the same
/// type `ClassReflection::from_decl` resolved for completely unrelated reasons
/// (real property-access type-checking), reused here rather than
/// re-parsing `#[ORM\ManyToOne(targetEntity: ...)]`'s own argument for
/// the common case where the property's own native type already names
/// the target class directly (every real corpus association does).
fn association_target_from_native_type(ty: &Type) -> Option<String> {
    ty.union_members()
        .iter()
        .find_map(|m| m.as_object())
        .map(|(class, _)| class.to_string())
}

/// Extracts this class's own Doctrine entity metadata, `None` when it
/// carries no `#[ORM\Entity]` attribute at all - the overwhelming
/// majority of classes in any real project, including this project's
/// own.
pub(crate) fn compute_doctrine_entity(
    c: &ClassDecl,
    properties: &FxHashMap<Name, PropertyReflection>,
) -> Option<DoctrineEntity> {
    let entity_attr = find_orm_attribute(&c.attrs, "Entity")?;
    let repository_class = class_const_arg(entity_attr, "repositoryClass").map(class_ref);
    let table_name = find_orm_attribute(&c.attrs, "Table")
        .and_then(|attr| string_arg(attr, "name"))
        .map(Name::from);
    let discriminator_map = find_orm_attribute(&c.attrs, "DiscriminatorMap")
        .map(discriminator_map_arg)
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
    if let Some(ctor) = c.constructor() {
        for p in &ctor.function.params {
            if p.promoted_visibility.is_some() {
                attrs_by_property.insert(&p.name, &p.attrs);
            }
        }
    }

    let mut id_fields: Vec<Name> = Vec::new();
    let mut fields: FxHashMap<Name, Type> = FxHashMap::default();
    let mut column_names: FxHashMap<Name, Name> = FxHashMap::default();
    let mut associations: FxHashMap<Name, DoctrineAssociation> = FxHashMap::default();
    for (name, attrs) in &attrs_by_property {
        let Some(sig) = properties.get(*name) else {
            continue;
        };
        fields.insert(Name::from(*name), sig.ty_native.clone());
        if let Some(column_attr) = find_orm_attribute(attrs, "Column") {
            let column_name = string_arg(column_attr, "name").unwrap_or(name);
            column_names.insert(Name::from(*name), Name::from(column_name));
        }
        if find_orm_attribute(attrs, "Id").is_some() {
            id_fields.push(Name::from(*name));
        }
        let to_one = find_orm_attribute(attrs, "ManyToOne")
            .or_else(|| find_orm_attribute(attrs, "OneToOne"));
        let to_many = find_orm_attribute(attrs, "OneToMany")
            .or_else(|| find_orm_attribute(attrs, "ManyToMany"));
        if let Some(assoc_attr) = to_one {
            if let Some(target) = association_target_from_native_type(&sig.ty_native) {
                associations.insert(
                    Name::from(*name),
                    DoctrineAssociation {
                        target: Name::from(&target),
                        is_collection: false,
                    },
                );
            } else if let Some(target) = class_const_arg(assoc_attr, "targetEntity") {
                associations.insert(
                    Name::from(*name),
                    DoctrineAssociation {
                        target: class_ref(target),
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
                    Name::from(*name),
                    DoctrineAssociation {
                        target: class_ref(target),
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
