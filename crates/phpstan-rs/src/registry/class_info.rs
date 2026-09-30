use indexmap::IndexMap;
use rustc_hash::{FxHashMap, FxHashSet};

use crate::registry::function_sig::{
    DeclaredType, FunctionReflection, ParameterReflection, apply_implicit_nullable_default,
    build_template_params, build_type_aliases, declared_type, infer_class_const_type,
    mark_template_params, substitute_type_aliases,
};
use crate::registry::name_context::{
    class_ref, collect_class_name_case_refs, resolve_names_in_type,
};
use crate::registry::provenance::{Origin, Provenance};
use crate::registry::reflection_attribute::ReflectionAttribute;
use crate::registry::type_node::parse_type_node;
use phpdoc::docblock::{self, DocBlock};
use phpdoc::grammar::TypeNode;
use syntax::ast::{ClassDecl, ClassDef, ClassKind, ExprKind, Literal, Visibility};
use syntax::name_resolution::NameContext;
use syntax::token::Span;
use types::{self, AccessoryKind, Name, TemplateParam, Type};

mod attributes;
mod doctrine_entity;
mod tag_synthesis;
mod virtual_property;

use attributes::compute_attribute_targets;
pub(crate) use doctrine_entity::DoctrineEntity;
use doctrine_entity::compute_doctrine_entity;
use tag_synthesis::method_tag_sig;
pub(crate) use virtual_property::{hook_body_calls_parent_hook, hook_body_references_property};
use virtual_property::{hooks_reference_backing_storage, property_hook_set_type};

impl PropertyReflection {
    /// Whether a read is legal: not a `@property-write` tag, and a
    /// virtual hooked property needs a `get` hook.
    pub fn is_readable(&self) -> bool {
        !self.tag_write_only && (!self.is_virtual || self.has_get_hook)
    }

    /// Whether a write is legal: not a `@property-read` tag, and a
    /// virtual hooked property needs a `set` hook.
    pub fn is_writable(&self) -> bool {
        !self.tag_read_only && (!self.is_virtual || self.has_set_hook)
    }
}

#[derive(Debug, Clone)]
pub struct PropertyReflection {
    pub(in crate::registry) ty: Type,
    pub(in crate::registry) is_static: bool,
    pub(in crate::registry) is_readonly: bool,
    // `@readonly`-by-PHPDoc (never native `readonly`) with
    // `@phpstan-allow-private-mutation`: the declaring class may still
    // write it outside its constructor.
    pub(in crate::registry) allows_private_mutation: bool,
    // `@readonly`-by-PHPDoc, never native `readonly`.
    pub(in crate::registry) readonly_by_phpdoc: bool,
    // A `@property-read`/`@property-write` magic tag's one-directional
    // access.
    pub(in crate::registry) tag_read_only: bool,
    pub(in crate::registry) tag_write_only: bool,
    // `public`/`private`/`protected`. A magic `@property` tag and an
    // enum's built-in `name`/`value` are always `Visibility::Public` -
    // real PHP has no way to declare either any other way.
    pub(in crate::registry) visibility: Visibility,
    // PHP 8.4 asymmetric visibility (`public private(set) string $x;`) -
    // the write-side visibility, defaulting to `visibility` itself (the
    // read side) for an ordinary, symmetric property, or one with no
    // established per-item place to write a separate write-visibility (a
    // promoted constructor property, a magic `@property` tag, an enum's
    // built-in `name`/`value`).
    pub(in crate::registry) write_visibility: Visibility,
    // A native `readonly` property declared `public` with no set
    // visibility of its own, which PHP 8.4 makes `protected(set)`
    // (`Checker::write_visibility`).
    pub(in crate::registry) implicit_protected_set: bool,
    // PHP 8.4 property hooks: an explicit `set(Type $value) { ... }` hook
    // can accept a wider type than the property's own backing/read type
    // (`public array $x { set(array $value) { $this->x =
    // sanitize($value); } }` - the hook does its own coercion inside the
    // body before the real assignment). `None` when there's no hook, or
    // the hook uses an implicit `set { ... }` with no parameter list at
    // all - a write then still checks against `ty` as before.
    pub(in crate::registry) set_ty: Option<Type>,
    // PHP 8.4's "virtual property" concept - a hooked property with no
    // backing storage allocated for it. Computed as a purely syntactic
    // compile-time scan, mirroring PHP's own `ReflectionProperty::
    // isVirtual()`: a hooked property is virtual iff no hook body
    // contains, anywhere in its own function scope (including an
    // unreachable branch like `if (false) {...}` - not reachability-
    // based), a `$this->propName` property-fetch for that exact property
    // (read, bare unused statement, `isset()`, or `unset()` all count
    // identically; a nested closure/arrow-fn/anonymous-class body does
    // NOT count - it's a separate function scope; neither does a dynamic
    // `$this->$name` access or an indirect copy like `$self = $this;
    // $self->propName`). Always `false` for a property with no hooks at
    // all. Gates `propertyGetHook.noRead`/`propertySetHook.noAssign`
    // (`checker/oop.rs::check_property_hooks`), mirroring real PHPStan's
    // `GetNonVirtualPropertyHookReadRule`/
    // `SetNonVirtualPropertyHookAssignRule`.
    pub(in crate::registry) is_virtual: bool,
    // `@internal` on this property's own docblock - see
    // `docblock::DocBlock::internal`'s doc comment. Only ever set for a
    // real, plainly-declared property with its own doc comment; a
    // promoted constructor property, a magic `@property` tag, and an
    // enum's built-in `name`/`value` all stay `false` - none of them has
    // an established per-item place to write the tag.
    pub(in crate::registry) is_internal: bool,
    // `@deprecated` on this property's own doc comment - direct mirror of
    // `FunctionReflection::deprecated`'s "own tag only, no class-level fallback"
    // scoping. Same restriction as `is_internal` above.
    pub(in crate::registry) deprecated: Option<String>,
    // Whether this property's own declaration (an ordinary
    // `PropertyDecl::default`, or a promoted constructor parameter's own
    // `Param::default`) gives it a default value - `checker/
    // uninitialized.rs`'s "genuinely might still be uninitialized"
    // candidate gate needs the class's AST to compute this, which isn't
    // available for an arbitrary `$obj->prop` access (only for
    // `$this->prop` inside the class currently being checked), so this
    // field makes the same fact available from `PropertyReflection` alone, for
    // `check_pointless_offset_or_property_probe`'s "maybe genuinely
    // uninitialized, don't flag isset() as redundant" exemption. `false`
    // (the safe, permissive default) for every synthetic property with
    // no real declaration to read a default from.
    pub(in crate::registry) has_default: bool,
    // PHP 8.4 `abstract` property modifier (only legal on a hooked
    // property inside an abstract class, but tracked here unconditionally
    // like `is_readonly` above - the parser/engine enforce the legality
    // half). `false` for every synthetic property.
    pub(in crate::registry) is_abstract: bool,
    // Whether a `get`/`set` hook is declared at all (real PHP's
    // `hasHook()`), independent of whether it has a real body -
    // `PropertyHookBody::Abstract` is what makes this question
    // answerable. `false` for every synthetic property.
    pub(in crate::registry) has_get_hook: bool,
    pub(in crate::registry) has_set_hook: bool,
    // The native, no-docblock-merge type this property alone declares -
    // the exact `ty`-vs-`ty_native` split `ParameterReflection` has, needed here for
    // `OverridingPropertyRule`'s `property.nativeType`/
    // `.missingNativeType`/`.extraNativeType` family, which real PHPStan
    // checks purely at the native level. `resolve_type(None, native_hint)`
    // collapses "no native hint" and an explicit `mixed` hint into the
    // same `mixed` value, which is why `has_native_type` below must
    // be a separate boolean rather than testing this field against
    // `mixed`.
    pub(in crate::registry) ty_native: Type,
    // This declaration's own PHPDoc type (`@var`, or for a promoted
    // property its constructor `@param`/inline `@var`), resolved but not
    // reconciled with the native hint - `ParameterReflection::doc_ty`'s property
    // equivalent. `None` without one.
    pub(in crate::registry) doc_ty: Option<Type>,
    pub(in crate::registry) has_native_type: bool,
    // `ParameterReflection::case_refs`'s property-position equivalent. Populated
    // identically at all three real construction sites (plain declared
    // property, promoted constructor property, `@property`/`-read`/
    // `-write` tag).
    pub(in crate::registry) case_refs: Vec<(Name, Name)>,
    // `set_ty`'s case_refs equivalent - `property_hook_set_type` never
    // collected these, unlike the sibling `ty` field above.
    pub(in crate::registry) set_case_refs: Vec<(Name, Name)>,
    pub(in crate::registry) attributes: Vec<ReflectionAttribute>,
    pub(in crate::registry) provenance: Provenance,
}

impl PropertyReflection {
    pub fn ty(&self) -> &Type {
        &self.ty
    }

    pub fn visibility(&self) -> Visibility {
        self.visibility
    }

    pub fn is_internal(&self) -> bool {
        self.is_internal
    }

    pub fn deprecated(&self) -> &Option<String> {
        &self.deprecated
    }

    pub fn attributes(&self) -> &Vec<ReflectionAttribute> {
        &self.attributes
    }

    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }

    pub fn is_static(&self) -> bool {
        self.is_static
    }

    pub fn is_readonly(&self) -> bool {
        self.is_readonly
    }

    pub fn allows_private_mutation(&self) -> bool {
        self.allows_private_mutation
    }

    pub fn readonly_by_phpdoc(&self) -> bool {
        self.readonly_by_phpdoc
    }

    pub fn tag_read_only(&self) -> bool {
        self.tag_read_only
    }

    pub fn tag_write_only(&self) -> bool {
        self.tag_write_only
    }

    pub fn write_visibility(&self) -> Visibility {
        self.write_visibility
    }

    pub fn implicit_protected_set(&self) -> bool {
        self.implicit_protected_set
    }

    pub fn set_ty(&self) -> &Option<Type> {
        &self.set_ty
    }

    pub fn is_virtual(&self) -> bool {
        self.is_virtual
    }

    pub fn has_default(&self) -> bool {
        self.has_default
    }

    pub fn is_abstract(&self) -> bool {
        self.is_abstract
    }

    pub fn has_get_hook(&self) -> bool {
        self.has_get_hook
    }

    pub fn has_set_hook(&self) -> bool {
        self.has_set_hook
    }

    pub fn ty_native(&self) -> &Type {
        &self.ty_native
    }

    pub fn doc_ty(&self) -> &Option<Type> {
        &self.doc_ty
    }

    pub fn has_native_type(&self) -> bool {
        self.has_native_type
    }

    pub fn case_refs(&self) -> &Vec<(Name, Name)> {
        &self.case_refs
    }

    pub fn set_case_refs(&self) -> &Vec<(Name, Name)> {
        &self.set_case_refs
    }
}

