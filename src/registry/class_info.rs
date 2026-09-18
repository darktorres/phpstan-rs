use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::ast::{ClassDecl, ClassKind, EnumDecl, ExprKind, Literal, TraitAlias, Visibility};
use crate::docblock::{self, DocBlock};
use crate::intern::intern;
use crate::registry::function_sig::{
    FunctionSig, ParamSig, apply_implicit_nullable_default, build_template_params,
    build_type_aliases, infer_const_type, mark_template_params, resolve_type_with_parsed_native,
    substitute_type_aliases,
};
use crate::registry::name_context::{
    NameContext, collect_class_name_case_refs, resolve_names_in_type,
};
use crate::types::{self, AccessoryKind, TemplateParam, Type};

mod attributes;
mod doctrine_entity;
mod tag_synthesis;
mod virtual_property;

use attributes::compute_attribute_targets;
pub(crate) use doctrine_entity::DoctrineEntity;
use doctrine_entity::compute_doctrine_entity;
use tag_synthesis::method_tag_sig;
pub(crate) use tag_synthesis::object_class_names;
pub(crate) use virtual_property::hook_body_references_property;
use virtual_property::{hooks_reference_backing_storage, property_hook_set_type};

#[derive(Debug, Clone)]
pub struct PropertySig {
    pub ty: Type,
    pub is_static: bool,
    pub is_readonly: bool,
    // `public`/`private`/`protected`. A magic `@property` tag and an
    // enum's built-in `name`/`value` are always `Visibility::Public` -
    // real PHP has no way to declare either any other way.
    pub visibility: Visibility,
    // PHP 8.4 asymmetric visibility (`public private(set) string $x;`) -
    // the write-side visibility, defaulting to `visibility` itself (the
    // read side) for an ordinary, symmetric property, or one with no
    // established per-item place to write a separate write-visibility (a
    // promoted constructor property, a magic `@property` tag, an enum's
    // built-in `name`/`value`).
    pub write_visibility: Visibility,
    // PHP 8.4 property hooks: an explicit `set(Type $value) { ... }` hook
    // can accept a wider type than the property's own backing/read type
    // (`public array $x { set(array $value) { $this->x =
    // sanitize($value); } }` - the hook does its own coercion inside the
    // body before the real assignment). `None` when there's no hook, or
    // the hook uses an implicit `set { ... }` with no parameter list at
    // all - a write then still checks against `ty` as before.
    pub set_ty: Option<Type>,
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
    pub is_virtual: bool,
    // `@internal` on this property's own docblock - see
    // `docblock::DocBlock::internal`'s doc comment. Only ever set for a
    // real, plainly-declared property with its own doc comment; a
    // promoted constructor property, a magic `@property` tag, and an
    // enum's built-in `name`/`value` all stay `false` - none of them has
    // an established per-item place to write the tag.
    pub is_internal: bool,
    // `@deprecated` on this property's own doc comment - direct mirror of
    // `FunctionSig::deprecated`'s "own tag only, no class-level fallback"
    // scoping. Same restriction as `is_internal` above.
    pub deprecated: Option<String>,
    // Whether this property's own declaration (an ordinary
    // `PropertyDecl::default`, or a promoted constructor parameter's own
    // `Param::default`) gives it a default value - `checker/
    // uninitialized.rs`'s "genuinely might still be uninitialized"
    // candidate gate needs the class's AST to compute this, which isn't
    // available for an arbitrary `$obj->prop` access (only for
    // `$this->prop` inside the class currently being checked), so this
    // field makes the same fact available from `PropertySig` alone, for
    // `check_pointless_offset_or_property_probe`'s "maybe genuinely
    // uninitialized, don't flag isset() as redundant" exemption. `false`
    // (the safe, permissive default) for every synthetic property with
    // no real declaration to read a default from.
    pub has_default: bool,
    // PHP 8.4 `abstract` property modifier (only legal on a hooked
    // property inside an abstract class, but tracked here unconditionally
    // like `is_readonly` above - the parser/engine enforce the legality
    // half). `false` for every synthetic property.
    pub is_abstract: bool,
    // Whether a `get`/`set` hook is declared at all (real PHP's
    // `hasHook()`), independent of whether it has a real body -
    // `PropertyHookBody::Abstract` is what makes this question
    // answerable. `false` for every synthetic property.
    pub has_get_hook: bool,
    pub has_set_hook: bool,
    // The native, no-docblock-merge type this property alone declares -
    // the exact `ty`-vs-`ty_native` split `ParamSig` has, needed here for
    // `OverridingPropertyRule`'s `property.nativeType`/
    // `.missingNativeType`/`.extraNativeType` family, which real PHPStan
    // checks purely at the native level. `resolve_type(None, native_hint)`
    // collapses "no native hint" and an explicit `mixed` hint into the
    // same `Type::Mixed` value, which is why `has_native_type` below must
    // be a separate boolean rather than testing this field against
    // `Type::Mixed`.
    pub ty_native: Type,
    pub has_native_type: bool,
    // `ParamSig::case_refs`'s property-position equivalent. Populated
    // identically at all three real construction sites (plain declared
    // property, promoted constructor property, `@property`/`-read`/
    // `-write` tag).
    pub case_refs: Vec<(Arc<str>, Arc<str>)>,
    // `set_ty`'s case_refs equivalent - `property_hook_set_type` never
    // collected these, unlike the sibling `ty` field above.
    pub set_case_refs: Vec<(Arc<str>, Arc<str>)>,
}

// A class constant's `public`/`private`/`protected` modifier (PHP 7.1+).
// Mirrors `PropertySig`'s shape, minus the property-only concerns
// (`is_static`/`is_readonly`/`set_ty`) a constant has no equivalent of.
#[derive(Debug, Clone)]
pub struct ConstSig {
    pub ty: Type,
    pub visibility: Visibility,
    // `@internal` on this constant's own doc comment - direct mirror of
    // `PropertySig::is_internal`. Always `false` for an enum case (no
    // doc-comment/`@internal` convention of its own).
    pub is_internal: bool,
    // `@deprecated` on this constant's own doc comment - direct mirror of
    // `PropertySig::deprecated`. Always `None` for an enum case.
    pub deprecated: Option<String>,
}

