use std::collections::{HashMap, HashSet};

use crate::ast::{ClassDecl, ClassKind, EnumDecl, ExprKind, Literal, TraitAlias, Visibility};
use crate::docblock::{self, DocBlock};
use crate::registry::function_sig::{
    FunctionSig, ParamSig, apply_implicit_nullable_default, build_template_params,
    build_type_aliases, infer_const_type, mark_template_params, resolve_type,
    substitute_type_aliases,
};
use crate::registry::name_context::{
    NameContext, collect_class_name_case_refs, resolve_names_in_type,
};
use crate::types::{self, AccessoryKind, TemplateParam, Type};

mod attributes;
mod tag_synthesis;
mod virtual_property;

use attributes::compute_attribute_targets;
use tag_synthesis::method_tag_sig;
pub(crate) use tag_synthesis::object_class_names;
pub(crate) use virtual_property::hook_body_references_property;
use virtual_property::{hooks_reference_backing_storage, property_hook_set_type};

#[derive(Debug, Clone)]
pub struct PropertySig {
    pub ty: Type,
    pub is_static: bool,
    pub is_readonly: bool,
    // Phase 57: `public`/`private`/`protected` - previously parsed
    // (`PropertyDecl::visibility`/`Param::promoted_visibility`) but
    // discarded before reaching this struct, so no checker code anywhere
    // could enforce access legality. A magic `@property` tag and an
    // enum's own built-in `name`/`value` are always `Visibility::Public`
    // - real PHP has no way to declare either any other way.
    pub visibility: Visibility,
    // PHP 8.4 asymmetric visibility (`public private(set) string $x;`) -
    // the write-side visibility, defaulting to `visibility` itself (the
    // read side) for an ordinary, symmetric property, or one with no
    // established per-item place to write a separate write-visibility
    // (a promoted constructor property, a magic `@property` tag, an
    // enum's own built-in `name`/`value`) - same restriction
    // `is_internal` below already has.
    pub write_visibility: Visibility,
    // PHP 8.4 property hooks: an explicit `set(Type $value) { ... }` hook
    // can accept a wider type than the property's own backing/read type
    // (`public array $x { set(array $value) { $this->x =
    // sanitize($value); } }` - the hook does its own coercion inside the
    // body before the real assignment). `None` when there's no hook, or
    // the hook uses an implicit `set { ... }` with no parameter list at
    // all - a write then still checks against `ty` as before.
    pub set_ty: Option<Type>,
    // Phase 43: PHP 8.4's own "virtual property" concept - a hooked
    // property with no backing storage allocated for it at all. Real PHP
    // computes this itself (via the Zend engine/BetterReflection, neither
    // vendored here) as a purely *syntactic* compile-time scan: a hooked
    // property is virtual iff no hook body contains, anywhere in its own
    // function scope (including an unreachable branch like `if (false)
    // {...}` - not reachability-based), a `$this->propName` property-fetch
    // for that exact property (read, bare unused statement, `isset()`, or
    // `unset()` all count identically; a *nested* closure/arrow-fn/
    // anonymous-class body does NOT count - it's a separate function
    // scope; neither does a dynamic `$this->$name` access or an indirect
    // copy like `$self = $this; $self->propName`). Verified empirically
    // against real PHP 8.5's own `ReflectionProperty::isVirtual()` across
    // ~15 shapes (no vendored source exists for this specific algorithm -
    // see ROADMAP.md's Phase 43 entry), not recalled. Always `false` for
    // a property with no hooks at all, matching `isVirtual()` on an
    // ordinary property. Gates `propertyGetHook.noRead`/
    // `propertySetHook.noAssign` (`checker/oop.rs::check_property_hooks`)
    // exactly the way real PHPStan's own `GetNonVirtualPropertyHookReadRule`/
    // `SetNonVirtualPropertyHookAssignRule` do.
    pub is_virtual: bool,
    // Phase 67: `@internal` on this property's own docblock - see
    // `docblock::DocBlock::internal`'s own doc comment. Only ever set for
    // a real, plainly-declared property with its own doc comment
    // (`from_decl`'s first construction site below); a promoted
    // constructor property, a magic `@property` tag, and an enum's own
    // built-in `name`/`value` all stay `false` - none of them has an
    // established per-item place to write the tag.
    pub is_internal: bool,
    // Phase 39 follow-up: `@deprecated` on this property's own doc
    // comment - direct mirror of `FunctionSig::deprecated`'s identical
    // "own tag only, no class-level fallback" scoping. Only ever set for
    // a real, plainly-declared property with its own doc comment, same
    // restriction `is_internal` above already has.
    pub deprecated: Option<String>,
    // Phase 60: whether this property's own declaration (an ordinary
    // `PropertyDecl::default`, or a promoted constructor parameter's own
    // `Param::default`) gives it a default value - `checker/uninitialized
    // .rs`'s own Phase 62 "genuinely might still be uninitialized"
    // candidate gate needs the class's own AST to compute this, which
    // isn't available for an arbitrary `$obj->prop` access (only for
    // `$this->prop` inside the class currently being checked) - this
    // field makes the same fact available from `PropertySig` alone, for
    // `check_pointless_offset_or_property_probe`'s own "maybe genuinely
    // uninitialized, don't flag isset() as redundant" exemption. `false`
    // (the safe, permissive default - never *wrongly* exempts a property
    // that really does always have a value) for every synthetic property
    // with no real declaration to read a default from (a magic
    // `@property` tag, an enum's own built-in `name`/`value`).
    pub has_default: bool,
    // Phase 64 Item 3: PHP 8.4 `abstract` property modifier (only legal
    // on a hooked property inside an abstract class, but tracked here
    // unconditionally like `is_readonly` above - the parser/engine
    // already enforce the legality half). `false` for every synthetic
    // property (promoted param, magic `@property` tag, enum built-in) -
    // none of those shapes can ever be declared `abstract`.
    pub is_abstract: bool,
    // Phase 64 Item 1: whether a `get`/`set` hook is *declared at all*
    // (real PHP's own `hasHook()`), independent of whether it has a real
    // body - `PropertyHookBody::Abstract` is what makes this question
    // answerable at all (see that variant's own doc comment). `false`
    // for every synthetic property (promoted param, magic `@property`
    // tag, enum built-in) - none of those can carry hooks.
    pub has_get_hook: bool,
    pub has_set_hook: bool,
    // Phase 64: the native, no-docblock-merge type this property alone
    // declares - the exact `ty`-vs-`ty_native` split `ParamSig` already
    // has (see that field's own doc comment), needed here for
    // `OverridingPropertyRule`'s own `property.nativeType`/
    // `.missingNativeType`/`.extraNativeType` family, which real PHPStan
    // checks purely at the native level (never the PHPDoc-merged type).
    // `resolve_type(None, native_hint)` collapses "no native hint" and an
    // explicit `mixed` hint into the same `Type::Mixed` value, which is
    // why `has_native_type` below must be a separate boolean rather than
    // testing this field against `Type::Mixed`.
    pub ty_native: Type,
    pub has_native_type: bool,
    // Phase 48 follow-up (item 7c): `ParamSig::case_refs`'s own property-
    // position equivalent - see that field's own doc comment. Populated
    // identically at all three real construction sites (plain declared
    // property, promoted constructor property, `@property`/`-read`/
    // `-write` tag) - the tag site rides along for free, since it's the
    // exact same `PropertySig` the real-property sites already populate.
    pub case_refs: Vec<(String, String)>,
    // Phase 48 follow-up (item 6a): `set_ty`'s own case_refs equivalent -
    // `property_hook_set_type` never collected these, unlike the sibling
    // `ty` field two lines above its own call site.
    pub set_case_refs: Vec<(String, String)>,
}