// A class constant's `public`/`private`/`protected` modifier (PHP 7.1+).
// Mirrors `PropertyReflection`'s shape, minus the property-only concerns
// (`is_static`/`is_readonly`/`set_ty`) a constant has no equivalent of.
#[derive(Debug, Clone)]
pub struct ConstantReflection {
    pub(in crate::registry) ty: Type,
    pub(in crate::registry) visibility: Visibility,
    // `final const` (PHP 8.1+).
    pub(in crate::registry) is_final: bool,
    // The PHP 8.3 native type declaration, resolved; `None` when untyped.
    pub(in crate::registry) native_ty: Option<Type>,
    // Whether `ty` comes from an `@var` tag rather than the value.
    pub(in crate::registry) has_phpdoc_type: bool,
    // The declared value's own type, regardless of any `@var`.
    pub(in crate::registry) value_ty: Type,
    // `@internal` on this constant's own doc comment - direct mirror of
    // `PropertyReflection::is_internal`. Always `false` for an enum case (no
    // doc-comment/`@internal` convention of its own).
    pub(in crate::registry) is_internal: bool,
    // `@deprecated` on this constant's own doc comment - direct mirror of
    // `PropertyReflection::deprecated`. Always `None` for an enum case.
    pub(in crate::registry) deprecated: Option<String>,
    pub(in crate::registry) attributes: Vec<ReflectionAttribute>,
    pub(in crate::registry) provenance: Provenance,
}

impl ConstantReflection {
    pub fn ty(&self) -> &Type {
        &self.ty
    }

    pub fn visibility(&self) -> Visibility {
        self.visibility
    }

    pub fn is_final(&self) -> bool {
        self.is_final
    }

    pub fn native_ty(&self) -> &Option<Type> {
        &self.native_ty
    }

    pub fn has_phpdoc_type(&self) -> bool {
        self.has_phpdoc_type
    }

    pub fn value_ty(&self) -> &Type {
        &self.value_ty
    }

    pub fn is_internal(&self) -> bool {
        self.is_internal
    }

    pub fn deprecated(&self) -> &Option<String> {
        &self.deprecated
    }

    pub fn attributes(&self) -> &Vec<ReflectionAttribute> {
        &self.attributes
    }

    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// A trait method's alias from a trait-use block (`use T { m as n; }`):
/// `trait_name` resolved, `None` when the clause names no trait.
#[derive(Debug, Clone)]
pub struct TraitAlias {
    pub trait_name: Option<Name>,
    pub method: Name,
    pub alias: Name,
}

#[derive(Debug, Clone)]
#[allow(clippy::type_complexity)]
pub struct ClassReflection {
    pub(in crate::registry) kind: ClassKind,
    pub(in crate::registry) is_abstract: bool,
    pub(in crate::registry) is_final: bool,
    // `@final`/`@phpstan-final` on this class's own docblock, without
    // the native `final` keyword (which `is_final` covers).
    pub(in crate::registry) is_final_by_phpdoc: bool,
    // `readonly class` (PHP 8.2). Always `false` for an enum (real PHP
    // rejects `readonly enum` at parse time) or a trait composed in (a
    // trait has no `readonly`-ness of its own to inherit; `readonly
    // class Foo { use T; }` only ever makes `Foo`'s properties readonly,
    // never retroactively marks `T`).
    pub(in crate::registry) is_readonly: bool,
    // `@phpstan-immutable`/`@phpstan-readonly` on this class's own
    // docblock. Never baked with parent-inheritance
    // (`Registry::class_is_immutable` walks `extends` at query time
    // instead) - this field alone only answers "did this exact
    // declaration carry the tag," not "is this class immutable
    // considering its whole ancestor chain."
    pub(in crate::registry) is_immutable: bool,
    pub(in crate::registry) extends: Vec<Name>,
    pub(in crate::registry) implements: Vec<Name>,
    pub(in crate::registry) methods: FxHashMap<Name, FunctionReflection>,
    // The own signature of each method whose name the body declares more
    // than once, keyed by its name's position: `methods` holds their
    // union.
    pub(in crate::registry) redeclared_methods: FxHashMap<(u32, u32), FunctionReflection>,
    pub(in crate::registry) properties: FxHashMap<Name, PropertyReflection>,
    // Declaration order is kept: a listing of constants (a `match`
    // remainder, a `Foo::X_*` wildcard) follows source order, as in real
    // PHPStan.
    pub(in crate::registry) consts: IndexMap<Name, ConstantReflection>,
    pub(in crate::registry) uses: Vec<Name>,
    // Method names declared `abstract` in this class's own body (not
    // walked up the hierarchy - a completeness check needs to walk
    // `extends`/`implements` itself and consult each ancestor's own set).
    pub(in crate::registry) abstract_methods: FxHashSet<Name>,
    // Method names declared `final` in this class's own body.
    pub(in crate::registry) final_methods: FxHashSet<Name>,
    // Method names declared `static` in this class's own body - same
    // "not walked up the hierarchy" shape as `abstract_methods`/
    // `final_methods` above, used by `check_one_method_override`'s
    // `method.nonStatic`/`method.static` check, mirroring real PHPStan's
    // `OverridingMethodRule`: an override must match its ancestor's
    // static-ness exactly, either direction.
    pub(in crate::registry) static_methods: FxHashSet<Name>,
    // Property names declared `final` in this class's own body (PHP 8.4) -
    // same "not walked up the hierarchy" shape as `final_methods` above;
    // `Registry::find_final_ancestor_property` does that walk. Real PHP
    // fatals overriding a `final` property in a subclass exactly like a
    // `final` method.
    pub(in crate::registry) final_properties: FxHashSet<Name>,
    // `@final`'s docblock-only siblings to `final_methods`/
    // `final_properties` above - real PHP never enforces this convention
    // (no engine-level guarantee backs it, unlike the native keyword), so
    // it needs its own explicit ancestor-walk consultation
    // (`Registry::find_final_ancestor_method`/
    // `find_final_ancestor_property`, real keyword taking priority when
    // both somehow apply on the same ancestor).
    pub(in crate::registry) final_methods_by_phpdoc: FxHashSet<Name>,
    pub(in crate::registry) final_properties_by_phpdoc: FxHashSet<Name>,
    // `use A, B { B::hello as helloB; }` - each entry's `trait_name` (when
    // present) is already resolved to the same fully-qualified name
    // `uses` itself is keyed under, so `flatten_traits` can look it up in
    // `self.classes` directly.
    pub(in crate::registry) trait_aliases: Vec<TraitAlias>,
    // Method/property names explicitly mentioned in any trait-adaptation
    // clause (`insteadof` or `as`) - the trait-conflict check treats
    // these as already addressed by the class author, even for
    // `insteadof` (whose actual trait-selection this project doesn't
    // otherwise model).
    pub(in crate::registry) resolved_trait_conflicts: FxHashSet<Name>,
    // This class's own `@template T` names, in declaration order - order
    // matters, since a generic reference (`Collection<User>`) binds its
    // type arguments to these positionally (`checker::
    // class_template_bindings`). Empty for a non-generic class.
    pub(in crate::registry) templates: Vec<Name>,
    // `@extends Parent<Arg, ...>`'s argument list - fixes
    // `extends.first()`'s `@template` names for every instance of this
    // class, independent of whether this class is itself generic (e.g.
    // `class IntCollection extends Collection<int> {}`, which isn't
    // generic itself but permanently binds `Collection`'s `T` to `int`).
    // An argument can itself reference this class's own `@template` name
    // (`class Box<T> extends Collection<T> {}`) - such an argument is
    // a template `name`, substituted using whatever this
    // class's templates are bound to at each use, not a fixed value.
    // Empty when there's no `@extends` tag, or no native `extends` for
    // it to apply to.
    pub(in crate::registry) extends_args: Vec<Type>,
    // Same idea as `extends_args`, but keyed by which interface each
    // `@implements Iface<Arg, ...>` tag applies to (a class can implement
    // more than one at once) - the key is resolved to the same
    // fully-qualified name `implements` itself uses, so the two agree.
    pub(in crate::registry) implements_args: FxHashMap<Name, Vec<Type>>,
    // Same idea again, but for a generic trait named in an `@use
    // Trait<Arg, ...>` tag - keyed the same way as `implements_args`.
    // Unlike `extends`/`implements`, which are both written on the
    // class's own top-level docblock, real PHPStan resolves `@use` from
    // the `use` statement's own doc comment (`/** @use
    // TemplateTypeTrait<ObjectType> */` sits right above `use
    // TemplateTypeTrait;`, not the class's separate `/** @api */`
    // docblock above it) - see `TraitUse::doc_comment`.
    pub(in crate::registry) uses_args: FxHashMap<Name, Vec<Type>>,
    // `@template T of Bound` - the resolved bound for whichever of
    // `templates` above declared one. Checked by
    // `checker::check_generic_bounds` wherever this class is referenced
    // with concrete type arguments (`Collection<Foo>`).
    pub(in crate::registry) template_bounds: FxHashMap<Name, Type>,
    // `@template T = Default` - the resolved default for whichever of
    // `templates` above declared one, mirroring `template_bounds`'s
    // shape/construction exactly. A generic reference supplying fewer
    // type arguments than `templates.len()` is only a real arity error
    // (`check_generic_reference_arity`, `checker/generics.rs`) for a
    // template name absent from this map - one with a default is
    // legitimately optional to supply.
    pub(in crate::registry) template_defaults: FxHashMap<Name, Type>,
    // Names from `templates` above additionally declared
    // `@template-covariant`/`@template-contravariant` - a name absent
    // from both stays invariant.
    pub(in crate::registry) covariant_templates: FxHashSet<Name>,
    pub(in crate::registry) contravariant_templates: FxHashSet<Name>,
    // This class/interface/trait's own docblock carries `@api` (or,
    // equivalently, `@api-do-not-implement`, which implies it - see
    // `DocBlock::api`'s doc comment). Consulted only by
    // `checker::phpstan_api`, and only ever meaningful for a class this
    // project didn't declare itself (one loaded from a real
    // `phpstan/phpstan` `vendor/` tree as context) - `false` for every
    // ordinary class.
    pub(in crate::registry) is_api: bool,
    // `@api-do-not-implement` specifically - an `@api` interface/class a
    // third party must not `extends`/`implements`, even though calling
    // its already-declared members is still fully covered by `is_api`
    // above.
    pub(in crate::registry) is_api_do_not_implement: bool,
    // `@internal` on this class/interface/trait/enum's own docblock -
    // see `docblock::DocBlock::internal`'s doc comment.
    pub(in crate::registry) is_internal: bool,
    // `@phpstan-consistent-constructor` on this class's own docblock -
    // see `docblock::DocBlock::consistent_constructor`'s doc comment.
    pub(in crate::registry) is_consistent_constructor: bool,
    // `@deprecated` on this class's own docblock - consulted only at
    // `new Foo()` (`checker::check_deprecated_instantiation`).
    // Deliberately doesn't imply anything about this class's own members
    // (see `FunctionReflection::deprecated`'s doc comment for why that question
    // is left unclaimed) - this is purely "is the class itself tagged."
    pub(in crate::registry) deprecated: Option<String>,
    // This class's own `#[\Attribute(flags)]` meta-attribute flags (an
    // `int-mask-of<Attribute::TARGET_*>`), if this class is declared
    // usable as an attribute class at all - `None` means it isn't (real
    // PHP's `ReflectionClass::isAttribute()` would be `false`), distinct
    // from what a bare `#[\Attribute]` with no arguments means
    // (`Attribute::TARGET_ALL`, matching `Attribute::__construct`'s own
    // default parameter value - see `compute_attribute_targets`'s doc
    // comment).
    pub(in crate::registry) attribute_targets: Option<u32>,
    // This class's own attributes, names resolved.
    pub(in crate::registry) attributes: Vec<ReflectionAttribute>,
    // For a backed enum only, each case's literal backing value
    // (`StringLiteral`/`IntLiteral`), keyed by case name - lets
    // `::from()`/`::tryFrom()` narrow a literal-argument call to the
    // exact matching case instead of the coarse
    // `self`/`?self`. Only populated when every case's value is a bare
    // literal (mirrors the "don't guess" restriction the existing
    // `->value` union-of-literals property type applies) - empty for a
    // non-enum, an unbacked enum, or a backed enum with any non-literal
    // case value (a case naming a class constant).
    pub(in crate::registry) enum_case_values: FxHashMap<Name, Type>,
    // This class's own `@phpstan-type Name TypeExpr` local alias table
    // (`function_sig::build_type_aliases`), keyed by each alias's own
    // resolved FQN - already spliced into every member's `ty` at
    // `from_decl` time (see `substitute_type_aliases`'s doc comment), but
    // kept here too for `Checker::inline_var_annotation`'s separate,
    // check-time `@var` resolution path, which needs the same table to
    // expand a local variable annotated `/** @var Name $x */` inside one
    // of this class's method bodies. Empty for a class with no such tags.
    pub(in crate::registry) type_aliases: FxHashMap<Name, Type>,
    // `ParameterReflection::case_refs`'s `@phpstan-type` equivalent - keyed the
    // same way `type_aliases` itself is (the alias's resolved local
    // name), `(raw, resolved)` pairs for every class-name reference the
    // alias's own RHS text carries (not a reference to the alias
    // elsewhere - that's this same mechanism at whatever
    // `@param`/`@return`/property tag actually uses the alias, via its
    // own `case_refs`).
    pub(in crate::registry) type_alias_case_refs: FxHashMap<Name, Vec<(Name, Name)>>,
    // This class's own `@phpstan-import-type` tags, name-resolved
    // (`from` to the source class's FQN, the imported name to the local
    // key this class's members already reference it by) but not yet
    // looked up in the source class's `type_aliases` - the source class
    // may not be indexed yet at `from_decl` time. Resolved by
    // `Registry::resolve_deferred_type_imports`, a later pass over the
    // fully-indexed registry - merged into `type_aliases` above once
    // resolved, so this field is empty again after that pass runs; never
    // read afterward.
    pub(in crate::registry) type_imports: Vec<ResolvedTypeImport>,
    // `@method` tags, already resolved into real `FunctionReflection`s but not
    // yet inserted into `methods` - insertion has to wait for
    // `Registry::resolve_method_tags`, a later pass. Empty again after
    // that pass runs; never read afterward.
    pub(in crate::registry) method_tags: Vec<(Name, FunctionReflection, bool)>,
    // `#[ORM\Entity]` metadata - `None`
    // for the overwhelming majority of classes, which aren't Doctrine
    // entities at all.
    pub(in crate::registry) doctrine_entity: Option<DoctrineEntity>,
    // `@phpstan-require-extends`/`@phpstan-require-implements`/
    // `@phpstan-sealed` - resolved directly to fully-qualified class
    // names (a tag's value can legally be a union of several, already
    // flattened here by `object_class_names`; a non-`Object` member,
    // e.g. an unresolvable name, is silently dropped, matching this
    // project's "don't guess" precedent elsewhere). See
    // `checker::oop::check_require_tags`/`check_sealed_hierarchy` for
    // the two, differently-shaped consuming checks.
    pub(in crate::registry) require_extends: Vec<Name>,
    pub(in crate::registry) require_implements: Vec<Name>,
    pub(in crate::registry) sealed: Vec<Name>,
    /// The class's own `php-language-extensions` access attributes.
    pub(in crate::registry) access: crate::registry::access_attributes::AccessAttributes,
    /// Each method's, keyed by lowercased method name (only methods that
    /// have any).
    pub(in crate::registry) method_access:
        FxHashMap<Name, crate::registry::access_attributes::AccessAttributes>,
    // `@mixin Type` - resolved FQNs, same shape/flattening as the three
    // above. Not yet looked up in the target's own `methods`/`properties`
    // (may not be indexed yet) - `Registry::resolve_deferred_mixins`,
    // mirroring `resolve_deferred_type_imports`'s own two-step shape.
    pub(in crate::registry) mixins: Vec<Name>,
    // `ParameterReflection::case_refs`'s `@mixin`-tag equivalent - `(raw, resolved)`
    // pairs for every `@mixin` target on this class's docblock, in
    // declaration order (not keyed by name - unlike
    // `type_alias_case_refs`, a class can carry several `@mixin` tags
    // with no unique name to key by).
    pub(in crate::registry) mixin_case_refs: Vec<(Name, Name)>,
    // A `new class { ... }` expression's class.
    pub(in crate::registry) is_anonymous: bool,
    pub(in crate::registry) provenance: Provenance,
}

impl ClassReflection {
    pub fn is_anonymous(&self) -> bool {
        self.is_anonymous
    }