#[derive(Debug, Clone)]
#[allow(clippy::type_complexity)]
pub struct ClassInfo {
    pub kind: ClassKind,
    pub is_abstract: bool,
    pub is_final: bool,
    // `readonly class` (PHP 8.2). Always `false` for an enum (real PHP
    // rejects `readonly enum` at parse time) or a trait composed in (a
    // trait has no `readonly`-ness of its own to inherit; `readonly
    // class Foo { use T; }` only ever makes `Foo`'s properties readonly,
    // never retroactively marks `T`).
    pub is_readonly: bool,
    // `@phpstan-immutable`/`@phpstan-readonly` on this class's own
    // docblock. Never baked with parent-inheritance
    // (`Registry::class_is_immutable` walks `extends` at query time
    // instead) - this field alone only answers "did this exact
    // declaration carry the tag," not "is this class immutable
    // considering its whole ancestor chain."
    pub is_immutable: bool,
    pub extends: Vec<Arc<str>>,
    pub implements: Vec<Arc<str>>,
    pub methods: HashMap<Arc<str>, FunctionSig>,
    pub properties: HashMap<Arc<str>, PropertySig>,
    pub consts: HashMap<Arc<str>, ConstSig>,
    pub uses: Vec<Arc<str>>,
    // Method names declared `abstract` in this class's own body (not
    // walked up the hierarchy - a completeness check needs to walk
    // `extends`/`implements` itself and consult each ancestor's own set).
    pub abstract_methods: HashSet<Arc<str>>,
    // Method names declared `final` in this class's own body.
    pub final_methods: HashSet<Arc<str>>,
    // Method names declared `static` in this class's own body - same
    // "not walked up the hierarchy" shape as `abstract_methods`/
    // `final_methods` above, used by `check_one_method_override`'s
    // `method.nonStatic`/`method.static` check, mirroring real PHPStan's
    // `OverridingMethodRule`: an override must match its ancestor's
    // static-ness exactly, either direction.
    pub static_methods: HashSet<Arc<str>>,
    // Property names declared `final` in this class's own body (PHP 8.4) -
    // same "not walked up the hierarchy" shape as `final_methods` above;
    // `Registry::find_final_ancestor_property` does that walk. Real PHP
    // fatals overriding a `final` property in a subclass exactly like a
    // `final` method.
    pub final_properties: HashSet<Arc<str>>,
    // `@final`'s docblock-only siblings to `final_methods`/
    // `final_properties` above - real PHP never enforces this convention
    // (no engine-level guarantee backs it, unlike the native keyword), so
    // it needs its own explicit ancestor-walk consultation
    // (`Registry::find_final_ancestor_method`/
    // `find_final_ancestor_property`, real keyword taking priority when
    // both somehow apply on the same ancestor).
    pub final_methods_by_phpdoc: HashSet<Arc<str>>,
    pub final_properties_by_phpdoc: HashSet<Arc<str>>,
    // `use A, B { B::hello as helloB; }` - each entry's `trait_name` (when
    // present) is already resolved to the same fully-qualified name
    // `uses` itself is keyed under, so `flatten_traits` can look it up in
    // `self.classes` directly.
    pub trait_aliases: Vec<TraitAlias>,
    // Method/property names explicitly mentioned in any trait-adaptation
    // clause (`insteadof` or `as`) - the trait-conflict check treats
    // these as already addressed by the class author, even for
    // `insteadof` (whose actual trait-selection this project doesn't
    // otherwise model).
    pub resolved_trait_conflicts: HashSet<Arc<str>>,
    // This class's own `@template T` names, in declaration order - order
    // matters, since a generic reference (`Collection<User>`) binds its
    // type arguments to these positionally (`checker::
    // class_template_bindings`). Empty for a non-generic class.
    pub templates: Vec<Arc<str>>,
    // `@extends Parent<Arg, ...>`'s argument list - fixes
    // `extends.first()`'s `@template` names for every instance of this
    // class, independent of whether this class is itself generic (e.g.
    // `class IntCollection extends Collection<int> {}`, which isn't
    // generic itself but permanently binds `Collection`'s `T` to `int`).
    // An argument can itself reference this class's own `@template` name
    // (`class Box<T> extends Collection<T> {}`) - such an argument is
    // `Type::TemplateParam(name)`, substituted using whatever this
    // class's templates are bound to at each use, not a fixed value.
    // Empty when there's no `@extends` tag, or no native `extends` for
    // it to apply to.
    pub extends_args: Vec<Type>,
    // Same idea as `extends_args`, but keyed by which interface each
    // `@implements Iface<Arg, ...>` tag applies to (a class can implement
    // more than one at once) - the key is resolved to the same
    // fully-qualified name `implements` itself uses, so the two agree.
    pub implements_args: HashMap<Arc<str>, Vec<Type>>,
    // Same idea again, but for a generic trait named in an `@use
    // Trait<Arg, ...>` tag - keyed the same way as `implements_args`.
    // Unlike `extends`/`implements`, which are both written on the
    // class's own top-level docblock, real PHPStan resolves `@use` from
    // the `use` statement's own doc comment (`/** @use
    // TemplateTypeTrait<ObjectType> */` sits right above `use
    // TemplateTypeTrait;`, not the class's separate `/** @api */`
    // docblock above it) - see `ClassDecl::uses_doc_comments`'s doc
    // comment for where this project captures that.
    pub uses_args: HashMap<Arc<str>, Vec<Type>>,
    // `@template T of Bound` - the resolved bound for whichever of
    // `templates` above declared one. Checked by
    // `checker::check_generic_bounds` wherever this class is referenced
    // with concrete type arguments (`Collection<Foo>`).
    pub template_bounds: HashMap<Arc<str>, Type>,
    // `@template T = Default` - the resolved default for whichever of
    // `templates` above declared one, mirroring `template_bounds`'s
    // shape/construction exactly. A generic reference supplying fewer
    // type arguments than `templates.len()` is only a real arity error
    // (`check_generic_reference_arity`, `checker/generics.rs`) for a
    // template name absent from this map - one with a default is
    // legitimately optional to supply.
    pub template_defaults: HashMap<Arc<str>, Type>,
    // Names from `templates` above additionally declared
    // `@template-covariant`/`@template-contravariant` - a name absent
    // from both stays invariant.
    pub covariant_templates: HashSet<Arc<str>>,
    pub contravariant_templates: HashSet<Arc<str>>,
    // This class/interface/trait's own docblock carries `@api` (or,
    // equivalently, `@api-do-not-implement`, which implies it - see
    // `DocBlock::api`'s doc comment). Consulted only by
    // `checker::phpstan_api`, and only ever meaningful for a class this
    // project didn't declare itself (one loaded from a real
    // `phpstan/phpstan` `vendor/` tree as context) - `false` for every
    // ordinary class.
    pub is_api: bool,
    // `@api-do-not-implement` specifically - an `@api` interface/class a
    // third party must not `extends`/`implements`, even though calling
    // its already-declared members is still fully covered by `is_api`
    // above.
    pub is_api_do_not_implement: bool,
    // `@internal` on this class/interface/trait/enum's own docblock -
    // see `docblock::DocBlock::internal`'s doc comment.
    pub is_internal: bool,
    // `@phpstan-consistent-constructor` on this class's own docblock -
    // see `docblock::DocBlock::consistent_constructor`'s doc comment.
    pub is_consistent_constructor: bool,
    // `@deprecated` on this class's own docblock - consulted only at
    // `new Foo()` (`checker::check_deprecated_instantiation`).
    // Deliberately doesn't imply anything about this class's own members
    // (see `FunctionSig::deprecated`'s doc comment for why that question
    // is left unclaimed) - this is purely "is the class itself tagged."
    pub deprecated: Option<String>,
    // This class's own `#[\Attribute(flags)]` meta-attribute flags (an
    // `int-mask-of<Attribute::TARGET_*>`), if this class is declared
    // usable as an attribute class at all - `None` means it isn't (real
    // PHP's `ReflectionClass::isAttribute()` would be `false`), distinct
    // from what a bare `#[\Attribute]` with no arguments means
    // (`Attribute::TARGET_ALL`, matching `Attribute::__construct`'s own
    // default parameter value - see `compute_attribute_targets`'s doc
    // comment).
    pub attribute_targets: Option<u32>,
    // For a backed enum only, each case's literal backing value
    // (`StringLiteral`/`IntLiteral`), keyed by case name - lets
    // `::from()`/`::tryFrom()` narrow a literal-argument call to the
    // exact matching case's `Type::EnumCase` instead of the coarse
    // `self`/`?self`. Only populated when every case's value is a bare
    // literal (mirrors the "don't guess" restriction the existing
    // `->value` union-of-literals property type applies) - empty for a
    // non-enum, an unbacked enum, or a backed enum with any non-literal
    // case value (a case naming a class constant).
    pub enum_case_values: HashMap<Arc<str>, Type>,
    // This class's own `@phpstan-type Name TypeExpr` local alias table
    // (`function_sig::build_type_aliases`), keyed by each alias's own
    // resolved FQN - already spliced into every member's `ty` at
    // `from_decl` time (see `substitute_type_aliases`'s doc comment), but
    // kept here too for `Checker::inline_var_annotation`'s separate,
    // check-time `@var` resolution path, which needs the same table to
    // expand a local variable annotated `/** @var Name $x */` inside one
    // of this class's method bodies. Empty for a class with no such tags.
    pub type_aliases: HashMap<Arc<str>, Type>,
    // `ParamSig::case_refs`'s `@phpstan-type` equivalent - keyed the
    // same way `type_aliases` itself is (the alias's resolved local
    // name), `(raw, resolved)` pairs for every class-name reference the
    // alias's own RHS text carries (not a reference to the alias
    // elsewhere - that's this same mechanism at whatever
    // `@param`/`@return`/property tag actually uses the alias, via its
    // own `case_refs`).
    pub type_alias_case_refs: HashMap<Arc<str>, Vec<(Arc<str>, Arc<str>)>>,
    // This class's own `@phpstan-import-type` tags, name-resolved
    // (`from` to the source class's FQN, the imported name to the local
    // key this class's members already reference it by) but not yet
    // looked up in the source class's `type_aliases` - the source class
    // may not be indexed yet at `from_decl` time. Resolved by
    // `Registry::resolve_deferred_type_imports`, a later pass over the
    // fully-indexed registry - merged into `type_aliases` above once
    // resolved, so this field is empty again after that pass runs; never
    // read afterward.
    pub type_imports: Vec<ResolvedTypeImport>,
    // `@method` tags, already resolved into real `FunctionSig`s but not
    // yet inserted into `methods` - insertion has to wait for
    // `Registry::resolve_method_tags`, a later pass. Empty again after
    // that pass runs; never read afterward.
    pub method_tags: Vec<(Arc<str>, FunctionSig)>,
    // `#[ORM\Entity]` metadata (PHASE_BACKLOG Phase 168 168a) - `None`
    // for the overwhelming majority of classes, which aren't Doctrine
    // entities at all.
    pub doctrine_entity: Option<DoctrineEntity>,
    // `@phpstan-require-extends`/`@phpstan-require-implements`/
    // `@phpstan-sealed` - resolved directly to fully-qualified class
    // names (a tag's value can legally be a union of several, already
    // flattened here by `object_class_names`; a non-`Object` member,
    // e.g. an unresolvable name, is silently dropped, matching this
    // project's "don't guess" precedent elsewhere). See
    // `checker::oop::check_require_tags`/`check_sealed_hierarchy` for
    // the two, differently-shaped consuming checks.
    pub require_extends: Vec<Arc<str>>,
    pub require_implements: Vec<Arc<str>>,
    pub sealed: Vec<Arc<str>>,
    // `@mixin Type` - resolved FQNs, same shape/flattening as the three
    // above. Not yet looked up in the target's own `methods`/`properties`
    // (may not be indexed yet) - `Registry::resolve_deferred_mixins`,
    // mirroring `resolve_deferred_type_imports`'s own two-step shape.
    pub mixins: Vec<Arc<str>>,
    // `ParamSig::case_refs`'s `@mixin`-tag equivalent - `(raw, resolved)`
    // pairs for every `@mixin` target on this class's docblock, in
    // declaration order (not keyed by name - unlike
    // `type_alias_case_refs`, a class can carry several `@mixin` tags
    // with no unique name to key by).
    pub mixin_case_refs: Vec<(Arc<str>, Arc<str>)>,
}