// Phase 57's own still-open item: a class constant's own `public`/
// `private`/`protected` modifier (PHP 7.1+) - `ast::ClassConst` already
// carries `visibility` (added for Phase 59's unused-private-constant
// detection), but it never reached the registry's own `consts` map,
// which used to store a bare `Type` with no way to enforce access
// legality at all. Mirrors `PropertySig`'s identical shape, minus the
// property-only concerns (`is_static`/`is_readonly`/`set_ty`) a constant
// has no equivalent of.
#[derive(Debug, Clone)]
pub struct ConstSig {
    pub ty: Type,
    pub visibility: Visibility,
    // Phase 67 follow-up: `@internal` on this constant's own doc
    // comment - previously blocked on `ConstSig` not existing at all
    // (`ClassInfo::consts` was a bare `HashMap<String, Type>`); now that
    // it does (added for the visibility field above), this is a direct
    // mirror of `PropertySig::is_internal`. Always `false` for an enum
    // case (no doc-comment/`@internal` convention of its own, same as
    // its own always-`Visibility::Public`).
    pub is_internal: bool,
    // Phase 39 follow-up: `@deprecated` on this constant's own doc
    // comment - direct mirror of `PropertySig::deprecated` just above.
    // Always `None` for an enum case, same reasoning as its own always-
    // `false` `is_internal`.
    pub deprecated: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ClassInfo {
    pub kind: ClassKind,
    pub is_abstract: bool,
    pub is_final: bool,
    // Phase 37: `readonly class` (PHP 8.2) - see `ast::ClassDecl::
    // is_readonly`'s own doc comment for why this is only just now
    // surfaced at the class level. Always `false` for an enum (real PHP
    // rejects `readonly enum` at parse time - never reached this field at
    // all) or a trait composed in (a trait has no `readonly`-ness of its
    // own to inherit; `readonly class Foo { use T; }` only ever makes
    // `Foo`'s own properties readonly, never retroactively marks `T`).
    pub is_readonly: bool,
    // Phase 41 follow-up: `@phpstan-immutable`/`@phpstan-readonly` on
    // this class's own docblock - see `ClassInfo::from_decl`'s own
    // construction site for the real reasoning. Never baked with parent-
    // inheritance (`Registry::class_is_immutable` walks `extends` at
    // query time instead) - this field alone only ever answers "did
    // *this exact* declaration carry the tag," not "is this class
    // immutable considering its whole ancestor chain."
    pub is_immutable: bool,
    pub extends: Vec<String>,
    pub implements: Vec<String>,
    pub methods: HashMap<String, FunctionSig>,
    pub properties: HashMap<String, PropertySig>,
    pub consts: HashMap<String, ConstSig>,
    pub uses: Vec<String>,
    // Method names declared `abstract` in this class's own body (not
    // walked up the hierarchy - a completeness check needs to walk
    // `extends`/`implements` itself and consult each ancestor's own set).
    pub abstract_methods: HashSet<String>,
    // Method names declared `final` in this class's own body.
    pub final_methods: HashSet<String>,
    // Phase 64: method names declared `static` in this class's own body -
    // same "not walked up the hierarchy" shape as `abstract_methods`/
    // `final_methods` above, used by `check_one_method_override`'s own
    // `method.nonStatic`/`method.static` check (real PHPStan's
    // `OverridingMethodRule`: an override must match its ancestor's own
    // static-ness exactly, either direction).
    pub static_methods: HashSet<String>,
    // Phase 64 Item 3: property names declared `final` in this class's
    // own body (PHP 8.4) - same "not walked up the hierarchy" shape as
    // `final_methods` above; `Registry::find_final_ancestor_property`
    // does that walk. Real PHP fatals overriding a `final` property in a
    // subclass exactly like a `final` method.
    pub final_properties: HashSet<String>,
    // Phase 64: `@final`'s own docblock-only siblings to `final_methods`/
    // `final_properties` above - real PHP never enforces this convention
    // (no engine-level guarantee backs it, unlike the native keyword), so
    // it needs its own explicit ancestor-walk consultation
    // (`Registry::find_final_ancestor_method`/`find_final_ancestor_property`,
    // real keyword taking priority when both somehow apply on the same
    // ancestor, matching real PHPStan's own if/elseif order).
    pub final_methods_by_phpdoc: HashSet<String>,
    pub final_properties_by_phpdoc: HashSet<String>,
    // `use A, B { B::hello as helloB; }` - each entry's `trait_name` (when
    // present) is already resolved to the same fully-qualified name
    // `uses` itself is keyed under, so `flatten_traits` can look it up in
    // `self.classes` directly.
    pub trait_aliases: Vec<TraitAlias>,
    // Method/property names explicitly mentioned in ANY trait-adaptation
    // clause (`insteadof` or `as`) - Finding 58's own trait-conflict check
    // treats these as already addressed by the class author, even for
    // `insteadof` (whose actual trait-selection this project doesn't
    // otherwise model).
    pub resolved_trait_conflicts: HashSet<String>,
    // This class's own `@template T` names, in declaration order - order
    // matters, since a generic reference (`Collection<User>`) binds its
    // type arguments to these positionally (`checker::
    // class_template_bindings`). Empty for a non-generic class.
    pub templates: Vec<String>,
    // `@extends Parent<Arg, ...>`'s own argument list (Phase 10.5) - fixes
    // `extends.first()`'s own `@template` names for every instance of
    // this class, independent of whether this class is itself generic
    // (e.g. `class IntCollection extends Collection<int> {}`, which isn't
    // generic itself but permanently binds `Collection`'s own `T` to
    // `int`). An argument can itself reference *this* class's own
    // `@template` name (`class Box<T> extends Collection<T> {}`) - such
    // an argument is `Type::TemplateParam(name)`, substituted using
    // whatever this class's own templates are bound to at each use, not
    // a fixed value. Empty when there's no `@extends` tag, or no native
    // `extends` for it to apply to.
    pub extends_args: Vec<Type>,
    // Same idea as `extends_args`, but keyed by which interface each
    // `@implements Iface<Arg, ...>` tag applies to (a class can implement
    // more than one at once) - the key is resolved to the same
    // fully-qualified name `implements` itself uses, so the two agree.
    pub implements_args: HashMap<String, Vec<Type>>,
    // Same idea again, but for a generic trait named in an `@use
    // Trait<Arg, ...>` tag (Phase 54) - keyed the same way as
    // `implements_args`. Unlike `extends`/`implements`, which are both
    // written on the class's own top-level docblock, real PHPStan
    // resolves `@use` from the `use` *statement*'s own doc comment
    // (confirmed directly, `vendor/phpstan-src`'s own `TemplateObjectType`:
    // `/** @use TemplateTypeTrait<ObjectType> */` sits right above `use
    // TemplateTypeTrait;`, not the class's separate `/** @api */` docblock
    // above it) - see `ClassDecl::uses_doc_comments`'s own doc comment for
    // where this project captures that.
    pub uses_args: HashMap<String, Vec<Type>>,
    // `@template T of Bound` (Phase 10.5's bound-enforcement half) - the
    // resolved bound for whichever of `templates` above declared one.
    // Checked by `checker::check_generic_bounds` wherever this class is
    // referenced with concrete type arguments (`Collection<Foo>`), not
    // just parsed past like it was before this phase.
    pub template_bounds: HashMap<String, Type>,
    // `@template T = Default` (Phase 54/80) - the resolved default for
    // whichever of `templates` above declared one, mirroring `template_
    // bounds`'s own shape/construction exactly (same `class_doc.
    // template_defaults` raw-string source, same `resolve_generic_tag`
    // resolution). A generic reference supplying fewer type arguments
    // than `templates.len()` is only a real arity error
    // (`check_generic_reference_arity`, `checker/generics.rs`) for a
    // template name *absent* from this map - one *with* a default is
    // legitimately optional to supply.
    pub template_defaults: HashMap<String, Type>,
    // Names from `templates` above additionally declared
    // `@template-covariant`/`@template-contravariant` (Phase 10.5's
    // variance half) - a name absent from both stays invariant.
    pub covariant_templates: HashSet<String>,
    pub contravariant_templates: HashSet<String>,
    // Phase 40: this class/interface/trait's own docblock carries `@api`
    // (or, equivalently, `@api-do-not-implement`, which implies it - see
    // `DocBlock::api`'s own doc comment). Consulted only by
    // `checker::phpstan_api`, and only ever meaningful for a class this
    // project didn't declare itself (i.e. one loaded from a real
    // `phpstan/phpstan` `vendor/` tree as context) - `false` for every
    // ordinary class, which is exactly the right default.
    pub is_api: bool,
    // `@api-do-not-implement` specifically - an `@api` interface/class a
    // third party must not `extends`/`implements`, even though calling
    // its already-declared members is still fully covered by `is_api`
    // above.
    pub is_api_do_not_implement: bool,
    // Phase 67: `@internal` on this class/interface/trait/enum's own
    // docblock - see `docblock::DocBlock::internal`'s own doc comment.
    pub is_internal: bool,
    // `@phpstan-consistent-constructor` on this class's own docblock -
    // see `docblock::DocBlock::consistent_constructor`'s own doc comment.
    pub is_consistent_constructor: bool,
    // Phase 39 follow-up: `@deprecated` on this class's own docblock -
    // consulted only at `new Foo()` (`checker::check_deprecated_instantiation`).
    // Deliberately doesn't imply anything about this class's own members
    // (see `FunctionSig::deprecated`'s doc comment for why that question
    // is left unclaimed) - this is purely "is the class itself tagged."
    pub deprecated: Option<String>,
    // Phase 37 follow-up: this class's own `#[\Attribute(flags)]` meta-
    // attribute flags (an `int-mask-of<Attribute::TARGET_*>`), if this
    // class is declared usable as an attribute class at all - `None`
    // means it isn't (real PHP's `ReflectionClass::isAttribute()` would
    // be `false`), which is a distinct question from what a *bare*
    // `#[\Attribute]` with no arguments means (`Attribute::TARGET_ALL`,
    // matching `Attribute::__construct`'s own default parameter value -
    // see `compute_attribute_targets`'s own doc comment).
    pub attribute_targets: Option<u32>,
    // Phase 79: for a backed enum only, each case's own literal backing
    // value (`StringLiteral`/`IntLiteral`), keyed by case name - lets
    // `::from()`/`::tryFrom()` narrow a literal-argument call to the
    // exact matching case's own `Type::EnumCase` instead of the coarse
    // `self`/`?self`. Only populated when every case's value is a bare
    // literal (mirrors the identical "don't guess" restriction the
    // existing `->value` union-of-literals property type already
    // applies) - empty for a non-enum, an unbacked enum, or a backed enum
    // with any non-literal case value (a case naming a class constant).
    pub enum_case_values: HashMap<String, Type>,
    // This class's own `@phpstan-type Name TypeExpr` local alias table
    // (`function_sig::build_type_aliases`), keyed by each alias's own
    // resolved FQN - already spliced into every member's own `ty` at
    // `from_decl` time (see `substitute_type_aliases`'s doc comment), but
    // kept here too for `Checker::inline_var_annotation`'s own separate,
    // check-time `@var` resolution path, which needs the same table to
    // expand a local variable annotated `/** @var Name $x */` inside one
    // of this class's own method bodies (real corpus shape: `league/uri-
    // interfaces`'s own `UriString::parse()`, `/** @var ComponentMap
    // $components */`). Empty for a class with no such tags at all.
    pub type_aliases: HashMap<String, Type>,
    // Phase 48 follow-up (item 7d): `ParamSig::case_refs`'s own
    // `@phpstan-type` equivalent - keyed the same way `type_aliases`
    // itself is (the alias's own resolved local name), `(raw, resolved)`
    // pairs for every class-name reference the alias's *own* RHS text
    // carries (not a reference *to* the alias elsewhere - that's this
    // same mechanism at whatever `@param`/`@return`/property tag actually
    // uses the alias, via its own `case_refs`).
    pub type_alias_case_refs: HashMap<String, Vec<(String, String)>>,
    // This class's own `@phpstan-import-type` tags, name-resolved (`from`
    // to the source class's own FQN, the imported name to the local key
    // this class's own members already reference it by) but not yet
    // looked up in the source class's own `type_aliases` - the source
    // class may not be indexed at all yet at `from_decl` time. Resolved
    // by `Registry::resolve_deferred_type_imports`, a later pass over the
    // fully-indexed registry (mirrors Phase 80's `resolve_deferred_value_of`
    // shape) - merged into `type_aliases` above once resolved, so this
    // field is empty again after that pass runs; never read afterward.
    pub type_imports: Vec<ResolvedTypeImport>,
    // `@method` tags, already resolved into real `FunctionSig`s but not
    // yet inserted into `methods` - see the long comment at this field's
    // own construction site in `from_decl` for why insertion has to wait
    // for `Registry::resolve_method_tags`, a later pass. Empty again
    // after that pass runs; never read afterward.
    pub method_tags: Vec<(String, FunctionSig)>,
    // `@phpstan-require-extends`/`@phpstan-require-implements`/
    // `@phpstan-sealed` - resolved directly to fully-qualified class
    // names (a tag's own value can legally be a union of several, already
    // flattened here by `object_class_names`; a non-`Object` member, e.g.
    // an unresolvable name, is silently dropped - the "don't guess"
    // precedent every other type-bearing tag in this project already
    // follows). See `checker::oop::check_require_tags`/`check_sealed_
    // hierarchy` for the two, differently-shaped consuming checks.
    pub require_extends: Vec<String>,
    pub require_implements: Vec<String>,
    pub sealed: Vec<String>,
    // `@mixin Type` - resolved FQNs, same shape/flattening as the three
    // above. Not yet looked up in the target's own `methods`/`properties`
    // (may not be indexed yet) - `Registry::resolve_deferred_mixins`,
    // mirroring `resolve_deferred_type_imports`'s own two-step shape.
    pub mixins: Vec<String>,
    // Phase 48 follow-up (item 7d): `ParamSig::case_refs`'s own `@mixin`-
    // tag equivalent - `(raw, resolved)` pairs for every `@mixin` target
    // on this class's own docblock, in declaration order (not keyed by
    // name - unlike `type_alias_case_refs` below, a class can carry
    // several `@mixin` tags with no unique name to key by). Populated via
    // its own small dedicated closure, not the shared `resolve_tag_types`
    // helper just above (also used by `require_extends`/`require_
    // implements`/`sealed`, unrelated to case-checking).
    pub mixin_case_refs: Vec<(String, String)>,
}

/// See `ClassInfo::type_imports`'s own doc comment.
#[derive(Debug, Clone)]
pub struct ResolvedTypeImport {
    // The key this class's own already-built member types reference the
    // import under (`substitute_type_aliases` matches on this exact key,
    // same as an ordinary local `@phpstan-type` alias would).
    pub local_key: String,
    pub source_class_fqn: String,
    // The name as declared on the *source* class's own `@phpstan-type` -
    // not yet resolved to that class's own key (needs its own namespace,
    // reconstructed from `source_class_fqn` at resolution time rather
    // than carried here, since a class's own containing namespace is
    // always recoverable from its own FQN).
    pub source_alias_name: String,
}

/// Same "two full declarations of one name collapse into their positional
/// union" rule `Registry::insert_built_decl`'s free-function path already
/// applies (Phase 32 Finding C, `registry/index.rs`) - phpstorm-stubs
/// represents a method's own version-split/overload the identical way
/// inside one class body (`DatePeriod::__construct`'s four declarations,
/// `ReflectionMethod::setAccessible()`'s pre-/post-8.0 split), but unlike
/// a free function's declarations (seen one at a time as the registry
/// walks top-level statements) a class's own `c.methods`/`e.methods`
/// arrives as one already-parsed batch - a bare `.collect()` into a
/// `HashMap` would silently keep only whichever declaration happens to be
/// last in source order and drop every alternate parameter list the
/// earlier declaration(s) carried, with zero trace (confirmed real:
/// `DatePeriod::__construct($start, $interval, $end)` - the stub's own
/// first, 3-required-arg overload - vanishes entirely, since the file's
/// last declaration is the unrelated `($isostr, $options = 0)` form).
fn collect_merged_methods(
    methods: impl Iterator<Item = (String, FunctionSig)>,
) -> HashMap<String, FunctionSig> {
    let mut out: HashMap<String, FunctionSig> = HashMap::new();
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
    pub(crate) fn from_decl(c: &ClassDecl, ctx: &NameContext) -> ClassInfo {
        let class_doc: Option<DocBlock> = c.doc_comment.as_deref().map(docblock::parse);
        let templates: Vec<String> = class_doc
            .as_ref()
            .map(|d| d.templates.clone())
            .unwrap_or_default();
        // Phase 54: this class's own `@template` metadata (bound/variance/
        // default), keyed by name - `templates` above stays a plain
        // `Vec<String>` (positional order still drives type-argument
        // binding elsewhere, e.g. `checker::class_template_bindings_rec`),
        // but every `Type::TemplateParam` this class's own docblock
        // produces (properties, consts, `@extends`/`@implements` args, and
        // every method's own params/return type below) now carries the
        // real metadata from here instead of a bare name.
        let class_scope = ctx.declared_name(&c.name);
        let template_params: HashMap<String, TemplateParam> = class_doc
            .as_ref()
            .map(|d| build_template_params(d, &class_scope, ctx))
            .unwrap_or_default();
        // `@phpstan-type Name TypeExpr` local aliases - resolved once,
        // here, and spliced into every member's own `ty` below (never
        // `ty_native`, which by definition never reflects a docblock at
        // all) via `substitute_type_aliases`. See that function's own
        // doc comment for why this must run *after* `resolve_names_in_
        // type` on each member's surrounding type, not before.
        let (type_aliases, type_alias_case_refs) =
            build_type_aliases(class_doc.as_ref(), &template_params, ctx);
        // `@phpstan-import-type` - resolved just enough here (both names,
        // via this class's own `ctx`) to be looked up in the source
        // class's own `type_aliases` once it's indexed too; see
        // `ResolvedTypeImport`'s own doc comment.
        let type_imports: Vec<ResolvedTypeImport> = class_doc
            .as_ref()
            .map(|d| {
                d.type_imports
                    .iter()
                    .map(|imp| {
                        let local_name = imp.as_name.as_deref().unwrap_or(&imp.alias);
                        ResolvedTypeImport {
                            local_key: ctx.resolve(local_name),
                            source_class_fqn: ctx.resolve(&imp.from),
                            source_alias_name: imp.alias.clone(),
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();

        let methods: HashMap<String, FunctionSig> =
            collect_merged_methods(c.methods.iter().map(|m| {
                (
                    m.function.name.clone(),
                    FunctionSig::from_decl(
                        &m.function,
                        ctx,
                        &template_params,
                        &type_aliases,
                        m.visibility.clone(),
                    ),
                )
            }));
        let abstract_methods: HashSet<String> = c
            .methods
            .iter()
            .filter(|m| m.is_abstract)
            .map(|m| m.function.name.clone())
            .collect();
        // A `private` method isn't part of a class's inherited/overridable
        // surface at all in real PHP - a subclass declaring its own
        // same-named private method is an independent, unrelated
        // declaration, not an override - so `final private` doesn't
        // actually restrict anything a subclass does (confirmed via
        // vendor/phpstorm-stubs: `Exception`'s own `final private function
        // __clone()` is freely redeclared, identically, by many of its
        // subclasses, and real PHP raises no error for it).
        let final_methods: HashSet<String> = c
            .methods
            .iter()
            .filter(|m| m.is_final && m.visibility != Visibility::Private)
            .map(|m| m.function.name.clone())
            .collect();
        let static_methods: HashSet<String> = c
            .methods
            .iter()
            .filter(|m| m.is_static)
            .map(|m| m.function.name.clone())
            .collect();
        // Phase 64: `@final`'s own docblock-only sibling to `final_methods`
        // just above - same private-is-not-overridable exemption, same
        // reasoning (a purely static-analysis convention real PHP itself
        // never enforces, so it needs its own explicit ancestor walk
        // rather than piggybacking on native keyword's own compile-time
        // guarantee).
        let final_methods_by_phpdoc: HashSet<String> = c
            .methods
            .iter()
            .filter(|m| {
                m.visibility != Visibility::Private
                    && m.function
                        .doc_comment
                        .as_deref()
                        .map(docblock::parse)
                        .is_some_and(|d| d.is_final)
            })
            .map(|m| m.function.name.clone())
            .collect();
        // Phase 64 Item 3: same private-is-not-overridable exemption
        // `final_methods` above already applies, one member kind over -
        // a `private` property (final or not) is never part of a
        // subclass's overridable surface either.
        let mut final_properties: HashSet<String> = c
            .properties
            .iter()
            .filter(|p| p.is_final && p.visibility != Visibility::Private)
            .map(|p| p.name.clone())
            .collect();
        // `public final int $x` is real, valid PHP on a promoted
        // constructor property too (confirmed via `php -l`, and real PHP
        // does enforce `Cannot override final property` for it - the
        // design draft that originally scoped this phase wrongly assumed
        // no `final`/`abstract` syntax exists on a promoted property at
        // all; that's only true for `abstract`, which really is
        // impossible there - an abstract constructor can't declare any
        // promoted property, real fatal error).
        if let Some(ctor) = c.methods.iter().find(|m| m.function.name == "__construct") {
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

        // Phase 64: `@final`'s own docblock-only sibling to `final_properties`
        // above - same private-is-not-overridable exemption.
        let mut final_properties_by_phpdoc: HashSet<String> = c
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
        if let Some(ctor) = c.methods.iter().find(|m| m.function.name == "__construct") {
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

        let mut properties: HashMap<String, PropertySig> = c
            .properties
            .iter()
            .map(|p| {
                let doc_ty = p.doc_comment.as_deref().map(docblock::parse);
                let ty = resolve_type(
                    doc_ty.as_ref().and_then(|d| d.var_type.as_deref()),
                    p.type_hint.as_deref(),
                );
                let ty = mark_template_params(ty, &template_params);
                let mut case_refs = Vec::new();
                collect_class_name_case_refs(&ty, ctx, &mut case_refs);
                let ty = resolve_names_in_type(ty, ctx);
                let ty = substitute_type_aliases(ty, &type_aliases);
                let ty_native = resolve_type(None, p.type_hint.as_deref());
                let ty_native = mark_template_params(ty_native, &template_params);
                let ty_native = resolve_names_in_type(ty_native, ctx);
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
                        // `@readonly` (`DocBlock::readonly`'s own doc
                        // comment) folds into the exact same flag native
                        // `readonly` already sets - both flow through
                        // identical downstream enforcement.
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
        if let Some(ctor) = c.methods.iter().find(|m| m.function.name == "__construct") {
            let ctor_doc = ctor.function.doc_comment.as_deref().map(docblock::parse);
            for p in &ctor.function.params {
                if p.promoted_visibility.is_none() {
                    continue;
                }
                // Phase 93: same inline-`@var`-on-a-promoted-property
                // fallback as `FunctionSig::from_decl`'s own param loop -
                // see that one's doc comment. Needed here too, separately -
                // this builds the property's own `PropertySig` (read by
                // `$this->details`-style property access elsewhere in the
                // class), a completely different construction from the
                // `ParamSig` `FunctionSig::from_decl` builds for the same
                // parameter.
                let doc_ty: Option<String> = ctor_doc
                    .as_ref()
                    .and_then(|d| d.params.get(&p.name))
                    .cloned()
                    .or_else(|| {
                        p.doc_comment
                            .as_deref()
                            .map(docblock::parse)
                            .and_then(|d| d.var_type)
                    });
                let ty = resolve_type(doc_ty.as_deref(), p.type_hint.as_deref());
                let ty = mark_template_params(ty, &template_params);
                let mut case_refs = Vec::new();
                collect_class_name_case_refs(&ty, ctx, &mut case_refs);
                let ty = resolve_names_in_type(ty, ctx);
                let ty = substitute_type_aliases(ty, &type_aliases);
                let ty = apply_implicit_nullable_default(ty, p.default.as_ref());
                let ty_native = resolve_type(None, p.type_hint.as_deref());
                let ty_native = mark_template_params(ty_native, &template_params);
                let ty_native = resolve_names_in_type(ty_native, ctx);
                let ty_native = apply_implicit_nullable_default(ty_native, p.default.as_ref());
                // A promoted param's own inline `@readonly` (rare - most
                // real code that reaches for `@readonly` at all predates
                // promoted properties, let alone tags one - but cheap to
                // support given `p.doc_comment` is already parsed above
                // for `@var`'s own fallback).
                let doc_readonly = p
                    .doc_comment
                    .as_deref()
                    .map(docblock::parse)
                    .is_some_and(|d| d.readonly);
                // Phase 43.1: a promoted constructor property can carry
                // its own inline PHP 8.4 hooks (`Param::hooks`) exactly
                // like a plain property declaration can - resolved
                // through the identical generalized helpers, not new
                // logic.
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
                        // genuinely can carry inline hooks (Phase 43.1).
                        has_get_hook: p.hooks.as_ref().is_some_and(|h| h.get.is_some()),
                        has_set_hook: p.hooks.as_ref().is_some_and(|h| h.set.is_some()),
                        case_refs,
                        set_case_refs,
                    },
                );
            }
        }

        // `@property`/`@property-read`/`@property-write Type $name`
        // (Phase 13 Finding D) - magic properties with no real declared
        // field. Only fills a name this class doesn't already declare
        // for real - a real property/promoted param always wins, since
        // the tag exists to add virtual members, not override real ones.
        // `@property-write`'s own write-only direction isn't separately
        // enforced (no existing "write-only" checking mechanism to hook
        // into, and no corpus evidence calling for one) - treated the
        // same as a plain `@property` (readable and writable).
        if let Some(doc) = &class_doc {
            for tag in &doc.properties {
                properties.entry(tag.name.clone()).or_insert_with(|| {
                    let ty = types::parse_type_string(&tag.ty);
                    let ty = mark_template_params(ty, &template_params);
                    let mut case_refs = Vec::new();
                    collect_class_name_case_refs(&ty, ctx, &mut case_refs);
                    let ty = resolve_names_in_type(ty, ctx);
                    PropertySig {
                        ty,
                        // A magic `@property` tag has no native
                        // declaration at all to check against - `false`,
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
                        // member with no real backing declaration at all
                        // - genuinely unknown whether it "has a default"
                        // in any real sense, so `false` (the safe,
                        // never-wrongly-exempting choice) rather than a
                        // guess.
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

        // `@method` (Phase 36 follow-up) - a virtual method with no real
        // declaration at all. Deliberately *not* inserted into `methods`
        // here the way `@property` is just above: `methods` at this point
        // only reflects this class's own directly-declared methods, not
        // yet any trait-flattened-in ones (`Registry::flatten_traits` is
        // a separate, later pass) - inserting eagerly here would let a
        // same-named `@method` tag wrongly win over a real trait method
        // that hasn't been merged in yet, since `flatten_traits`'s own
        // merge is *also* vacant-only (real corpus false positive this
        // avoids: `pestphp/pest-plugin-arch`'s own `Blueprint`, whose own
        // `@method void assertDoesNotDependOn(Layer $target, Layer
        // $dependency)` would otherwise have permanently shadowed the
        // real, `use`-flattened `DependenciesAsserts::assertDoesNotDependOn`
        // - different parameter names, no real body - producing bogus
        // undefined-variable errors on the trait's own real code).
        // `Registry::resolve_method_tags` runs this insertion later
        // instead, once `flatten_traits` has had its own say.
        let method_tags: Vec<(String, FunctionSig)> = class_doc
            .as_ref()
            .map(|d| {
                d.method_tags
                    .iter()
                    .map(|tag| (tag.name.clone(), method_tag_sig(tag, &template_params, ctx)))
                    .collect()
            })
            .unwrap_or_default();

        // A `@var` immediately above a `const` declaration (a real,
        // common convention for pinning a `const array`'s own element
        // type more precisely than either its native hint - not even
        // kept past parsing - or `infer_const_type`'s own literal-value
        // inference can, e.g. `@var list<class-string<MiddlewareInterface>>`
        // on a `public const array X = [Foo::class, ...]`) wins over the
        // inferred type the same way a property's own `@var` already
        // does above - only when actually present, so a plain, undocumented
        // const keeps exactly `infer_const_type`'s existing inference.
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
                        // Phase 67 follow-up: `@internal` on this
                        // constant's own doc comment - see
                        // `PropertySig::is_internal`'s identical doc
                        // comment for the convention this mirrors.
                        is_internal: doc.as_ref().is_some_and(|d| d.internal),
                        deprecated: doc.as_ref().and_then(|d| d.deprecated.clone()),
                    },
                )
            })
            .collect();

        // `@extends`/`@implements` generic args (Phase 10.5) - an
        // argument can reference *this* class's own `@template` name
        // (`class Box<T> extends Collection<T> {}`), so it goes through
        // the same mark-then-resolve pipeline as every other member type
        // above, not just a bare `types::parse_type_string`. Also runs the
        // identical `substitute_type_aliases` pass every other member
        // type gets (see `type_aliases`'s own doc comment for why it must
        // come *after* `resolve_names_in_type`, not before) - a real,
        // previously-missing case found via this session's own corpus
        // diff: `friendsofphp/php-cs-fixer`'s own `AbstractFixer`
        // subclasses each declare a same-named but *locally, per-class*
        // `@phpstan-type _AutogeneratedComputedConfiguration` alias, then
        // restate `@implements ConfigurableFixerInterface<..., _Autogene
        // ratedComputedConfiguration>` on both a fixer and its own parent
        // - without this, the two references stayed distinct *opaque*
        // `Type::Object` nodes (one per class's own unresolved alias
        // name) instead of each expanding to its own real underlying
        // shape, so `check_cross_path_interface_conflicts`'s own exact-
        // equality comparison saw two "different" types where real
        // PHPStan - which always compares the *expanded* shape - sees an
        // equivalent one and reports nothing.
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
        let implements_args: HashMap<String, Vec<Type>> = class_doc
            .iter()
            .flat_map(|d| &d.implements)
            .filter_map(|raw| match resolve_generic_tag(raw) {
                Type::Object(name, args) => Some((name, args)),
                _ => None,
            })
            .collect();
        // `@use Trait<Arg, ...>` - unlike `extends`/`implements`, lives on
        // the `use` *statement*'s own doc comment, not `c.doc_comment`
        // (`class_doc` above) - see `ClassInfo::uses_args`'s own doc
        // comment. `uses_doc_comments` repeats the identical string once
        // per trait name in a multi-name `use A, B;` statement, so
        // dedup(collect into a `HashSet` first) before re-parsing each
        // distinct doc comment only once.
        let uses_args: HashMap<String, Vec<Type>> = c
            .uses_doc_comments
            .iter()
            .filter_map(|d| d.as_deref())
            .collect::<HashSet<_>>()
            .into_iter()
            .map(docblock::parse)
            .flat_map(|d| d.uses)
            .filter_map(|raw| match resolve_generic_tag(&raw) {
                Type::Object(name, args) => Some((name, args)),
                _ => None,
            })
            .collect();
        let template_bounds: HashMap<String, Type> = class_doc
            .as_ref()
            .map(|d| {
                d.template_bounds
                    .iter()
                    .map(|(name, raw)| (name.clone(), resolve_generic_tag(raw)))
                    .collect()
            })
            .unwrap_or_default();
        let template_defaults: HashMap<String, Type> = class_doc
            .as_ref()
            .map(|d| {
                d.template_defaults
                    .iter()
                    .map(|(name, raw)| (name.clone(), resolve_generic_tag(raw)))
                    .collect()
            })
            .unwrap_or_default();
        let covariant_templates = class_doc
            .as_ref()
            .map(|d| d.covariant_templates.clone())
            .unwrap_or_default();
        let contravariant_templates = class_doc
            .as_ref()
            .map(|d| d.contravariant_templates.clone())
            .unwrap_or_default();

        // Real PHP (8.0+) makes any class declaring a public
        // `__toString()` automatically implement `Stringable`, with no
        // `implements Stringable` needed - added specifically so the
        // pre-existing body of `__toString()`-bearing classes would work
        // seamlessly with the `string|Stringable` idiom without
        // modification. Same shape as the enum UnitEnum/BackedEnum
        // implicit-interface handling below, for a different implicit
        // interface.
        let mut implements: Vec<String> = c.implements.iter().map(|s| ctx.resolve(s)).collect();
        if methods.contains_key("__toString") {
            implements.push("Stringable".to_string());
        }
        let trait_aliases: Vec<TraitAlias> = c
            .trait_aliases
            .iter()
            .map(|a| TraitAlias {
                trait_name: a.trait_name.as_ref().map(|t| ctx.resolve(t)),
                method: a.method.clone(),
                alias: a.alias.clone(),
            })
            .collect();

        let is_api = class_doc.as_ref().is_some_and(|d| d.api);
        let is_api_do_not_implement = class_doc.as_ref().is_some_and(|d| d.api_do_not_implement);
        let is_internal = class_doc.as_ref().is_some_and(|d| d.internal);
        let is_consistent_constructor =
            class_doc.as_ref().is_some_and(|d| d.consistent_constructor);
        // Phase 41 follow-up: `@phpstan-immutable` on the *class's own*
        // docblock (not a property's - `DocBlock::readonly`'s own doc
        // comment covers that separate, pre-existing use of the same
        // tag) - real PHPStan's `ClassReflection::isImmutable()`, read
        // directly, treats `@phpstan-immutable` OR `@phpstan-readonly`
        // here identically, both making every property of the class (and
        // its subclasses) behave as if individually marked readonly.
        // Deliberately *not* baked with parent-inheritance here -
        // `Registry::class_is_immutable` (`registry/hierarchy.rs`) walks
        // `extends` at query time instead, avoiding a registry-build-
        // ordering hazard (a not-yet-indexed ancestor's own tag).
        let is_immutable = class_doc
            .as_ref()
            .is_some_and(|d| d.immutable || d.readonly);
        // `@phpstan-require-extends`/`-implements`/`@phpstan-sealed` -
        // each raw tag's own type expression is parsed, name-resolved,
        // then flattened to the class names it names (`object_class_names`)
        // exactly like real PHPStan's own `getObjectClassNames()` call
        // sites for these same three tags.
        let resolve_tag_types = |raw: &[String]| -> Vec<String> {
            raw.iter()
                .flat_map(|text| {
                    object_class_names(resolve_names_in_type(types::parse_type_string(text), ctx))
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
        let sealed = class_doc
            .as_ref()
            .map(|d| resolve_tag_types(&d.sealed))
            .unwrap_or_default();
        let mixins = class_doc
            .as_ref()
            .map(|d| resolve_tag_types(&d.mixins))
            .unwrap_or_default();
        // Phase 48 follow-up (item 7d): own small dedicated closure, not
        // `resolve_tag_types` above - see `mixin_case_refs`'s own doc
        // comment.
        let mixin_case_refs: Vec<(String, String)> = class_doc
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

        ClassInfo {
            kind: c.kind,
            is_abstract: c.is_abstract,
            is_final: c.is_final,
            is_readonly: c.is_readonly,
            extends: c.extends.iter().map(|s| ctx.resolve(s)).collect(),
            implements,
            methods,
            properties,
            consts,
            uses: c.uses.iter().map(|s| ctx.resolve(s)).collect(),
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
        }
    }

    /// An enum participates in the class hierarchy the same way a class
    /// does (methods, `implements`, `use`) - it just can't extend anything
    /// or declare instance properties. Cases aren't registered as anything
    /// yet: resolving `Suit::Hearts`'s type would need static-access type
    /// inference, which nothing in the checker does for *any* class today,
    /// not just enums - a separate, pre-existing gap this doesn't attempt
    /// to close.
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
        let implements_args: HashMap<String, Vec<Type>> = class_doc
            .iter()
            .flat_map(|d| &d.implements)
            .filter_map(
                |raw| match resolve_names_in_type(types::parse_type_string(raw), ctx) {
                    Type::Object(name, args) => Some((name, args)),
                    _ => None,
                },
            )
            .collect();
        // See `ClassInfo::from_decl`'s own identical `uses_args`
        // construction - the reasoning (`@use` lives on the `use`
        // statement's own doc comment) is unrelated to whether the
        // enclosing declaration is a class or an enum.
        let uses_args: HashMap<String, Vec<Type>> = e
            .uses_doc_comments
            .iter()
            .filter_map(|d| d.as_deref())
            .collect::<HashSet<_>>()
            .into_iter()
            .map(docblock::parse)
            .flat_map(|d| d.uses)
            .filter_map(
                |raw| match resolve_names_in_type(types::parse_type_string(&raw), ctx) {
                    Type::Object(name, args) => Some((name, args)),
                    _ => None,
                },
            )
            .collect();
        let mut methods: HashMap<String, FunctionSig> =
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
        // enum's `from(int|string $value): static` /
        // `tryFrom(...): ?static` (`BackedEnum`) likewise - none
        // declared in source. Found the hard way, same as the
        // ->name/->value properties below: undefined-static-method
        // detection immediately flagged every single real
        // `Enum::cases()`/`::tryFrom()` call in the wild.
        let self_ty = Type::Object(fqn.clone(), Vec::new());
        // Real PHPStan's own `functionMap.php` declares `UnitEnum::cases()`
        // as `list<static>` outright (confirmed directly in
        // `vendor/phpstan-src/resources/functionMap_php81delta.php`) - a
        // bare, unmarked `Array` here left every caller's own result
        // (`array_merge(['none'], HistoryImageType::cases())`, confirmed
        // live at `HistorySearchController::__invoke`) unable to prove
        // list-ness itself, a real corpus false positive.
        let cases_return_ty = Type::Intersection(vec![
            Type::Array(
                Box::new(Type::unknown_array_key()),
                Box::new(self_ty.clone()),
            ),
            Type::Accessory(AccessoryKind::ArrayList),
        ]);
        methods.insert(
            "cases".to_string(),
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
                name: "value".to_string(),
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
                "from".to_string(),
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
                    // matches (confirmed directly via `php -r`) - unlike
                    // `tryFrom()`, which returns `null` instead.
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
                "tryFrom".to_string(),
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
        let mut consts: HashMap<String, ConstSig> = e
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
        // static-access existence check and its type inference both
        // just work, with no new lookup path needed - previously cases
        // weren't registered as anything at all, so `Suit::Hearts`
        // always read as Mixed and a genuinely undefined case name was
        // never caught. A case has no visibility syntax in real PHP at
        // all - always public.
        for case in &e.cases {
            consts.insert(
                case.name.clone(),
                ConstSig {
                    ty: Type::EnumCase(fqn.clone(), case.name.clone()),
                    visibility: Visibility::Public,
                    // A case has no doc-comment/`@internal`/`@deprecated`
                    // convention of its own in real PHP either - same
                    // reasoning as its own always-`Visibility::Public`
                    // just above.
                    is_internal: false,
                    deprecated: None,
                },
            );
        }

        // Phase 79: each case's own literal backing value, independent
        // of whether *every* case happens to be a bare literal (unlike
        // `->value`'s own union type just below, which deliberately
        // requires all-or-nothing precision) - `::from()`/`::tryFrom()`
        // narrowing only needs to know a given literal argument's own
        // matching case, if any, and a partial map still answers that
        // for the literal-valued cases it does cover.
        let enum_case_values: HashMap<String, Type> = e
            .cases
            .iter()
            .filter_map(|c| {
                let v = c.value.as_ref()?;
                let ty = match &v.kind {
                    ExprKind::Literal(Literal::String(s)) => Type::StringLiteral(s.clone()),
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
        // itself (`UnitEnum`/`BackedEnum`). Found the hard way: adding
        // undefined-property-on-read detection immediately produced a
        // false positive on every single real `$case->value` access in
        // the wild, since nothing registered these as properties at all.
        let mut properties = HashMap::new();
        properties.insert(
            "name".to_string(),
            PropertySig {
                ty: Type::String,
                // Every enum case's `->name` is language-guaranteed to be
                // a real, natively-`string`-typed property - matching
                // `has_default` just below's identical "language-
                // guaranteed, not a guess" reasoning. Enums can't be
                // extended, so no override check ever actually reads this,
                // but a real, correct value costs nothing to provide.
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
                // `false`, so `isset($case->name)` can still be
                // correctly recognized as redundant.
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
            // Phase 34 Finding C: when every case's own value is a bare
            // literal (the overwhelmingly common shape - a case value
            // naming a class constant instead falls back to the plain
            // backing type below, "don't guess"), `->value` is typed as
            // the union of those literals, not the generic backing type
            // - matching real PHPStan's own precision for a backed
            // enum's `->value`. Real shape: `SortOrder: string { case
            // Asc = 'ASC'; case Desc = 'DESC'; }`'s `->value` needs to be
            // `'ASC'|'DESC'`, not bare `string`, to satisfy
            // `OrderByClause::__construct`'s own declared parameter.
            let case_values: Vec<Type> = e
                .cases
                .iter()
                .filter_map(|c| enum_case_values.get(&c.name).cloned())
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
                "value".to_string(),
                PropertySig {
                    ty: value_ty,
                    // Same reasoning as `->name` above - language-
                    // guaranteed, but here `ty_native` is the plain
                    // backing type, not the possibly-narrower literal-
                    // union `ty` a fully-literal case set gets - real
                    // PHP's own native reflection only ever sees the
                    // backing type (`string`/`int`), never a per-case
                    // refinement PHPStan derives from analysis alone.
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
                    // Same reasoning as `->name` just above - a backed
                    // enum case's `->value` is language-guaranteed too.
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
        // Found the hard way: real code narrowing/typing an enum against
        // `UnitEnum` (e.g. doctrine/lexer's own `AbstractLexer<T of
        // UnitEnum|string|int>`) was flagged as a bound violation despite
        // being exactly what real PHP guarantees.
        let mut implements: Vec<String> = e.implements.iter().map(|s| ctx.resolve(s)).collect();
        implements.push("UnitEnum".to_string());
        if e.backing_type.is_some() {
            implements.push("BackedEnum".to_string());
        }

        let abstract_methods: HashSet<String> = e
            .methods
            .iter()
            .filter(|m| m.is_abstract)
            .map(|m| m.function.name.clone())
            .collect();
        let final_methods: HashSet<String> = e
            .methods
            .iter()
            .filter(|m| m.is_final && m.visibility != Visibility::Private)
            .map(|m| m.function.name.clone())
            .collect();
        let static_methods: HashSet<String> = e
            .methods
            .iter()
            .filter(|m| m.is_static)
            .map(|m| m.function.name.clone())
            .collect();
        let trait_aliases: Vec<TraitAlias> = e
            .trait_aliases
            .iter()
            .map(|a| TraitAlias {
                trait_name: a.trait_name.as_ref().map(|t| ctx.resolve(t)),
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
            uses: e.uses.iter().map(|s| ctx.resolve(s)).collect(),
            abstract_methods,
            final_methods,
            static_methods,
            // An enum can't declare instance properties at all (see
            // `ast::EnumDecl`'s own doc comment) - never a `final`-
            // property candidate either.
            final_properties: HashSet::new(),
            // An enum can never be `extends`-subclassed by anything at
            // all (`is_final: true` above already reflects this) - no
            // ancestor walk could ever reach these sets, so there's
            // nothing real to compute here.
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
            // Real PHPStan's own API surface has no enums to mark `@api`
            // anyway, so leaving these false is never a real gap.
            is_api: false,
            is_api_do_not_implement: false,
            is_internal: class_doc.as_ref().is_some_and(|d| d.internal),
            // An enum can never `extends` anything, so no subclass could
            // ever exist to hold it to a consistent constructor contract
            // either way - never meaningful here.
            is_consistent_constructor: false,
            // An enum can technically `implements`/`use` a tagged
            // interface/trait, but not `extends` at all - a genuinely
            // rare combination, not modeled here (matching `type_aliases`/
            // `type_imports` just below, already left unbuilt for enums).
            require_extends: Vec::new(),
            require_implements: Vec::new(),
            sealed: Vec::new(),
            // An enum can `@mixin` in real PHPStan's own grammar (nothing
            // stops the tag syntactically), but this is a genuinely rare
            // combination not modeled here - same disclosed scope trim as
            // `type_aliases`/`type_imports`/`require_extends` above.
            mixins: Vec::new(),
            mixin_case_refs: Vec::new(),
            deprecated: class_doc.as_ref().and_then(|d| d.deprecated.clone()),
            attribute_targets: compute_attribute_targets(&e.attrs),
            enum_case_values,
            type_aliases: HashMap::new(),
            type_alias_case_refs: HashMap::new(),
            type_imports: Vec::new(),
            // Same disclosed scope trim as `mixins` just above - a real
            // `@method` on an enum's own docblock is a genuinely rare
            // combination not modeled here.
            method_tags: Vec::new(),
        }
    }

    /// Merges a second unconditional declaration of the same class name
    /// into `self` - PHPStan's own `stubFiles` mechanism (a real, common
    /// pattern: Piwigo's own `phpstan-stubs/Imagick.stub.php`) lets a
    /// project redeclare just *one* method of an already-known class as a
    /// targeted override, relying on the real declaration to still
    /// provide everything else. `self`/`other` are symmetric in the sense
    /// that it doesn't matter which file happened to be indexed first -
    /// whichever is `other` here (the *second* one indexed) wins on a
    /// same-name member conflict (`HashMap::extend`'s standard "later
    /// insert wins" behavior), matching a deliberate override's intent -
    /// but never *erases* a member, `extends`/`implements`, or generic
    /// information `self` already had that `other` simply doesn't
    /// mention (a bare `class Imagick { ... }` override redeclaring no
    /// `extends`/`implements` of its own must not blank out the real
    /// class's).
    pub(crate) fn merge(mut self, other: ClassInfo) -> ClassInfo {
        // Not a bare `HashMap::extend` (a plain overwrite, `self`'s own
        // colliding entry discarded outright) - a real corpus find
        // (Piwigo's own vendored *copy* of `jetbrains/phpstorm-stubs`,
        // analyzed as ordinary project source in this project's own
        // unscoped, whole-`vendor/`-included corpus check specifically):
        // `other`'s own `FFI::memset()` stub method silently overwrote
        // this project's own internal, correctly `FunctionSig::is_stub`-
        // marked one, losing that marker and wrongly letting
        // `registry::purity` classify `memset()` - a raw-memory-write
        // native function - as pure. Routing a colliding method through
        // `FunctionSig::merge` instead (the same function already used
        // for a top-level function's identical "declared twice" shape)
        // keeps its own permissive "either side's marker sticks" `is_stub`
        // computation in the one place that owns it, rather than
        // reimplementing that same reasoning here a second time.
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
        // doesn't repeat `@api` on its own docblock must not un-mark a
        // class the real declaration already tagged.
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