    pub fn kind(&self) -> ClassKind {
        self.kind
    }

    pub fn is_abstract(&self) -> bool {
        self.is_abstract
    }

    pub fn is_final(&self) -> bool {
        self.is_final
    }

    pub fn is_final_by_phpdoc(&self) -> bool {
        self.is_final_by_phpdoc
    }

    pub fn is_readonly(&self) -> bool {
        self.is_readonly
    }

    pub fn is_immutable(&self) -> bool {
        self.is_immutable
    }

    pub fn extends(&self) -> &Vec<Name> {
        &self.extends
    }

    pub fn implements(&self) -> &Vec<Name> {
        &self.implements
    }

    pub fn methods(&self) -> &FxHashMap<Name, FunctionReflection> {
        &self.methods
    }

    pub fn redeclared_methods(&self) -> &FxHashMap<(u32, u32), FunctionReflection> {
        &self.redeclared_methods
    }

    pub fn properties(&self) -> &FxHashMap<Name, PropertyReflection> {
        &self.properties
    }

    pub fn consts(&self) -> &IndexMap<Name, ConstantReflection> {
        &self.consts
    }

    pub fn uses(&self) -> &Vec<Name> {
        &self.uses
    }

    pub fn abstract_methods(&self) -> &FxHashSet<Name> {
        &self.abstract_methods
    }

    pub fn final_methods(&self) -> &FxHashSet<Name> {
        &self.final_methods
    }

    pub fn static_methods(&self) -> &FxHashSet<Name> {
        &self.static_methods
    }

    pub fn final_properties(&self) -> &FxHashSet<Name> {
        &self.final_properties
    }

    pub fn final_methods_by_phpdoc(&self) -> &FxHashSet<Name> {
        &self.final_methods_by_phpdoc
    }

    pub fn final_properties_by_phpdoc(&self) -> &FxHashSet<Name> {
        &self.final_properties_by_phpdoc
    }

    pub fn trait_aliases(&self) -> &Vec<TraitAlias> {
        &self.trait_aliases
    }

    pub fn resolved_trait_conflicts(&self) -> &FxHashSet<Name> {
        &self.resolved_trait_conflicts
    }

    pub fn templates(&self) -> &Vec<Name> {
        &self.templates
    }

    pub fn extends_args(&self) -> &Vec<Type> {
        &self.extends_args
    }

    pub fn implements_args(&self) -> &FxHashMap<Name, Vec<Type>> {
        &self.implements_args
    }

    pub fn uses_args(&self) -> &FxHashMap<Name, Vec<Type>> {
        &self.uses_args
    }

    pub fn template_bounds(&self) -> &FxHashMap<Name, Type> {
        &self.template_bounds
    }

    pub fn template_defaults(&self) -> &FxHashMap<Name, Type> {
        &self.template_defaults
    }

    pub fn covariant_templates(&self) -> &FxHashSet<Name> {
        &self.covariant_templates
    }

    pub fn contravariant_templates(&self) -> &FxHashSet<Name> {
        &self.contravariant_templates
    }

    pub fn is_api(&self) -> bool {
        self.is_api
    }

    pub fn is_api_do_not_implement(&self) -> bool {
        self.is_api_do_not_implement
    }

    pub fn is_internal(&self) -> bool {
        self.is_internal
    }

    pub fn is_consistent_constructor(&self) -> bool {
        self.is_consistent_constructor
    }

    pub fn deprecated(&self) -> &Option<String> {
        &self.deprecated
    }

    pub fn attribute_targets(&self) -> Option<u32> {
        self.attribute_targets
    }

    pub fn attributes(&self) -> &Vec<ReflectionAttribute> {
        &self.attributes
    }

    pub fn enum_case_values(&self) -> &FxHashMap<Name, Type> {
        &self.enum_case_values
    }

    pub fn type_aliases(&self) -> &FxHashMap<Name, Type> {
        &self.type_aliases
    }