/// See `ClassInfo::type_imports`'s own doc comment.
#[derive(Debug, Clone)]
pub struct ResolvedTypeImport {
    // The key this class's own already-built member types reference the
    // import under (`substitute_type_aliases` matches on this exact key,
    // same as an ordinary local `@phpstan-type` alias would).
    pub local_key: Arc<str>,
    pub source_class_fqn: Arc<str>,
    // The name as declared on the *source* class's own `@phpstan-type` -
    // not yet resolved to that class's own key (needs its own namespace,
    // reconstructed from `source_class_fqn` at resolution time rather
    // than carried here, since a class's own containing namespace is
    // always recoverable from its own FQN).
    pub source_alias_name: Arc<str>,
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
fn collect_merged_methods(
    methods: impl Iterator<Item = (Arc<str>, FunctionSig)>,
) -> HashMap<Arc<str>, FunctionSig> {
    let mut out: HashMap<Arc<str>, FunctionSig> = HashMap::new();
    for (name, sig) in methods {
        match out.remove(&name) {
            Some(existing) => {
                out.insert(name, existing.merge(sig));
            }
            None => {
                out.insert(name, sig);
            }
        }
    }
    out
}

impl ClassInfo {
    #[hotpath::measure]
    pub(crate) fn from_decl(c: &ClassDecl, ctx: &NameContext) -> ClassInfo {
        let class_doc: Option<DocBlock> = c.doc_comment.as_deref().map(docblock::parse);
        let templates: Vec<Arc<str>> = class_doc
            .as_ref()
            .map(|d| d.templates.iter().map(|s| intern(s)).collect())
            .unwrap_or_default();
        // This class's `@template` metadata (bound/variance/default),
        // keyed by name - `templates` above stays a plain `Vec<String>`
        // (positional order still drives type-argument binding
        // elsewhere, e.g. `checker::class_template_bindings_rec`), but
        // every `Type::TemplateParam` this class's docblock produces
        // (properties, consts, `@extends`/`@implements` args, and every
        // method's params/return type below) carries the real metadata
        // from here instead of a bare name.
        let class_scope = ctx.declared_name(&c.name);
        let template_params: HashMap<String, TemplateParam> = class_doc
            .as_ref()
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
                            local_key: intern(&ctx.resolve(local_name)),
                            source_class_fqn: intern(&ctx.resolve(&imp.from)),
                            source_alias_name: intern(&imp.alias),
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
        let mut methods_vec: Vec<(Arc<str>, FunctionSig)> = Vec::with_capacity(c.methods.len());
        let mut abstract_methods: HashSet<Arc<str>> = HashSet::new();
        let mut final_methods: HashSet<Arc<str>> = HashSet::new();
        let mut static_methods: HashSet<Arc<str>> = HashSet::new();
        let mut final_methods_by_phpdoc: HashSet<Arc<str>> = HashSet::new();
        for m in &c.methods {
            let name = m.function.name.clone();
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
            let sig = FunctionSig::from_decl(
                &m.function,
                ctx,
                &template_params,
                &type_aliases,
                m.visibility.clone(),
            );
            methods_vec.push((name.clone(), sig));
        }
        let methods: HashMap<Arc<str>, FunctionSig> =
            collect_merged_methods(methods_vec.into_iter());
        // Same private-is-not-overridable exemption `final_methods`
        // above applies, one member kind over - a private property
        // (final or not) is never part of a subclass's overridable
        // surface either.
        let mut final_properties: HashSet<Arc<str>> = c
            .properties
            .iter()
            .filter(|p| p.is_final && p.visibility != Visibility::Private)
            .map(|p| p.name.clone())
            .collect();
        // `public final int $x` is real, valid PHP on a promoted
        // constructor property too, and real PHP does enforce "Cannot
        // override final property" for it - unlike `abstract`, which
        // really is impossible there (an abstract constructor can't
        // declare any promoted property, a real fatal error).
        if let Some(ctor) = c
            .methods
            .iter()
            .find(|m| m.function.name == "__construct".into())
        {
            final_properties.extend(
                ctor.function
                    .params
                    .iter()
                    .filter(|p| {
                        p.promoted_final
                            && p.promoted_visibility.as_ref() != Some(&Visibility::Private)
                    })
                    .map(|p| p.name.clone()),
            );
        }

        // `@final`'s docblock-only sibling to `final_properties` above -
        // same private-is-not-overridable exemption.
        let mut final_properties_by_phpdoc: HashSet<Arc<str>> = c
            .properties
            .iter()
            .filter(|p| {
                p.visibility != Visibility::Private
                    && p.doc_comment
                        .as_deref()
                        .map(docblock::parse)
                        .is_some_and(|d| d.is_final)
            })
            .map(|p| p.name.clone())
            .collect();
        if let Some(ctor) = c
            .methods
            .iter()
            .find(|m| m.function.name == "__construct".into())
        {
            final_properties_by_phpdoc.extend(
                ctor.function
                    .params
                    .iter()
                    .filter(|p| {
                        p.promoted_visibility.as_ref() != Some(&Visibility::Private)
                            && p.doc_comment
                                .as_deref()
                                .map(docblock::parse)
                                .is_some_and(|d| d.is_final)
                    })
                    .map(|p| p.name.clone()),
            );
        }

        let mut properties: HashMap<Arc<str>, PropertySig> = c
            .properties
            .iter()
            .map(|p| {
                let doc_ty = p.doc_comment.as_deref().map(docblock::parse);
                let native_hint = p.type_hint.as_deref();
                let native_parsed = native_hint.map(types::parse_type_string);
                let doc_reconciled = resolve_type_with_parsed_native(
                    doc_ty.as_ref().and_then(|d| d.var_type.as_deref()),
                    native_hint,
                    native_parsed.as_ref(),
                );
                // See `FunctionSig::from_decl`'s param loop for why this
                // sharing is exact, not an approximation.
                let shared = Some(&doc_reconciled) == native_parsed.as_ref();
                let ty = mark_template_params(doc_reconciled, &template_params);
                let mut case_refs = Vec::new();
                collect_class_name_case_refs(&ty, ctx, &mut case_refs);
                let ty = resolve_names_in_type(ty, ctx);
                let ty_native = if shared {
                    ty.clone()
                } else {
                    let native_only = native_parsed.clone().unwrap_or(Type::Mixed);
                    let t = mark_template_params(native_only, &template_params);
                    resolve_names_in_type(t, ctx)
                };
                let ty = substitute_type_aliases(ty, &type_aliases);
                let (set_ty, set_case_refs) =
                    property_hook_set_type(p.hooks.as_ref(), &template_params, ctx);
                let is_virtual = p
                    .hooks
                    .as_ref()
                    .is_some_and(|h| !hooks_reference_backing_storage(h, &p.name));
                (
                    p.name.clone(),
                    PropertySig {
                        ty,
                        ty_native,
                        has_native_type: p.type_hint.is_some(),
                        is_static: p.is_static,
                        // `@readonly` (`DocBlock::readonly`'s doc
                        // comment) folds into the exact same flag native
                        // `readonly` sets - both flow through identical
                        // downstream enforcement.
                        is_readonly: p.is_readonly || doc_ty.as_ref().is_some_and(|d| d.readonly),
                        set_ty,
                        is_virtual,
                        visibility: p.visibility.clone(),
                        write_visibility: p
                            .write_visibility
                            .clone()
                            .unwrap_or(p.visibility.clone()),
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
        if let Some(ctor) = c
            .methods
            .iter()
            .find(|m| m.function.name == "__construct".into())
        {
            let ctor_doc = ctor.function.doc_comment.as_deref().map(docblock::parse);
            for p in &ctor.function.params {
                if p.promoted_visibility.is_none() {
                    continue;
                }
                // Same inline-`@var`-on-a-promoted-property fallback as
                // `FunctionSig::from_decl`'s param loop - needed here too,
                // separately: this builds the property's `PropertySig`
                // (read by `$this->details`-style property access
                // elsewhere in the class), a completely different
                // construction from the `ParamSig` `FunctionSig::from_decl`
                // builds for the same parameter.
                let doc_ty: Option<String> = ctor_doc
                    .as_ref()
                    .and_then(|d| d.params.get(p.name.as_ref()))
                    .cloned()
                    .or_else(|| {
                        p.doc_comment
                            .as_deref()
                            .map(docblock::parse)
                            .and_then(|d| d.var_type)
                    });
                let native_hint = p.type_hint.as_deref();
                let native_parsed = native_hint.map(types::parse_type_string);
                let doc_reconciled = resolve_type_with_parsed_native(
                    doc_ty.as_deref(),
                    native_hint,
                    native_parsed.as_ref(),
                );
                // See `FunctionSig::from_decl`'s param loop for why this
                // sharing is exact, not an approximation.
                let shared = Some(&doc_reconciled) == native_parsed.as_ref();
                let ty = mark_template_params(doc_reconciled, &template_params);
                let mut case_refs = Vec::new();
                collect_class_name_case_refs(&ty, ctx, &mut case_refs);
                let ty = resolve_names_in_type(ty, ctx);
                let ty_native = if shared {
                    ty.clone()
                } else {
                    let native_only = native_parsed.clone().unwrap_or(Type::Mixed);
                    let t = mark_template_params(native_only, &template_params);
                    resolve_names_in_type(t, ctx)
                };
                let ty = substitute_type_aliases(ty, &type_aliases);
                let ty = apply_implicit_nullable_default(ty, p.default.as_ref());
                let ty_native = apply_implicit_nullable_default(ty_native, p.default.as_ref());
                // A promoted param's own inline `@readonly` - cheap to
                // support given `p.doc_comment` is already parsed above
                // for `@var`'s fallback.
                let doc_readonly = p
                    .doc_comment
                    .as_deref()
                    .map(docblock::parse)
                    .is_some_and(|d| d.readonly);
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
                    p.name.clone(),
                    PropertySig {
                        ty,
                        ty_native,
                        has_native_type: p.type_hint.is_some(),
                        is_static: false,
                        is_readonly: p.promoted_readonly || doc_readonly,
                        set_ty,
                        is_virtual,
                        visibility: p.promoted_visibility.clone().unwrap_or(Visibility::Public),
                        write_visibility: p
                            .promoted_visibility
                            .clone()
                            .unwrap_or(Visibility::Public),
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
        // add virtual members, not override real ones. `@property-write`'s
        // write-only direction isn't separately enforced (no existing
        // "write-only" checking mechanism to hook into) - treated the
        // same as a plain `@property` (readable and writable).
        if let Some(doc) = &class_doc {
            for tag in &doc.properties {
                properties.entry(intern(&tag.name)).or_insert_with(|| {
                    let ty = types::parse_type_string(&tag.ty);
                    let ty = mark_template_params(ty, &template_params);
                    let mut case_refs = Vec::new();
                    collect_class_name_case_refs(&ty, ctx, &mut case_refs);
                    let ty = resolve_names_in_type(ty, ctx);
                    PropertySig {
                        ty,
                        // A magic `@property` tag has no native
                        // declaration to check against - `false`,
                        // matching every other synthetic-property field
                        // here.
                        ty_native: Type::Mixed,
                        has_native_type: false,
                        is_static: false,
                        is_readonly: tag.readable && !tag.writable,
                        set_ty: None,
                        is_virtual: false,
                        visibility: Visibility::Public,
                        write_visibility: Visibility::Public,
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
        let method_tags: Vec<(Arc<str>, FunctionSig)> = class_doc
            .as_ref()
            .map(|d| {
                d.method_tags
                    .iter()
                    .map(|tag| {
                        (
                            intern(&tag.name),
                            method_tag_sig(tag, &template_params, ctx),
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
                let ty = match doc_ty {
                    Some(doc_ty) => {
                        let ty = types::parse_type_string(&doc_ty);
                        let ty = mark_template_params(ty, &template_params);
                        let ty = resolve_names_in_type(ty, ctx);
                        substitute_type_aliases(ty, &type_aliases)
                    }
                    None => infer_const_type(&cc.value),
                };
                (
                    cc.name.clone(),
                    ConstSig {
                        ty,
                        visibility: cc.visibility.clone(),
                        // `@internal` on this constant's own doc
                        // comment - see `PropertySig::is_internal`'s doc
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
        // `@implements` tag, would stay distinct opaque `Type::Object`
        // nodes instead of each expanding to its real underlying shape,
        // making `check_cross_path_interface_conflicts`'s exact-equality
        // comparison see two "different" types where real PHPStan (which
        // always compares the expanded shape) sees an equivalent one.
        let resolve_generic_tag = |raw: &str| -> Type {
            let ty = types::parse_type_string(raw);
            let ty = mark_template_params(ty, &template_params);
            let ty = resolve_names_in_type(ty, ctx);
            substitute_type_aliases(ty, &type_aliases)
        };
        let extends_args: Vec<Type> = class_doc
            .as_ref()
            .and_then(|d| d.extends.as_deref())
            .map(|raw| match resolve_generic_tag(raw) {
                Type::Object(_, args) => args,
                _ => Vec::new(),
            })
            .unwrap_or_default();
        let implements_args: HashMap<Arc<str>, Vec<Type>> = class_doc
            .iter()
            .flat_map(|d| &d.implements)
            .filter_map(|raw| match resolve_generic_tag(raw) {
                Type::Object(name, args) => Some((intern(&name), args)),
                _ => None,
            })
            .collect();
        // `@use Trait<Arg, ...>` - unlike `extends`/`implements`, lives
        // on the `use` statement's own doc comment, not `c.doc_comment`
        // (`class_doc` above) - see `ClassInfo::uses_args`'s doc comment.
        // `uses_doc_comments` repeats the identical string once per trait
        // name in a multi-name `use A, B;` statement, so dedup (collect
        // into a `HashSet` first) before re-parsing each distinct doc
        // comment only once.
        let uses_args: HashMap<Arc<str>, Vec<Type>> = c
            .uses_doc_comments
            .iter()
            .filter_map(|d| d.as_deref())
            .collect::<HashSet<_>>()
            .into_iter()
            .map(docblock::parse)
            .flat_map(|d| d.uses)
            .filter_map(|raw| match resolve_generic_tag(&raw) {
                Type::Object(name, args) => Some((intern(&name), args)),
                _ => None,
            })
            .collect();
        let template_bounds: HashMap<Arc<str>, Type> = class_doc
            .as_ref()
            .map(|d| {
                d.template_bounds
                    .iter()
                    .map(|(name, raw)| (intern(name), resolve_generic_tag(raw)))
                    .collect()
            })
            .unwrap_or_default();
        let template_defaults: HashMap<Arc<str>, Type> = class_doc
            .as_ref()
            .map(|d| {
                d.template_defaults
                    .iter()
                    .map(|(name, raw)| (intern(name), resolve_generic_tag(raw)))
                    .collect()
            })
            .unwrap_or_default();
        let covariant_templates: HashSet<Arc<str>> = class_doc
            .as_ref()
            .map(|d| d.covariant_templates.iter().map(|s| intern(s)).collect())
            .unwrap_or_default();
        let contravariant_templates: HashSet<Arc<str>> = class_doc
            .as_ref()
            .map(|d| {
                d.contravariant_templates
                    .iter()
                    .map(|s| intern(s))
                    .collect()
            })
            .unwrap_or_default();

        // Real PHP (8.0+) makes any class declaring a public
        // `__toString()` automatically implement `Stringable`, with no
        // `implements Stringable` needed - added specifically so the
        // pre-existing body of `__toString()`-bearing classes would work
        // seamlessly with the `string|Stringable` idiom without
        // modification. Same shape as the enum UnitEnum/BackedEnum
        // implicit-interface handling below, for a different implicit
        // interface.
        let mut implements: Vec<Arc<str>> = c
            .implements
            .iter()
            .map(|s| intern(&ctx.resolve(s)))
            .collect();
        if methods.contains_key("__toString") {
            implements.push(intern("Stringable"));
        }
        let trait_aliases: Vec<TraitAlias> = c
            .trait_aliases
            .iter()
            .map(|a| TraitAlias {
                trait_name: a.trait_name.as_ref().map(|t| intern(&ctx.resolve(t))),
                method: a.method.clone(),
                alias: a.alias.clone(),
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
        let resolve_tag_types = |raw: &[String]| -> Vec<Arc<str>> {
            raw.iter()
                .flat_map(|text| {
                    object_class_names(resolve_names_in_type(types::parse_type_string(text), ctx))
                })
                .map(|s| intern(&s))
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
        let mixin_case_refs: Vec<(Arc<str>, Arc<str>)> = class_doc
            .as_ref()
            .map(|d| {
                d.mixins
                    .iter()
                    .flat_map(|text| {
                        let ty = types::parse_type_string(text);
                        let mut refs = Vec::new();
                        collect_class_name_case_refs(&ty, ctx, &mut refs);
                        refs
                    })
                    .collect()
            })
            .unwrap_or_default();
        let deprecated = class_doc.as_ref().and_then(|d| d.deprecated.clone());
        let attribute_targets = compute_attribute_targets(&c.attrs);
        let doctrine_entity = compute_doctrine_entity(c, ctx, &properties);

        ClassInfo {
            kind: c.kind,
            is_abstract: c.is_abstract,
            is_final: c.is_final,
            is_readonly: c.is_readonly,
            extends: c.extends.iter().map(|s| intern(&ctx.resolve(s))).collect(),
            implements,
            methods,
            properties,
            consts,
            uses: c.uses.iter().map(|s| intern(&ctx.resolve(s))).collect(),
            abstract_methods,
            static_methods,
            final_methods,
            final_properties,
            final_methods_by_phpdoc,
            final_properties_by_phpdoc,
            trait_aliases,
            resolved_trait_conflicts: c.resolved_trait_conflicts.iter().cloned().collect(),
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
            mixins,
            mixin_case_refs,
            deprecated,
            attribute_targets,
            enum_case_values: HashMap::new(),
            type_aliases,
            type_alias_case_refs,
            type_imports,
            method_tags,
            doctrine_entity,
        }
    }

    /// An enum participates in the class hierarchy the same way a class
    /// does (methods, `implements`, `use`) - it just can't extend anything
    /// or declare instance properties. Cases aren't registered as anything
    /// yet: resolving `Suit::Hearts`'s type would need static-access type
    /// inference, which nothing in the checker does for *any* class today,
    /// not just enums - a separate, pre-existing gap this doesn't attempt
    /// to close.
    #[hotpath::measure]
    pub(crate) fn from_enum_decl(e: &EnumDecl, ctx: &NameContext) -> ClassInfo {
        // Enums can't be generic in real PHP (no `@template` on an enum
        // means anything), so there's no enclosing-class template list to
        // pass down here, unlike ClassInfo::from_decl. An enum CAN
        // `implements` a genuinely generic interface though (`enum Suit
        // implements \IteratorAggregate<int, self> {}`-shaped code is
        // real), so `@implements` tags are still worth resolving - just
        // with an empty `template_params` map, since there's no class-
        // level template a tag's argument could reference.
        let fqn = ctx.declared_name(&e.name);
        let class_doc: Option<DocBlock> = e.doc_comment.as_deref().map(docblock::parse);
        let implements_args: HashMap<Arc<str>, Vec<Type>> = class_doc
            .iter()
            .flat_map(|d| &d.implements)
            .filter_map(
                |raw| match resolve_names_in_type(types::parse_type_string(raw), ctx) {
                    Type::Object(name, args) => Some((intern(&name), args)),
                    _ => None,
                },
            )
            .collect();
        // See `ClassInfo::from_decl`'s identical `uses_args`
        // construction - the reasoning (`@use` lives on the `use`
        // statement's own doc comment) is unrelated to whether the
        // enclosing declaration is a class or an enum.
        let uses_args: HashMap<Arc<str>, Vec<Type>> = e
            .uses_doc_comments
            .iter()
            .filter_map(|d| d.as_deref())
            .collect::<HashSet<_>>()
            .into_iter()
            .map(docblock::parse)
            .flat_map(|d| d.uses)
            .filter_map(
                |raw| match resolve_names_in_type(types::parse_type_string(&raw), ctx) {
                    Type::Object(name, args) => Some((intern(&name), args)),
                    _ => None,
                },
            )
            .collect();
        let mut methods: HashMap<Arc<str>, FunctionSig> =
            collect_merged_methods(e.methods.iter().map(|m| {
                (
                    m.function.name.clone(),
                    FunctionSig::from_decl(
                        &m.function,
                        ctx,
                        &HashMap::new(),
                        &HashMap::new(),
                        m.visibility.clone(),
                    ),
                )
            }));
        // `cases(): array` is a real, built-in static method every enum
        // gets from the language itself (`UnitEnum`), and a backed
        // enum's `from(int|string $value): static`/`tryFrom(...): ?static`
        // (`BackedEnum`) likewise - none declared in source. Without
        // these, undefined-static-method detection would flag every real
        // `Enum::cases()`/`::tryFrom()` call.
        let self_ty = Type::Object(fqn.clone(), Vec::new());
        // Real PHPStan's own `functionMap.php` declares
        // `UnitEnum::cases()` as `list<static>` outright - a bare,
        // unmarked `Array` here would leave every caller's result
        // (e.g. `array_merge(['none'], HistoryImageType::cases())`)
        // unable to prove list-ness itself.
        let cases_return_ty = Type::Intersection(vec![
            Type::Array(
                Box::new(Type::unknown_array_key()),
                Box::new(self_ty.clone()),
            ),
            Type::Accessory(AccessoryKind::ArrayList),
        ]);
        methods.insert(
            intern("cases"),
            FunctionSig {
                params: Vec::new(),
                return_type: cases_return_ty.clone(),
                return_type_native: cases_return_ty,
                // Fully authoritative built-in shapes, not from any real
                // docblock - `true` so `flatten_phpdoc_inheritance` never
                // treats a synthesized enum method as an inheritance
                // candidate.
                return_has_own_doc: true,
                // No real docblock/native text behind any of these
                // synthesized enum methods - nothing to check the case of.
                return_case_refs: Vec::new(),
                return_explicit_mixed_hint: false,
                no_discard: false,
                visibility: Visibility::Public,
                is_internal: false,
                is_api: false,
                has_override_attr: false,
                deprecated: None,
                throws: None,
                assert_if_true: Vec::new(),
                assert_if_false: Vec::new(),
                assert: Vec::new(),
                body: None,
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
            let backing_ty = types::parse_type_string(backing);
            let value_param = ParamSig {
                name: intern("value"),
                ty: backing_ty.clone(),
                ty_native: backing_ty,
                variadic: false,
                has_default: false,
                by_ref: false,
                has_own_doc: true,
                explicit_mixed_hint: false,
                case_refs: Vec::new(),
                closure_this_type: None,
                out_ty: None,
            };
            methods.insert(
                intern("from"),
                FunctionSig {
                    params: vec![value_param.clone()],
                    return_type: self_ty.clone(),
                    return_type_native: self_ty.clone(),
                    return_has_own_doc: true,
                    return_case_refs: Vec::new(),
                    return_explicit_mixed_hint: false,
                    no_discard: false,
                    visibility: Visibility::Public,
                    is_internal: false,
                    is_api: false,
                    has_override_attr: false,
                    deprecated: None,
                    // `from()` throws a real `ValueError` when no case
                    // matches - unlike `tryFrom()`, which returns `null`
                    // instead.
                    throws: Some(Type::Object("ValueError".to_string(), Vec::new())),
                    assert_if_true: Vec::new(),
                    assert_if_false: Vec::new(),
                    assert: Vec::new(),
                    body: None,
                    purity_dependencies: None,
                    is_transitively_pure: false,
                    is_stub: false,
                    is_magic: true,
                },
            );
            methods.insert(
                intern("tryFrom"),
                FunctionSig {
                    params: vec![value_param],
                    return_type: Type::Union(vec![self_ty.clone(), Type::Null]),
                    return_type_native: Type::Union(vec![self_ty.clone(), Type::Null]),
                    return_has_own_doc: true,
                    return_case_refs: Vec::new(),
                    return_explicit_mixed_hint: false,
                    no_discard: false,
                    visibility: Visibility::Public,
                    is_internal: false,
                    is_api: false,
                    has_override_attr: false,
                    deprecated: None,
                    throws: None,
                    assert_if_true: Vec::new(),
                    assert_if_false: Vec::new(),
                    assert: Vec::new(),
                    body: None,
                    purity_dependencies: None,
                    is_transitively_pure: false,
                    is_stub: false,
                    is_magic: true,
                },
            );
        }
        let mut consts: HashMap<Arc<str>, ConstSig> = e
            .consts
            .iter()
            .map(|cc| {
                let doc = cc.doc_comment.as_deref().map(docblock::parse);
                (
                    cc.name.clone(),
                    ConstSig {
                        ty: infer_const_type(&cc.value),
                        visibility: cc.visibility.clone(),
                        is_internal: doc.as_ref().is_some_and(|d| d.internal),
                        deprecated: doc.as_ref().and_then(|d| d.deprecated.clone()),
                    },
                )
            })
            .collect();
        // A case is accessed exactly like a constant (`Suit::Hearts`,
        // same `ClassName::member` syntax as `Suit::SOME_CONST`) and has
        // a real, useful static type: the enum itself. Folding it into
        // `consts` (rather than a separate field) means the checker's
        // static-access existence check and its type inference both just
        // work, with no new lookup path needed. A case has no visibility
        // syntax in real PHP - always public.
        for case in &e.cases {
            consts.insert(
                case.name.clone(),
                ConstSig {
                    ty: Type::EnumCase(fqn.clone(), case.name.to_string()),
                    visibility: Visibility::Public,
                    // A case has no doc-comment/`@internal`/`@deprecated`
                    // convention of its own in real PHP either - same
                    // reasoning as its always-`Visibility::Public` above.
                    is_internal: false,
                    deprecated: None,
                },
            );
        }

        // Each case's own literal backing value, independent of whether
        // every case happens to be a bare literal (unlike `->value`'s
        // union type below, which requires all-or-nothing precision) -
        // `::from()`/`::tryFrom()` narrowing only needs to know a given
        // literal argument's matching case, if any, and a partial map
        // still answers that for the literal-valued cases it covers.
        let enum_case_values: HashMap<Arc<str>, Type> = e
            .cases
            .iter()
            .filter_map(|c| {
                let v = c.value.as_ref()?;
                let ty = match &v.kind {
                    ExprKind::Literal(Literal::String(s)) => Type::StringLiteral(s.to_string()),
                    ExprKind::Literal(Literal::Int(n)) => Type::IntLiteral(*n),
                    _ => return None,
                };
                Some((c.name.clone(), ty))
            })
            .collect();

        // Every enum case is a real object with a built-in `->name`
        // (its case name, as a string) - and a backed enum's case
        // additionally has a real `->value` typed to its backing type -
        // neither declared in source, both provided by the language
        // itself (`UnitEnum`/`BackedEnum`). Without registering these as
        // properties, undefined-property-on-read detection would flag
        // every real `$case->value` access.
        let mut properties = HashMap::new();
        properties.insert(
            intern("name"),
            PropertySig {
                ty: Type::String,
                // Every enum case's `->name` is language-guaranteed to
                // be a real, natively-`string`-typed property - matching
                // `has_default` below's "language-guaranteed, not a
                // guess" reasoning. Enums can't be extended, so no
                // override check ever actually reads this, but a
                // correct value costs nothing to provide.
                ty_native: Type::String,
                has_native_type: true,
                is_static: false,
                is_readonly: false,
                set_ty: None,
                is_virtual: false,
                visibility: Visibility::Public,
                write_visibility: Visibility::Public,
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
                .cases
                .iter()
                .filter_map(|c| enum_case_values.get(c.name.as_ref()).cloned())
                .collect();
            let value_ty = if !case_values.is_empty() && case_values.len() == e.cases.len() {
                match case_values.len() {
                    1 => case_values.into_iter().next().unwrap(),
                    _ => Type::Union(case_values),
                }
            } else {
                types::parse_type_string(backing)
            };
            properties.insert(
                intern("value"),
                PropertySig {
                    ty: value_ty,
                    // Same reasoning as `->name` above - language-
                    // guaranteed, but here `ty_native` is the plain
                    // backing type, not the possibly-narrower literal-
                    // union `ty` a fully-literal case set gets: real
                    // PHP's native reflection only ever sees the backing
                    // type (`string`/`int`), never a per-case refinement
                    // PHPStan derives from analysis alone.
                    ty_native: types::parse_type_string(backing),
                    has_native_type: true,
                    is_static: false,
                    is_readonly: false,
                    set_ty: None,
                    is_virtual: false,
                    visibility: Visibility::Public,
                    write_visibility: Visibility::Public,
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
        // checks (`is_subclass_of`/`is_assignable`), not member lookup.
        // Without this, real code narrowing/typing an enum against
        // `UnitEnum` would be flagged as a bound violation despite being
        // exactly what real PHP guarantees.
        let mut implements: Vec<Arc<str>> = e
            .implements
            .iter()
            .map(|s| intern(&ctx.resolve(s)))
            .collect();
        implements.push(intern("UnitEnum"));
        if e.backing_type.is_some() {
            implements.push(intern("BackedEnum"));
        }

        // One pass over `e.methods` instead of 3 separate filtered ones -
        // see `ClassInfo::from_decl`'s equivalent loop for the same
        // reasoning.
        let mut abstract_methods: HashSet<Arc<str>> = HashSet::new();
        let mut final_methods: HashSet<Arc<str>> = HashSet::new();
        let mut static_methods: HashSet<Arc<str>> = HashSet::new();
        for m in &e.methods {
            if m.is_abstract {
                abstract_methods.insert(m.function.name.clone());
            }
            if m.is_final && m.visibility != Visibility::Private {
                final_methods.insert(m.function.name.clone());
            }
            if m.is_static {
                static_methods.insert(m.function.name.clone());
            }
        }
        let trait_aliases: Vec<TraitAlias> = e
            .trait_aliases
            .iter()
            .map(|a| TraitAlias {
                trait_name: a.trait_name.as_ref().map(|t| intern(&ctx.resolve(t))),
                method: a.method.clone(),
                alias: a.alias.clone(),
            })
            .collect();

        ClassInfo {
            kind: ClassKind::Enum,
            is_abstract: false,
            is_final: true,
            // Real PHP rejects `readonly enum` at parse time - an enum
            // can never be `readonly`.
            is_readonly: false,
            // An enum can carry `@phpstan-immutable` on its own docblock
            // same as a class can - reuse the identical computation.
            is_immutable: class_doc
                .as_ref()
                .is_some_and(|d| d.immutable || d.readonly),
            extends: Vec::new(),
            implements,
            methods,
            properties,
            consts,
            uses: e.uses.iter().map(|s| intern(&ctx.resolve(s))).collect(),
            abstract_methods,
            final_methods,
            static_methods,
            // An enum can't declare instance properties at all (see
            // `ast::EnumDecl`'s doc comment) - never a `final`-property
            // candidate either.
            final_properties: HashSet::new(),
            // An enum can never be `extends`-subclassed by anything
            // (`is_final: true` above already reflects this) - no
            // ancestor walk could ever reach these sets.
            final_methods_by_phpdoc: HashSet::new(),
            final_properties_by_phpdoc: HashSet::new(),
            trait_aliases,
            resolved_trait_conflicts: e.resolved_trait_conflicts.iter().cloned().collect(),
            templates: Vec::new(),
            extends_args: Vec::new(),
            implements_args,
            uses_args,
            template_bounds: HashMap::new(),
            template_defaults: HashMap::new(),
            covariant_templates: HashSet::new(),
            contravariant_templates: HashSet::new(),
            // Real PHPStan's API surface has no enums to mark `@api`
            // anyway.
            is_api: false,
            is_api_do_not_implement: false,
            is_internal: class_doc.as_ref().is_some_and(|d| d.internal),
            // An enum can never `extends` anything, so no subclass could
            // ever exist to hold it to a consistent constructor contract.
            is_consistent_constructor: false,
            // An enum can technically `implements`/`use` a tagged
            // interface/trait, but not `extends` at all - a rare
            // combination, not modeled here.
            require_extends: Vec::new(),
            require_implements: Vec::new(),
            sealed: Vec::new(),
            // An enum can `@mixin` in real PHPStan's grammar, but this is
            // a rare combination not modeled here.
            mixins: Vec::new(),
            mixin_case_refs: Vec::new(),
            deprecated: class_doc.as_ref().and_then(|d| d.deprecated.clone()),
            attribute_targets: compute_attribute_targets(&e.attrs),
            enum_case_values,
            type_aliases: HashMap::new(),
            type_alias_case_refs: HashMap::new(),
            type_imports: Vec::new(),
            // Same scope trim as `mixins` above - a real `@method` on an
            // enum's own docblock is a rare combination not modeled here.
            method_tags: Vec::new(),
            doctrine_entity: None,
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
    pub(crate) fn merge(mut self, other: ClassInfo) -> ClassInfo {
        // Not a bare `HashMap::extend` (a plain overwrite, `self`'s
        // colliding entry discarded outright): a vendored stub's method
        // could otherwise silently overwrite an internal, correctly
        // `FunctionSig::is_stub`-marked one, losing that marker and
        // wrongly letting `registry::purity` classify a raw-memory-write
        // native function as pure. Routing a colliding method through
        // `FunctionSig::merge` instead (the same function used for a
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