    #[allow(clippy::type_complexity)]
    pub fn type_alias_case_refs(&self) -> &FxHashMap<Name, Vec<(Name, Name)>> {
        &self.type_alias_case_refs
    }

    pub fn type_imports(&self) -> &Vec<ResolvedTypeImport> {
        &self.type_imports
    }

    pub fn method_tags(&self) -> &Vec<(Name, FunctionReflection, bool)> {
        &self.method_tags
    }

    pub fn doctrine_entity(&self) -> &Option<DoctrineEntity> {
        &self.doctrine_entity
    }

    pub fn require_extends(&self) -> &Vec<Name> {
        &self.require_extends
    }

    pub fn require_implements(&self) -> &Vec<Name> {
        &self.require_implements
    }

    pub fn sealed(&self) -> &Vec<Name> {
        &self.sealed
    }

    pub fn access(&self) -> &crate::registry::access_attributes::AccessAttributes {
        &self.access
    }

    pub fn method_access(
        &self,
    ) -> &FxHashMap<Name, crate::registry::access_attributes::AccessAttributes> {
        &self.method_access
    }

    pub fn mixins(&self) -> &Vec<Name> {
        &self.mixins
    }

    pub fn mixin_case_refs(&self) -> &Vec<(Name, Name)> {
        &self.mixin_case_refs
    }

    pub fn provenance(&self) -> &Provenance {
        &self.provenance
    }
}

/// See `ClassReflection::type_imports`'s own doc comment.
#[derive(Debug, Clone)]
pub struct ResolvedTypeImport {
    // The key this class's own already-built member types reference the
    // import under (`substitute_type_aliases` matches on this exact key,
    // same as an ordinary local `@phpstan-type` alias would).
    pub local_key: Name,
    pub source_class_fqn: Name,
    // The name as declared on the *source* class's own `@phpstan-type` -
    // not yet resolved to that class's own key (needs its own namespace,
    // reconstructed from `source_class_fqn` at resolution time rather
    // than carried here, since a class's own containing namespace is
    // always recoverable from its own FQN).
    pub source_alias_name: Name,
}

/// Same "two full declarations of one name collapse into their positional
/// union" rule `Registry::insert_built_decl`'s free-function path applies,
/// since phpstorm-stubs represents a method's version-split/overload the
/// identical way inside one class body (`DatePeriod::__construct`'s four
/// declarations, `ReflectionMethod::setAccessible()`'s pre-/post-8.0
/// split), but unlike a free function's declarations (seen one at a
/// time as the registry walks top-level statements) a class's
/// `c.methods`/`e.methods` arrives as one already-parsed batch - a bare
/// `.collect()` into a `HashMap` would silently keep only whichever
/// declaration happens to be last in source order and drop every
/// alternate parameter list the earlier declaration(s) carried
/// (`DatePeriod::__construct($start, $interval, $end)` - the stub's own
/// first, 3-required-arg overload - vanishes entirely, since the file's
/// last declaration is the unrelated `($isostr, $options = 0)` form).
///
/// Also returns each such declaration's own signature, keyed by its
/// name's position (`ClassReflection::redeclared_methods`).
#[allow(clippy::type_complexity)]
fn collect_merged_methods(
    methods: Vec<(Name, (u32, u32), FunctionReflection)>,
) -> (
    FxHashMap<Name, FunctionReflection>,
    FxHashMap<(u32, u32), FunctionReflection>,
) {
    let mut counts: FxHashMap<&str, usize> = FxHashMap::default();
    for (name, _, _) in &methods {
        *counts.entry(name.as_ref()).or_default() += 1;
    }
    let redeclared: FxHashMap<(u32, u32), FunctionReflection> = methods
        .iter()
        .filter(|(name, _, _)| counts[name.as_ref()] > 1)
        .map(|(_, site, sig)| (*site, sig.clone()))
        .collect();
    let mut out: FxHashMap<Name, FunctionReflection> = FxHashMap::default();
    for (name, _, sig) in methods {
        match out.remove(&name) {
            Some(existing) => {
                out.insert(name, existing.merge(sig));
            }
            None => {
                out.insert(name, sig);
            }
        }
    }
    (out, redeclared)
}

fn name_site(f: &syntax::ast::FunctionDecl) -> (u32, u32) {
    (f.name.span.start.line, f.name.span.start.col)
}

/// Each declared member's name span, by kind: what `ClassReflection::
/// record_provenance` stamps the members built from one declaration with.
#[derive(Default)]
pub(crate) struct MemberSpans<'a> {
    pub(crate) methods: FxHashMap<&'a str, Span>,
    pub(crate) method_sites: FxHashMap<(u32, u32), Span>,
    pub(crate) properties: FxHashMap<&'a str, Span>,
    pub(crate) consts: FxHashMap<&'a str, Span>,
}

impl<'a> MemberSpans<'a> {
    fn add_methods(&mut self, methods: &'a [syntax::ast::MethodDecl]) {
        for m in methods {
            self.methods.insert(&m.function.name, m.function.name.span);
            self.method_sites
                .insert(name_site(&m.function), m.function.name.span);
            for p in &m.function.params {
                if p.promotion.is_some() {
                    self.properties.insert(&p.name, p.name.span);
                }
            }
        }
    }

    fn add_consts(&mut self, consts: &'a [syntax::ast::ClassConst]) {
        for c in consts {
            self.consts.insert(&c.name, c.name.span);
        }
    }

    pub(crate) fn of_class(c: &'a ClassDef) -> MemberSpans<'a> {
        let mut spans = MemberSpans::default();
        spans.add_methods(&c.methods);
        spans.add_consts(&c.consts);
        for p in &c.properties {
            spans.properties.insert(&p.name, p.name.span);
        }
        for case in &c.enum_cases {
            spans.consts.insert(&case.name, case.name.span);
        }
        spans
    }
}

impl ClassReflection {
    /// Records `origin` on class `class`, declared at `name_span`, and on
    /// every member it builds, each at its own name's span from `spans`;
    /// a member with no declaration of its own (one synthesized from a
    /// tag, or built in) gets the class name's.
    pub(crate) fn record_provenance(
        &mut self,
        origin: &Origin,
        class: &Name,
        name_span: Span,
        spans: &MemberSpans,
    ) {
        let at = |span: Option<&Span>| {
            Provenance::member(origin.clone(), *span.unwrap_or(&name_span), class.clone())
        };
        self.provenance = Provenance::new(origin.clone(), name_span);
        for (name, sig) in self.methods.iter_mut() {
            sig.provenance = at(spans.methods.get(name.as_ref()));
        }
        for (site, sig) in self.redeclared_methods.iter_mut() {
            sig.provenance = at(spans.method_sites.get(site));
        }
        for (_, sig, _) in self.method_tags.iter_mut() {
            sig.provenance = at(None);
        }
        for (name, sig) in self.properties.iter_mut() {
            sig.provenance = at(spans.properties.get(name.as_ref()));
        }
        for (name, sig) in self.consts.iter_mut() {
            sig.provenance = at(spans.consts.get(name.as_ref()));
        }
    }

    pub(crate) fn from_decl(c: &ClassDecl, ctx: &NameContext) -> ClassReflection {
        Self::from_def(c.fq_name(), &c.def, ctx)
    }

    /// The reflection of class definition `c`, which declares class
    /// `name`.
    #[hotpath::measure]
    pub(crate) fn from_def(name: &str, c: &ClassDef, ctx: &NameContext) -> ClassReflection {
        let class_doc: Option<DocBlock> = c.doc_comment.as_deref().map(docblock::parse);
        // An enum can't be generic; its `@template` tags declare nothing.
        let template_doc = class_doc.as_ref().filter(|_| c.kind != ClassKind::Enum);
        let templates: Vec<Name> = template_doc
            .map(|d| d.templates.iter().map(Name::from).collect())
            .unwrap_or_default();
        // This class's `@template` metadata (bound/variance/default),
        // keyed by name - `templates` above stays a plain `Vec<String>`
        // (positional order still drives type-argument binding
        // elsewhere, e.g. `checker::class_template_bindings_rec`), but
        // every template this class's docblock produces
        // (properties, consts, `@extends`/`@implements` args, and every
        // method's params/return type below) carries the real metadata
        // from here instead of a bare name.
        let class_scope = name.to_string();
        let template_params: FxHashMap<String, TemplateParam> = template_doc
            .map(|d| build_template_params(d, &class_scope, ctx))
            .unwrap_or_default();
        // `@phpstan-type Name TypeExpr` local aliases - resolved once,
        // here, and spliced into every member's `ty` below (never
        // `ty_native`, which by definition never reflects a docblock) via
        // `substitute_type_aliases`. See that function's doc comment for
        // why this must run after `resolve_names_in_type` on each
        // member's surrounding type, not before.
        let (type_aliases, type_alias_case_refs) =
            build_type_aliases(class_doc.as_ref(), &template_params, ctx);
        // `@phpstan-import-type` - resolved just enough here (both names,
        // via this class's `ctx`) to be looked up in the source class's
        // `type_aliases` once it's indexed too; see `ResolvedTypeImport`'s
        // doc comment.
        let type_imports: Vec<ResolvedTypeImport> = class_doc
            .as_ref()
            .map(|d| {
                d.type_imports
                    .iter()
                    .map(|imp| {
                        let local_name = imp.as_name.as_deref().unwrap_or(&imp.alias);
                        ResolvedTypeImport {
                            local_key: Name::from(&ctx.resolve(local_name)),
                            source_class_fqn: Name::from(&ctx.resolve(&imp.from)),
                            source_alias_name: Name::from(&imp.alias),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        // A single pass over `c.methods` builds `methods` and all 4
        // name-only `HashSet`s together - each method's own name is
        // cloned only for the sets it actually belongs to, instead of
        // every method in the class being filtered/name-cloned once per
        // set across 5 separate iterations.
        let mut methods_vec: Vec<(Name, (u32, u32), FunctionReflection)> =
            Vec::with_capacity(c.methods.len());
        let mut abstract_methods: FxHashSet<Name> = FxHashSet::default();
        let mut final_methods: FxHashSet<Name> = FxHashSet::default();
        let mut static_methods: FxHashSet<Name> = FxHashSet::default();
        let mut final_methods_by_phpdoc: FxHashSet<Name> = FxHashSet::default();
        for m in &c.methods {
            let name = Name::from(&m.function.name);
            if m.is_abstract {
                abstract_methods.insert(name.clone());
            }
            // A `private` method isn't part of a class's inherited/
            // overridable surface at all in real PHP - a subclass
            // declaring its own same-named private method is an
            // independent, unrelated declaration, not an override - so
            // `final private` doesn't actually restrict anything a
            // subclass does (real PHP raises no error for
            // `Exception::__clone()`, `final private`, being freely
            // redeclared by subclasses).
            if m.is_final && m.visibility != Visibility::Private {
                final_methods.insert(name.clone());
            }
            if m.is_static {
                static_methods.insert(name.clone());
            }
            // `@final`'s docblock-only sibling to `final_methods` above -
            // same private-is-not-overridable exemption (a purely static-
            // analysis convention real PHP itself never enforces, so it
            // needs its own explicit ancestor walk rather than
            // piggybacking on the native keyword's compile-time
            // guarantee).
            if m.visibility != Visibility::Private
                && m.function
                    .doc_comment
                    .as_deref()
                    .map(docblock::parse)
                    .is_some_and(|d| d.is_final)
            {
                final_methods_by_phpdoc.insert(name.clone());
            }
            let sig = FunctionReflection::from_decl(
                &m.function,
                ctx,
                &template_params,
                &type_aliases,
                m.visibility,
            );
            methods_vec.push((name.clone(), name_site(&m.function), sig));
        }
        let (methods, redeclared_methods) = collect_merged_methods(methods_vec);
        // Same private-is-not-overridable exemption `final_methods`
        // above applies, one member kind over - a private property
        // (final or not) is never part of a subclass's overridable
        // surface either.
        let mut final_properties: FxHashSet<Name> = c
            .properties
            .iter()
            .filter(|p| p.is_final && p.visibility != Visibility::Private)
            .map(|p| Name::from(&p.name))
            .collect();
        // `public final int $x` is real, valid PHP on a promoted
        // constructor property too, and real PHP does enforce "Cannot
        // override final property" for it - unlike `abstract`, which
        // really is impossible there (an abstract constructor can't
        // declare any promoted property, a real fatal error).
        if let Some(ctor) = c.constructor() {
            final_properties.extend(
                ctor.function
                    .params
                    .iter()
                    .filter(|p| {
                        p.promotion.is_some_and(|promotion| promotion.is_final)
                            && p.promotion.map(|promotion| promotion.visibility).as_ref()
                                != Some(&Visibility::Private)
                    })
                    .map(|p| Name::from(&p.name)),
            );
        }

        // `@final`'s docblock-only sibling to `final_properties` above -
        // same private-is-not-overridable exemption.
        let mut final_properties_by_phpdoc: FxHashSet<Name> = c
            .properties
            .iter()
            .filter(|p| {
                p.visibility != Visibility::Private
                    && p.doc_comment
                        .as_deref()
                        .map(docblock::parse)
                        .is_some_and(|d| d.is_final)
            })
            .map(|p| Name::from(&p.name))
            .collect();
        if let Some(ctor) = c.constructor() {
            final_properties_by_phpdoc.extend(
                ctor.function
                    .params
                    .iter()
                    .filter(|p| {
                        p.promotion.map(|promotion| promotion.visibility).as_ref()
                            != Some(&Visibility::Private)
                            && p.doc_comment
                                .as_deref()
                                .map(docblock::parse)
                                .is_some_and(|d| d.is_final)
                    })
                    .map(|p| Name::from(&p.name)),
            );
        }

        let mut properties: FxHashMap<Name, PropertyReflection> = c
            .properties
            .iter()
            .map(|p| {
                let doc_ty = p.doc_comment.as_deref().map(docblock::parse);
                let DeclaredType {
                    ty,
                    native: ty_native,
                    case_refs,
                } = declared_type(
                    doc_ty.as_ref().and_then(|d| d.var_type.as_ref()),
                    p.type_hint.as_ref(),
                    &template_params,
                    ctx,
                );
                let ty = substitute_type_aliases(ty, &type_aliases);
                let own_doc_ty = doc_ty
                    .as_ref()
                    .and_then(|d| d.var_type.as_ref())
                    .map(|raw| {
                        let t = mark_template_params(parse_type_node(raw), &template_params);
                        substitute_type_aliases(resolve_names_in_type(t, ctx), &type_aliases)
                    });
                let (set_ty, set_case_refs) =
                    property_hook_set_type(p.hooks.as_ref(), &template_params, ctx);
                let is_virtual = p
                    .hooks
                    .as_ref()
                    .is_some_and(|h| !hooks_reference_backing_storage(h, &p.name));
                (
                    Name::from(&p.name),
                    PropertyReflection {
                        attributes: ReflectionAttribute::all(&p.attrs),
                        provenance: Provenance::default(),
                        ty,
                        ty_native,
                        doc_ty: own_doc_ty,
                        has_native_type: p.type_hint.is_some(),
                        is_static: p.is_static,
                        // `@readonly` (`DocBlock::readonly`'s doc
                        // comment) folds into the exact same flag native
                        // `readonly` sets - both flow through identical
                        // downstream enforcement.
                        is_readonly: p.is_readonly || doc_ty.as_ref().is_some_and(|d| d.readonly),
                        allows_private_mutation: !p.is_readonly
                            && doc_ty
                                .as_ref()
                                .is_some_and(|d| d.readonly && d.allow_private_mutation),
                        readonly_by_phpdoc: !p.is_readonly
                            && doc_ty.as_ref().is_some_and(|d| d.readonly),
                        tag_read_only: false,
                        tag_write_only: false,
                        set_ty,
                        is_virtual,
                        visibility: p.visibility,
                        write_visibility: p.write_visibility.unwrap_or(p.visibility),
                        implicit_protected_set: p.is_readonly
                            && p.write_visibility.is_none()
                            && p.visibility == Visibility::Public,
                        is_internal: doc_ty.as_ref().is_some_and(|d| d.internal),
                        deprecated: doc_ty.as_ref().and_then(|d| d.deprecated.clone()),
                        has_default: p.default.is_some(),
                        is_abstract: p.is_abstract,
                        has_get_hook: p.hooks.as_ref().is_some_and(|h| h.get.is_some()),
                        has_set_hook: p.hooks.as_ref().is_some_and(|h| h.set.is_some()),
                        case_refs,
                        set_case_refs,
                    },
                )
            })
            .collect();

        // Constructor property promotion: `public function __construct(
        // private int $id) {}` declares `$id` as a property, not just a
        // parameter - the parser records this via Param::promoted_visibility.
        // A `@param` on the constructor's own docblock describes the same
        // promoted property, so it gets the same PHPDoc-wins treatment.
        if let Some(ctor) = c.constructor() {
            let ctor_doc = ctor.function.doc_comment.as_deref().map(docblock::parse);
            for p in &ctor.function.params {
                if p.promotion.is_none() {
                    continue;
                }
                // Same inline-`@var`-on-a-promoted-property fallback as
                // `FunctionReflection::from_decl`'s param loop - needed here too,
                // separately: this builds the property's `PropertyReflection`
                // (read by `$this->details`-style property access
                // elsewhere in the class), a completely different
                // construction from the `ParameterReflection` `FunctionReflection::from_decl`
                // builds for the same parameter.
                let doc_ty: Option<TypeNode> = ctor_doc
                    .as_ref()
                    .and_then(|d| d.params.get(p.name.as_ref()))
                    .cloned()
                    .or_else(|| {
                        p.doc_comment
                            .as_deref()
                            .map(docblock::parse)
                            .and_then(|d| d.var_type)
                    });
                let DeclaredType {
                    ty,
                    native: ty_native,
                    case_refs,
                } = declared_type(doc_ty.as_ref(), p.type_hint.as_ref(), &template_params, ctx);
                let ty = substitute_type_aliases(ty, &type_aliases);
                let own_doc_ty = doc_ty.as_ref().map(|raw| {
                    let t = mark_template_params(parse_type_node(raw), &template_params);
                    substitute_type_aliases(resolve_names_in_type(t, ctx), &type_aliases)
                });
                let ty = apply_implicit_nullable_default(ty, p.default.as_ref());
                let ty_native = apply_implicit_nullable_default(ty_native, p.default.as_ref());
                // A promoted param's own inline `@readonly` - cheap to
                // support given `p.doc_comment` is already parsed above
                // for `@var`'s fallback.
                let param_doc = p.doc_comment.as_deref().map(docblock::parse);
                let doc_readonly = param_doc.as_ref().is_some_and(|d| d.readonly);
                let doc_allows_private_mutation =
                    param_doc.as_ref().is_some_and(|d| d.allow_private_mutation);
                // A promoted constructor property can carry its own
                // inline PHP 8.4 hooks (`Param::hooks`) exactly like a
                // plain property declaration can - resolved through the
                // identical generalized helpers.
                let (set_ty, set_case_refs) =
                    property_hook_set_type(p.hooks.as_deref(), &template_params, ctx);
                let is_virtual = p
                    .hooks
                    .as_deref()
                    .is_some_and(|h| !hooks_reference_backing_storage(h, &p.name));
                properties.insert(
                    Name::from(&p.name),
                    PropertyReflection {
                        attributes: ReflectionAttribute::all(&p.attrs),
                        provenance: Provenance::default(),
                        ty,
                        ty_native,
                        doc_ty: own_doc_ty,
                        has_native_type: p.type_hint.is_some(),
                        is_static: false,
                        is_readonly: p.promotion.is_some_and(|promotion| promotion.readonly)
                            || doc_readonly,
                        allows_private_mutation: !p
                            .promotion
                            .is_some_and(|promotion| promotion.readonly)
                            && doc_readonly
                            && doc_allows_private_mutation,
                        readonly_by_phpdoc: !p
                            .promotion
                            .is_some_and(|promotion| promotion.readonly)
                            && doc_readonly,
                        tag_read_only: false,
                        tag_write_only: false,
                        set_ty,
                        is_virtual,
                        visibility: p
                            .promotion
                            .map(|promotion| promotion.visibility)
                            .unwrap_or(Visibility::Public),
                        write_visibility: p
                            .promotion
                            .map(|promotion| promotion.visibility)
                            .unwrap_or(Visibility::Public),
                        implicit_protected_set: p
                            .promotion
                            .is_some_and(|promotion| promotion.readonly)
                            && p.promotion
                                .map(|promotion| promotion.visibility)
                                .as_ref()
                                .is_none_or(|v| *v == Visibility::Public),
                        is_internal: false,
                        deprecated: None,
                        has_default: p.default.is_some(),
                        is_abstract: false,
                        // Unlike `is_abstract` above (impossible on a
                        // promoted property - an abstract constructor
                        // can't declare one at all), promoted properties
                        // genuinely can carry inline hooks.
                        has_get_hook: p.hooks.as_ref().is_some_and(|h| h.get.is_some()),
                        has_set_hook: p.hooks.as_ref().is_some_and(|h| h.set.is_some()),
                        case_refs,
                        set_case_refs,
                    },
                );
            }
        }

        // `@property`/`@property-read`/`@property-write Type $name` -
        // magic properties with no real declared field. Only fills a
        // name this class doesn't already declare for real - a real
        // property/promoted param always wins, since the tag exists to
        // add virtual members, not override real ones. `@property-read`/
        // `@property-write`'s one-directional access is
        // `PropertyReflection::is_writable`/`is_readable`'s concern.
        if let Some(doc) = &class_doc {
            for tag in &doc.properties {
                properties.entry(Name::from(&tag.name)).or_insert_with(|| {
                    let ty = parse_type_node(&tag.ty);
                    let ty = mark_template_params(ty, &template_params);
                    let mut case_refs = Vec::new();
                    collect_class_name_case_refs(&ty, ctx, &mut case_refs);
                    let ty = resolve_names_in_type(ty, ctx);
                    PropertyReflection {
                        attributes: Vec::new(),
                        provenance: Provenance::default(),
                        doc_ty: None,
                        ty,
                        // A magic `@property` tag has no native
                        // declaration to check against - `false`,
                        // matching every other synthetic-property field
                        // here.
                        ty_native: Type::mixed(),
                        has_native_type: false,
                        is_static: false,
                        is_readonly: tag.readable && !tag.writable,
                        allows_private_mutation: false,
                        readonly_by_phpdoc: false,
                        tag_read_only: tag.readable && !tag.writable,
                        tag_write_only: tag.writable && !tag.readable,
                        set_ty: None,
                        is_virtual: false,
                        visibility: Visibility::Public,
                        write_visibility: Visibility::Public,
                        implicit_protected_set: false,
                        is_internal: false,
                        deprecated: None,
                        // A magic `@property` tag describes a virtual
                        // member with no real backing declaration -
                        // genuinely unknown whether it "has a default,"
                        // so `false` (the safe, never-wrongly-exempting
                        // choice) rather than a guess.
                        has_default: false,
                        is_abstract: false,
                        has_get_hook: false,
                        has_set_hook: false,
                        case_refs,
                        set_case_refs: Vec::new(),
                    }
                });
            }
        }

        // `@method` - a virtual method with no real declaration at all.
        // Deliberately not inserted into `methods` here the way
        // `@property` is above: `methods` at this point only reflects
        // this class's own directly-declared methods, not yet any
        // trait-flattened-in ones (`Registry::flatten_traits` is a
        // separate, later pass) - inserting eagerly here would let a
        // same-named `@method` tag wrongly win over a real trait method
        // that hasn't been merged in yet, since `flatten_traits`'s merge
        // is also vacant-only.
        // `Registry::resolve_method_tags` runs this insertion later
        // instead, once `flatten_traits` has had its own say.
        let method_tags: Vec<(Name, FunctionReflection, bool)> = class_doc
            .as_ref()
            .map(|d| {
                d.method_tags
                    .iter()
                    .map(|tag| {
                        (
                            Name::from(&tag.name),
                            method_tag_sig(tag, &template_params, ctx),
                            tag.is_static,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();

        // A `@var` immediately above a `const` declaration (a common
        // convention for pinning a `const array`'s element type more
        // precisely than either its native hint - not even kept past
        // parsing - or `infer_const_type`'s literal-value inference can)
        // wins over the inferred type the same way a property's `@var`
        // does above - only when actually present, so a plain,
        // undocumented const keeps `infer_const_type`'s existing
        // inference.
        let consts = c
            .consts
            .iter()
            .map(|cc| {
                let doc = cc.doc_comment.as_deref().map(docblock::parse);
                let doc_ty = doc.as_ref().and_then(|d| d.var_type.clone());
                let has_phpdoc_type = doc_ty.is_some();
                let ty = match doc_ty {
                    Some(doc_ty) => {
                        let ty = parse_type_node(&doc_ty);
                        let ty = mark_template_params(ty, &template_params);
                        let ty = resolve_names_in_type(ty, ctx);
                        substitute_type_aliases(ty, &type_aliases)
                    }
                    None => infer_class_const_type(&cc.value),
                };
                (
                    Name::from(&cc.name),
                    ConstantReflection {
                        attributes: ReflectionAttribute::all(&cc.attrs),
                        provenance: Provenance::default(),
                        ty,
                        visibility: cc.visibility,
                        is_final: cc.is_final,
                        native_ty: native_const_type(cc),
                        has_phpdoc_type,
                        value_ty: infer_class_const_type(&cc.value),
                        // `@internal` on this constant's own doc
                        // comment - see `PropertyReflection::is_internal`'s doc
                        // comment for the convention this mirrors.
                        is_internal: doc.as_ref().is_some_and(|d| d.internal),
                        deprecated: doc.as_ref().and_then(|d| d.deprecated.clone()),
                    },
                )
            })
            .collect();

        // `@extends`/`@implements` generic args - an argument can
        // reference this class's own `@template` name (`class Box<T>
        // extends Collection<T> {}`), so it goes through the same
        // mark-then-resolve pipeline as every other member type above,
        // not just a bare `types::parse_type_string`. Also runs the
        // identical `substitute_type_aliases` pass every other member
        // type gets (see `type_aliases`'s doc comment for why it must
        // come after `resolve_names_in_type`, not before): otherwise two
        // classes each declaring a same-named but locally-scoped
        // `@phpstan-type` alias, then referencing it in their own
        // `@implements` tag, would stay distinct unexpanded alias
        // references instead of each expanding to its real underlying shape,
        // making `check_cross_path_interface_conflicts`'s exact-equality
        // comparison see two "different" types where real PHPStan (which
        // always compares the expanded shape) sees an equivalent one.
        let resolve_generic_tag = |raw: &TypeNode| -> Type {
            let ty = parse_type_node(raw);
            let ty = mark_template_params(ty, &template_params);
            let ty = resolve_names_in_type(ty, ctx);
            substitute_type_aliases(ty, &type_aliases)
        };
        let extends_args: Vec<Type> = class_doc
            .as_ref()
            .and_then(|d| d.extends.as_ref())
            .map(|raw| {
                resolve_generic_tag(raw)
                    .as_object()
                    .map(|(_, args)| args.to_vec())
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        let implements_args: FxHashMap<Name, Vec<Type>> = class_doc
            .iter()
            .flat_map(|d| &d.implements)
            .filter_map(|raw| class_and_args(&resolve_generic_tag(raw)))
            .collect();
        // `@use Trait<Arg, ...>` - unlike `extends`/`implements`, lives
        // on the `use` statement's own doc comment, not `c.doc_comment`
        // (`class_doc` above) - see `ClassReflection::uses_args`'s doc comment.
        let uses_args: FxHashMap<Name, Vec<Type>> = c
            .trait_uses
            .iter()
            .filter_map(|u| u.doc_comment.as_deref())
            .map(docblock::parse)
            .flat_map(|d| d.uses)
            .filter_map(|raw| class_and_args(&resolve_generic_tag(&raw)))
            .collect();
        let template_bounds: FxHashMap<Name, Type> = class_doc
            .as_ref()
            .map(|d| {
                d.template_bounds
                    .iter()
                    .map(|(name, raw)| (Name::from(name), resolve_generic_tag(raw)))
                    .collect()
            })
            .unwrap_or_default();
        let template_defaults: FxHashMap<Name, Type> = class_doc
            .as_ref()
            .map(|d| {
                d.template_defaults
                    .iter()
                    .map(|(name, raw)| (Name::from(name), resolve_generic_tag(raw)))
                    .collect()
            })
            .unwrap_or_default();
        let covariant_templates: FxHashSet<Name> = class_doc
            .as_ref()
            .map(|d| d.covariant_templates.iter().map(Name::from).collect())
            .unwrap_or_default();
        let contravariant_templates: FxHashSet<Name> = class_doc
            .as_ref()
            .map(|d| d.contravariant_templates.iter().map(Name::from).collect())
            .unwrap_or_default();

        // Real PHP (8.0+) makes any class declaring a public
        // `__toString()` automatically implement `Stringable`, with no
        // `implements Stringable` needed - added specifically so the
        // pre-existing body of `__toString()`-bearing classes would work
        // seamlessly with the `string|Stringable` idiom without
        // modification. Same shape as the enum UnitEnum/BackedEnum
        // implicit-interface handling below, for a different implicit
        // interface.
        let mut implements: Vec<Name> = c.implements.iter().map(class_ref).collect();
        if methods.contains_key("__toString") {
            implements.push(Name::from("Stringable"));
        }
        let trait_aliases: Vec<TraitAlias> = c
            .trait_aliases()
            .filter_map(|a| {
                Some(TraitAlias {
                    trait_name: a.trait_name.as_ref().map(class_ref),
                    method: Name::from(&a.method),
                    alias: Name::from(a.alias.as_deref()?),
                })
            })
            .collect();

        let is_api = class_doc.as_ref().is_some_and(|d| d.api);
        let is_api_do_not_implement = class_doc.as_ref().is_some_and(|d| d.api_do_not_implement);
        let is_internal = class_doc.as_ref().is_some_and(|d| d.internal);
        let is_consistent_constructor =
            class_doc.as_ref().is_some_and(|d| d.consistent_constructor);
        // `@phpstan-immutable` on the class's own docblock (not a
        // property's - `DocBlock::readonly`'s doc comment covers that
        // separate, pre-existing use of the same tag), mirroring real
        // PHPStan's `ClassReflection::isImmutable()`: treats
        // `@phpstan-immutable` OR `@phpstan-readonly` identically, both
        // making every property of the class (and its subclasses)
        // behave as if individually marked readonly. Deliberately not
        // baked with parent-inheritance here - `Registry::
        // class_is_immutable` (`registry/hierarchy.rs`) walks `extends`
        // at query time instead, avoiding a registry-build-ordering
        // hazard (a not-yet-indexed ancestor's own tag).
        let is_immutable = class_doc
            .as_ref()
            .is_some_and(|d| d.immutable || d.readonly);
        // `@phpstan-require-extends`/`-implements`/`@phpstan-sealed` -
        // each raw tag's type expression is parsed, name-resolved, then
        // flattened to the class names it names (`object_class_names`),
        // mirroring real PHPStan's `getObjectClassNames()` call sites
        // for these same three tags.
        let resolve_tag_types = |raw: &[TypeNode]| -> Vec<Name> {
            raw.iter()
                .flat_map(|text| {
                    let ty = resolve_names_in_type(parse_type_node(text), ctx);
                    ty.object_class_names()
                        .into_iter()
                        .map(Name::from)
                        .collect::<Vec<_>>()
                })
                .collect()
        };
        let require_extends = class_doc
            .as_ref()
            .map(|d| resolve_tag_types(&d.require_extends))
            .unwrap_or_default();
        let require_implements = class_doc
            .as_ref()
            .map(|d| resolve_tag_types(&d.require_implements))
            .unwrap_or_default();
        let access =
            crate::registry::access_attributes::access_attributes(&c.attrs, ctx.namespace());
        let method_access = c
            .methods
            .iter()
            .filter_map(|m| {
                let a = crate::registry::access_attributes::access_attributes(
                    &m.function.attrs,
                    ctx.namespace(),
                );
                (!a.is_empty()).then(|| (Name::from(&m.function.name.to_ascii_lowercase()), a))
            })
            .collect();
        let sealed = class_doc
            .as_ref()
            .map(|d| resolve_tag_types(&d.sealed))
            .unwrap_or_default();
        let mixins = class_doc
            .as_ref()
            .map(|d| resolve_tag_types(&d.mixins))
            .unwrap_or_default();
        // Own small dedicated closure, not `resolve_tag_types` above -
        // see `mixin_case_refs`'s doc comment.
        let mixin_case_refs: Vec<(Name, Name)> = class_doc
            .as_ref()
            .map(|d| {
                d.mixins
                    .iter()
                    .flat_map(|text| {
                        let ty = parse_type_node(text);
                        let mut refs = Vec::new();
                        collect_class_name_case_refs(&ty, ctx, &mut refs);
                        refs
                    })
                    .collect()
            })
            .unwrap_or_default();
        let deprecated = class_doc.as_ref().and_then(|d| d.deprecated.clone());
        let attribute_targets = compute_attribute_targets(&c.attrs);
        let doctrine_entity = compute_doctrine_entity(c, &properties);

        let mut info = ClassReflection {
            is_anonymous: false,
            provenance: Provenance::default(),
            kind: c.kind,
            is_abstract: c.is_abstract,
            is_final: c.is_final,
            is_final_by_phpdoc: !c.is_final && class_doc.as_ref().is_some_and(|d| d.is_final),
            is_readonly: c.is_readonly,
            extends: c.extends.iter().map(class_ref).collect(),
            implements,
            methods,
            redeclared_methods,
            properties,
            consts,
            uses: c.used_traits().map(class_ref).collect(),
            abstract_methods,
            static_methods,
            final_methods,
            final_properties,
            final_methods_by_phpdoc,
            final_properties_by_phpdoc,
            trait_aliases,
            resolved_trait_conflicts: c.resolved_trait_conflicts().map(Name::from).collect(),
            templates,
            extends_args,
            implements_args,
            uses_args,
            template_bounds,
            template_defaults,
            covariant_templates,
            contravariant_templates,
            is_api,
            is_api_do_not_implement,
            is_internal,
            is_consistent_constructor,
            is_immutable,
            require_extends,
            require_implements,
            sealed,
            access,
            method_access,
            mixins,
            mixin_case_refs,
            deprecated,
            attribute_targets,
            attributes: ReflectionAttribute::all(&c.attrs),
            enum_case_values: FxHashMap::default(),
            type_aliases,
            type_alias_case_refs,
            type_imports,
            method_tags,
            doctrine_entity,
        };
        if c.kind == ClassKind::Enum {
            info.add_enum_members(c, &class_scope);
        }
        info
    }

    /// Adds what the language gives every enum: its cases, as constants
    /// typed as the case; `cases()`, and a backed enum's `from()` and
    /// `tryFrom()`; the `name` property, and a backed enum's `value`; and
    /// `UnitEnum`/`BackedEnum` among its interfaces.
    // Declared types are joined while the class hierarchy is being built.
    #[allow(clippy::disallowed_methods)]
    fn add_enum_members(&mut self, e: &ClassDef, fqn: &str) {
        // `cases(): array` is a real, built-in static method every enum
        // gets from the language itself (`UnitEnum`), and a backed
        // enum's `from(int|string $value): static`/`tryFrom(...): ?static`
        // (`BackedEnum`) likewise - none declared in source. Without
        // these, undefined-static-method detection would flag every real
        // `Enum::cases()`/`::tryFrom()` call.
        let self_ty = Type::object(fqn.into(), Vec::new());
        // Real PHPStan's own `functionMap.php` declares
        // `UnitEnum::cases()` as `list<static>` outright - a bare,
        // unmarked `Array` here would leave every caller's result
        // (e.g. `array_merge(['none'], HistoryImageType::cases())`)
        // unable to prove list-ness itself.
        let cases_return_ty = Type::intersection_of(vec![
            Type::array(Type::unknown_array_key(), self_ty.clone()),
            Type::accessory(AccessoryKind::ArrayList),
        ]);
        self.methods.insert(
            Name::from("cases"),
            FunctionReflection {
                provenance: Provenance::default(),
                params: Vec::new(),
                return_type: cases_return_ty.clone(),
                return_type_native: cases_return_ty,
                // Fully authoritative built-in shapes, not from any real
                // docblock - `true` so `flatten_phpdoc_inheritance` never
                // treats a synthesized enum method as an inheritance
                // candidate.
                return_has_own_doc: true,
                return_doc_ty: None,
                self_out: None,
                // No real docblock/native text behind any of these
                // synthesized enum methods - nothing to check the case of.
                return_case_refs: Vec::new(),
                return_explicit_mixed_hint: false,
                no_discard: false,
                returns_ref: false,
                must_use_result: false,
                visibility: Visibility::Public,
                is_internal: false,
                is_api: false,
                has_override_attr: false,
                attributes: Vec::new(),
                deprecated: None,
                throws: None,
                assert_if_true: Vec::new(),
                assert_if_false: Vec::new(),
                assert: Vec::new(),
                purity_dependencies: None,
                is_transitively_pure: false,
                is_stub: false,
                // A real, unconditional PHP built-in, not a textual
                // declaration - but never reachable through `required_
                // abstract_methods`'s own interface-only scan either way
                // (an enum is never itself an interface), so this is
                // about `is_magic`'s own general meaning staying
                // accurate, not fixing a second live bug.
                is_magic: true,
            },
        );
        if let Some(backing) = &e.backing_type {
            let backing_ty = types::native_type(backing);
            let value_param = ParameterReflection {
                attributes: Vec::new(),
                name: Name::from("value"),
                ty: backing_ty.clone(),
                ty_native: backing_ty,
                variadic: false,
                has_default: false,
                default_ty: None,
                doc_ty: None,
                by_ref: false,
                has_own_doc: true,
                explicit_mixed_hint: false,
                case_refs: Vec::new(),
                closure_this_type: None,
                out_ty: None,
            };
            self.methods.insert(
                Name::from("from"),
                FunctionReflection {
                    provenance: Provenance::default(),
                    params: vec![value_param.clone()],
                    return_type: self_ty.clone(),
                    return_type_native: self_ty.clone(),
                    return_has_own_doc: true,
                    return_doc_ty: None,
                    self_out: None,
                    return_case_refs: Vec::new(),
                    return_explicit_mixed_hint: false,
                    no_discard: false,
                    returns_ref: false,
                    must_use_result: false,
                    visibility: Visibility::Public,
                    is_internal: false,
                    is_api: false,
                    has_override_attr: false,
                    attributes: Vec::new(),
                    deprecated: None,
                    // `from()` throws a real `ValueError` when no case
                    // matches - unlike `tryFrom()`, which returns `null`
                    // instead.
                    throws: Some(Type::object("ValueError".into(), Vec::new())),
                    assert_if_true: Vec::new(),
                    assert_if_false: Vec::new(),
                    assert: Vec::new(),
                    purity_dependencies: None,
                    is_transitively_pure: false,
                    is_stub: false,
                    is_magic: true,
                },
            );
            self.methods.insert(
                Name::from("tryFrom"),
                FunctionReflection {
                    provenance: Provenance::default(),
                    params: vec![value_param],
                    return_type: Type::union_of(vec![self_ty.clone(), Type::null()]),
                    return_type_native: Type::union_of(vec![self_ty.clone(), Type::null()]),
                    return_has_own_doc: true,
                    return_doc_ty: None,
                    self_out: None,
                    return_case_refs: Vec::new(),
                    return_explicit_mixed_hint: false,
                    no_discard: false,
                    returns_ref: false,
                    must_use_result: false,
                    visibility: Visibility::Public,
                    is_internal: false,
                    is_api: false,
                    has_override_attr: false,
                    attributes: Vec::new(),
                    deprecated: None,
                    throws: None,
                    assert_if_true: Vec::new(),
                    assert_if_false: Vec::new(),
                    assert: Vec::new(),
                    purity_dependencies: None,
                    is_transitively_pure: false,
                    is_stub: false,
                    is_magic: true,
                },
            );
        }
        // A case is accessed exactly like a constant (`Suit::Hearts`,
        // same `ClassName::member` syntax as `Suit::SOME_CONST`) and has
        // a real, useful static type: the enum itself. Folding it into
        // `consts` (rather than a separate field) means the checker's
        // static-access existence check and its type inference both just
        // work, with no new lookup path needed. A case has no visibility
        // syntax in real PHP - always public.
        for case in &e.enum_cases {
            self.consts.insert(
                Name::from(&case.name),
                ConstantReflection {
                    attributes: ReflectionAttribute::all(&case.attrs),
                    provenance: Provenance::default(),
                    ty: Type::enum_case(Name::from(fqn), Name::from(&*case.name)),
                    visibility: Visibility::Public,
                    is_final: true,
                    native_ty: None,
                    has_phpdoc_type: false,
                    value_ty: Type::enum_case(Name::from(fqn), Name::from(&*case.name)),
                    // A case has no doc-comment/`@internal`/`@deprecated`
                    // convention of its own in real PHP either - same
                    // reasoning as its always-`Visibility::Public` above.
                    is_internal: false,
                    deprecated: None,
                },
            );
        }
        // Cases and constants interleave in source; keep that order.
        let position = |name: &Name| {
            e.enum_cases
                .iter()
                .find(|c| c.name == **name)
                .map(|c| c.name.span)
                .or_else(|| {
                    e.consts
                        .iter()
                        .find(|k| k.name == **name)
                        .map(|k| k.name.span)
                })
                .map(|span| (span.start.line, span.start.col))
        };
        self.consts.sort_by_cached_key(|name, _| position(name));

        // Each case's own literal backing value, independent of whether
        // every case happens to be a bare literal (unlike `->value`'s
        // union type below, which requires all-or-nothing precision) -
        // `::from()`/`::tryFrom()` narrowing only needs to know a given
        // literal argument's matching case, if any, and a partial map
        // still answers that for the literal-valued cases it covers.
        self.enum_case_values = e
            .enum_cases
            .iter()
            .filter_map(|c| {
                let v = c.value.as_ref()?;
                let ty = match &v.kind {
                    ExprKind::Literal(Literal::String(s)) => Type::string_literal(s.clone()),
                    ExprKind::Literal(Literal::Int(n)) => Type::int_literal(*n),
                    _ => return None,
                };
                Some((Name::from(&c.name), ty))
            })
            .collect();

        // Every enum case is a real object with a built-in `->name`
        // (its case name, as a string) - and a backed enum's case
        // additionally has a real `->value` typed to its backing type -
        // neither declared in source, both provided by the language
        // itself (`UnitEnum`/`BackedEnum`). Without registering these as
        // properties, undefined-property-on-read detection would flag
        // every real `$case->value` access.
        self.properties.insert(
            Name::from("name"),
            PropertyReflection {
                attributes: Vec::new(),
                provenance: Provenance::default(),
                doc_ty: None,
                ty: Type::string(),
                // Every enum case's `->name` is language-guaranteed to
                // be a real, natively-`string`-typed property - matching
                // `has_default` below's "language-guaranteed, not a
                // guess" reasoning. Enums can't be extended, so no
                // override check ever actually reads this, but a
                // correct value costs nothing to provide.
                ty_native: Type::string(),
                has_native_type: true,
                is_static: false,
                is_readonly: false,
                allows_private_mutation: false,
                readonly_by_phpdoc: false,
                tag_read_only: false,
                tag_write_only: false,
                set_ty: None,
                is_virtual: false,
                visibility: Visibility::Public,
                write_visibility: Visibility::Public,
                implicit_protected_set: false,
                is_internal: false,
                deprecated: None,
                // Unlike a magic `@property` tag, this one's presence is
                // language-guaranteed (every enum case always has a real
                // `->name`) - `true`, not the usual synthetic-property
                // `false`, so `isset($case->name)` can still be correctly
                // recognized as redundant.
                has_default: true,
                is_abstract: false,
                has_get_hook: false,
                has_set_hook: false,
                // A language built-in, not textual - nothing to check
                // the case of.
                case_refs: Vec::new(),
                set_case_refs: Vec::new(),
            },
        );
        if let Some(backing) = &e.backing_type {
            // When every case's value is a bare literal (a case value
            // naming a class constant falls back to the plain backing
            // type below, "don't guess"), `->value` is typed as the
            // union of those literals, not the generic backing type,
            // mirroring real PHPStan's precision for a backed enum's
            // `->value` (`SortOrder: string { case Asc = 'ASC'; case
            // Desc = 'DESC'; }`'s `->value` needs to be `'ASC'|'DESC'`,
            // not bare `string`).
            let case_values: Vec<Type> = e
                .enum_cases
                .iter()
                .filter_map(|c| self.enum_case_values.get(c.name.as_ref()).cloned())
                .collect();
            let value_ty = if !case_values.is_empty() && case_values.len() == e.enum_cases.len() {
                match case_values.len() {
                    1 => case_values.into_iter().next().unwrap(),
                    _ => Type::union_of(case_values),
                }
            } else {
                types::native_type(backing)
            };
            self.properties.insert(
                Name::from("value"),
                PropertyReflection {
                    attributes: Vec::new(),
                    provenance: Provenance::default(),
                    doc_ty: None,
                    ty: value_ty,
                    // Same reasoning as `->name` above - language-
                    // guaranteed, but here `ty_native` is the plain
                    // backing type, not the possibly-narrower literal-
                    // union `ty` a fully-literal case set gets: real
                    // PHP's native reflection only ever sees the backing
                    // type (`string`/`int`), never a per-case refinement
                    // PHPStan derives from analysis alone.
                    ty_native: types::native_type(backing),
                    has_native_type: true,
                    is_static: false,
                    is_readonly: false,
                    allows_private_mutation: false,
                    readonly_by_phpdoc: false,
                    tag_read_only: false,
                    tag_write_only: false,
                    set_ty: None,
                    is_virtual: false,
                    visibility: Visibility::Public,
                    write_visibility: Visibility::Public,
                    implicit_protected_set: false,
                    is_internal: false,
                    deprecated: None,
                    // Same reasoning as `->name` above - a backed enum
                    // case's `->value` is language-guaranteed too.
                    has_default: true,
                    is_abstract: false,
                    has_get_hook: false,
                    has_set_hook: false,
                    case_refs: Vec::new(),
                    set_case_refs: Vec::new(),
                },
            );
        }

        // Every enum implements `UnitEnum` (and a backed one also
        // `BackedEnum`) by virtue of being an enum at all - never
        // declared in source, the same as its synthesized `cases()`/
        // `name`/`value` above, but this time needed for hierarchy
        // checks (`is_subclass_of`/`accepts`), not member lookup.
        // Without this, real code narrowing/typing an enum against
        // `UnitEnum` would be flagged as a bound violation despite being
        // exactly what real PHP guarantees.
        self.implements.push(Name::from("UnitEnum"));
        if e.backing_type.is_some() {
            self.implements.push(Name::from("BackedEnum"));
        }

        for builtin in ["cases", "from", "tryFrom"] {
            if self.methods.contains_key(builtin)
                && !e
                    .methods
                    .iter()
                    .any(|m| m.function.name.eq_ignore_ascii_case(builtin))
            {
                self.static_methods.insert(Name::from(builtin));
            }
        }
    }

    /// Merges a second unconditional declaration of the same class name
    /// into `self`, mirroring real PHPStan's `stubFiles` mechanism: a
    /// project can redeclare just one method of an already-known class as
    /// a targeted override, relying on the real declaration to still
    /// provide everything else. Whichever is `other` here (the second one
    /// indexed) wins on a same-name member conflict (`HashMap::extend`'s
    /// standard "later insert wins" behavior), matching a deliberate
    /// override's intent - but never erases a member, `extends`/
    /// `implements`, or generic information `self` already had that
    /// `other` simply doesn't mention (a bare `class Imagick { ... }`
    /// override redeclaring no `extends`/`implements` of its own must
    /// not blank out the real class's).
    pub(crate) fn merge(mut self, other: ClassReflection) -> ClassReflection {
        // Not a bare `HashMap::extend` (a plain overwrite, `self`'s
        // colliding entry discarded outright): a vendored stub's method
        // could otherwise silently overwrite an internal, correctly
        // `FunctionReflection::is_stub`-marked one, losing that marker and
        // wrongly letting `registry::purity` classify a raw-memory-write
        // native function as pure. Routing a colliding method through
        // `FunctionReflection::merge` instead (the same function used for a
        // top-level function's identical "declared twice" shape) keeps
        // its permissive "either side's marker sticks" `is_stub`
        // computation in the one place that owns it.
        for (name, other_sig) in other.methods {
            match self.methods.remove(&name) {
                Some(self_sig) => {
                    self.methods.insert(name, self_sig.merge(other_sig));
                }
                None => {
                    self.methods.insert(name, other_sig);
                }
            }
        }
        self.redeclared_methods.extend(other.redeclared_methods);
        self.properties.extend(other.properties);
        self.consts.extend(other.consts);
        self.enum_case_values.extend(other.enum_case_values);
        self.type_aliases.extend(other.type_aliases);
        self.type_alias_case_refs.extend(other.type_alias_case_refs);
        self.type_imports.extend(other.type_imports);
        self.abstract_methods.extend(other.abstract_methods);
        self.final_methods.extend(other.final_methods);
        self.static_methods.extend(other.static_methods);
        self.final_properties.extend(other.final_properties);
        self.final_methods_by_phpdoc
            .extend(other.final_methods_by_phpdoc);
        self.final_properties_by_phpdoc
            .extend(other.final_properties_by_phpdoc);
        self.resolved_trait_conflicts
            .extend(other.resolved_trait_conflicts);
        self.covariant_templates.extend(other.covariant_templates);
        self.contravariant_templates
            .extend(other.contravariant_templates);
        self.template_bounds.extend(other.template_bounds);
        self.template_defaults.extend(other.template_defaults);
        self.implements_args.extend(other.implements_args);
        self.uses_args.extend(other.uses_args);
        // Same "a tag present on either declaration sticks" reasoning as
        // the `HashSet::extend` calls above - a stub override that
        // doesn't repeat `@api` on its docblock must not un-mark a class
        // the real declaration already tagged.
        self.is_api = self.is_api || other.is_api;
        self.is_api_do_not_implement =
            self.is_api_do_not_implement || other.is_api_do_not_implement;
        self.is_internal = self.is_internal || other.is_internal;
        self.is_consistent_constructor =
            self.is_consistent_constructor || other.is_consistent_constructor;
        self.require_extends.extend(other.require_extends);
        self.require_implements.extend(other.require_implements);
        self.sealed.extend(other.sealed);
        self.mixins.extend(other.mixins);
        self.mixin_case_refs.extend(other.mixin_case_refs);
        self.method_tags.extend(other.method_tags);
        self.deprecated = self.deprecated.or(other.deprecated);
        self.attribute_targets = self.attribute_targets.or(other.attribute_targets);
        if self.extends.is_empty() {
            self.extends = other.extends;
        }
        if self.implements.is_empty() {
            self.implements = other.implements;
        }
        if self.uses.is_empty() {
            self.uses = other.uses;
        }
        if self.templates.is_empty() {
            self.templates = other.templates;
        }
        if self.extends_args.is_empty() {
            self.extends_args = other.extends_args;
        }
        if self.trait_aliases.is_empty() {
            self.trait_aliases = other.trait_aliases;
        }
        self
    }
}

/// A class constant's PHP 8.3 native type.
fn native_const_type(cc: &syntax::ast::ClassConst) -> Option<Type> {
    cc.type_hint.as_ref().map(types::native_type)
}

/// An ancestor tag's class and generic arguments.
fn class_and_args(ty: &Type) -> Option<(Name, Vec<Type>)> {
    ty.as_object()
        .map(|(name, args)| (Name::from(name), args.to_vec()))
}
