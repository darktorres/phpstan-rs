use std::collections::{HashMap, HashSet};

use crate::ast::{
    ClassDecl, ClassKind, EnumDecl, ExprKind, Literal, PropertyDecl, TraitAlias, Visibility,
};
use crate::docblock::{self, DocBlock};
use crate::registry::function_sig::{
    FunctionSig, ParamSig, apply_implicit_nullable_default, build_template_params,
    infer_const_type, mark_template_params, resolve_type,
};
use crate::registry::name_context::{NameContext, resolve_names_in_type};
use crate::types::{self, TemplateParam, Type};

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
    // `@template T of Bound` (Phase 10.5's bound-enforcement half) - the
    // resolved bound for whichever of `templates` above declared one.
    // Checked by `checker::check_generic_bounds` wherever this class is
    // referenced with concrete type arguments (`Collection<Foo>`), not
    // just parsed past like it was before this phase.
    pub template_bounds: HashMap<String, Type>,
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
    // Phase 39 follow-up: `@deprecated` on this class's own docblock -
    // consulted only at `new Foo()` (`checker::check_deprecated_instantiation`).
    // Deliberately doesn't imply anything about this class's own members
    // (see `FunctionSig::deprecated`'s doc comment for why that question
    // is left unclaimed) - this is purely "is the class itself tagged."
    pub deprecated: Option<String>,
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

        let methods: HashMap<String, FunctionSig> = c
            .methods
            .iter()
            .map(|m| {
                (
                    m.function.name.clone(),
                    FunctionSig::from_decl(
                        &m.function,
                        ctx,
                        &template_params,
                        m.visibility.clone(),
                    ),
                )
            })
            .collect();
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
                let ty = resolve_names_in_type(ty, ctx);
                let set_ty = property_hook_set_type(p, &template_params, ctx);
                (
                    p.name.clone(),
                    PropertySig {
                        ty,
                        is_static: p.is_static,
                        is_readonly: p.is_readonly,
                        set_ty,
                        visibility: p.visibility.clone(),
                        write_visibility: p
                            .write_visibility
                            .clone()
                            .unwrap_or(p.visibility.clone()),
                        is_internal: doc_ty.as_ref().is_some_and(|d| d.internal),
                        deprecated: doc_ty.as_ref().and_then(|d| d.deprecated.clone()),
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
                let doc_ty = ctor_doc.as_ref().and_then(|d| d.params.get(&p.name));
                let ty = resolve_type(doc_ty.map(String::as_str), p.type_hint.as_deref());
                let ty = mark_template_params(ty, &template_params);
                let ty = resolve_names_in_type(ty, ctx);
                let ty = apply_implicit_nullable_default(ty, p.default.as_ref());
                properties.insert(
                    p.name.clone(),
                    PropertySig {
                        ty,
                        is_static: false,
                        is_readonly: p.promoted_readonly,
                        set_ty: None,
                        visibility: p.promoted_visibility.clone().unwrap_or(Visibility::Public),
                        write_visibility: p
                            .promoted_visibility
                            .clone()
                            .unwrap_or(Visibility::Public),
                        is_internal: false,
                        deprecated: None,
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
                    let ty = resolve_names_in_type(ty, ctx);
                    PropertySig {
                        ty,
                        is_static: false,
                        is_readonly: tag.readable && !tag.writable,
                        set_ty: None,
                        visibility: Visibility::Public,
                        write_visibility: Visibility::Public,
                        is_internal: false,
                        deprecated: None,
                    }
                });
            }
        }

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
                        resolve_names_in_type(ty, ctx)
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
        // above, not just a bare `types::parse_type_string`.
        let resolve_generic_tag = |raw: &str| -> Type {
            let ty = types::parse_type_string(raw);
            let ty = mark_template_params(ty, &template_params);
            resolve_names_in_type(ty, ctx)
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
        let template_bounds: HashMap<String, Type> = class_doc
            .as_ref()
            .map(|d| {
                d.template_bounds
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
        let deprecated = class_doc.as_ref().and_then(|d| d.deprecated.clone());

        ClassInfo {
            kind: c.kind,
            is_abstract: c.is_abstract,
            is_final: c.is_final,
            extends: c.extends.iter().map(|s| ctx.resolve(s)).collect(),
            implements,
            methods,
            properties,
            consts,
            uses: c.uses.iter().map(|s| ctx.resolve(s)).collect(),
            abstract_methods,
            final_methods,
            trait_aliases,
            resolved_trait_conflicts: c.resolved_trait_conflicts.iter().cloned().collect(),
            templates,
            extends_args,
            implements_args,
            template_bounds,
            covariant_templates,
            contravariant_templates,
            is_api,
            is_api_do_not_implement,
            is_internal,
            deprecated,
            enum_case_values: HashMap::new(),
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
        let mut methods: HashMap<String, FunctionSig> = e
            .methods
            .iter()
            .map(|m| {
                (
                    m.function.name.clone(),
                    FunctionSig::from_decl(&m.function, ctx, &HashMap::new(), m.visibility.clone()),
                )
            })
            .collect();
        // `cases(): array` is a real, built-in static method every enum
        // gets from the language itself (`UnitEnum`), and a backed
        // enum's `from(int|string $value): static` /
        // `tryFrom(...): ?static` (`BackedEnum`) likewise - none
        // declared in source. Found the hard way, same as the
        // ->name/->value properties below: undefined-static-method
        // detection immediately flagged every single real
        // `Enum::cases()`/`::tryFrom()` call in the wild.
        let self_ty = Type::Object(fqn.clone(), Vec::new());
        methods.insert(
            "cases".to_string(),
            FunctionSig {
                params: Vec::new(),
                return_type: Type::Array(
                    Box::new(Type::unknown_array_key()),
                    Box::new(self_ty.clone()),
                ),
                return_type_native: Type::Array(
                    Box::new(Type::unknown_array_key()),
                    Box::new(self_ty.clone()),
                ),
                no_discard: false,
                visibility: Visibility::Public,
                is_internal: false,
                has_override_attr: false,
                deprecated: None,
                throws: None,
                assert_if_true: Vec::new(),
                assert_if_false: Vec::new(),
                assert: Vec::new(),
            },
        );
        if let Some(backing) = &e.backing_type {
            let backing_ty = types::parse_type_string(backing);
            let value_param = ParamSig {
                name: "value".to_string(),
                ty: backing_ty,
                variadic: false,
                has_default: false,
                by_ref: false,
            };
            methods.insert(
                "from".to_string(),
                FunctionSig {
                    params: vec![value_param.clone()],
                    return_type: self_ty.clone(),
                    return_type_native: self_ty.clone(),
                    no_discard: false,
                    visibility: Visibility::Public,
                    is_internal: false,
                    has_override_attr: false,
                    deprecated: None,
                    // `from()` throws a real `ValueError` when no case
                    // matches (confirmed directly via `php -r`) - unlike
                    // `tryFrom()`, which returns `null` instead.
                    throws: Some(Type::Object("ValueError".to_string(), Vec::new())),
                    assert_if_true: Vec::new(),
                    assert_if_false: Vec::new(),
                    assert: Vec::new(),
                },
            );
            methods.insert(
                "tryFrom".to_string(),
                FunctionSig {
                    params: vec![value_param],
                    return_type: Type::Union(vec![self_ty.clone(), Type::Null]),
                    return_type_native: Type::Union(vec![self_ty.clone(), Type::Null]),
                    no_discard: false,
                    visibility: Visibility::Public,
                    is_internal: false,
                    has_override_attr: false,
                    deprecated: None,
                    throws: None,
                    assert_if_true: Vec::new(),
                    assert_if_false: Vec::new(),
                    assert: Vec::new(),
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
                is_static: false,
                is_readonly: false,
                set_ty: None,
                visibility: Visibility::Public,
                write_visibility: Visibility::Public,
                is_internal: false,
                deprecated: None,
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
                    is_static: false,
                    is_readonly: false,
                    set_ty: None,
                    visibility: Visibility::Public,
                    write_visibility: Visibility::Public,
                    is_internal: false,
                    deprecated: None,
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
            extends: Vec::new(),
            implements,
            methods,
            properties,
            consts,
            uses: e.uses.iter().map(|s| ctx.resolve(s)).collect(),
            abstract_methods,
            final_methods,
            trait_aliases,
            resolved_trait_conflicts: e.resolved_trait_conflicts.iter().cloned().collect(),
            templates: Vec::new(),
            extends_args: Vec::new(),
            implements_args,
            template_bounds: HashMap::new(),
            covariant_templates: HashSet::new(),
            contravariant_templates: HashSet::new(),
            // Real PHPStan's own API surface has no enums to mark `@api`
            // anyway, so leaving these false is never a real gap.
            is_api: false,
            is_api_do_not_implement: false,
            is_internal: class_doc.as_ref().is_some_and(|d| d.internal),
            deprecated: class_doc.as_ref().and_then(|d| d.deprecated.clone()),
            enum_case_values,
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
        self.methods.extend(other.methods);
        self.properties.extend(other.properties);
        self.consts.extend(other.consts);
        self.enum_case_values.extend(other.enum_case_values);
        self.abstract_methods.extend(other.abstract_methods);
        self.final_methods.extend(other.final_methods);
        self.resolved_trait_conflicts
            .extend(other.resolved_trait_conflicts);
        self.covariant_templates.extend(other.covariant_templates);
        self.contravariant_templates
            .extend(other.contravariant_templates);
        self.template_bounds.extend(other.template_bounds);
        self.implements_args.extend(other.implements_args);
        // Same "a tag present on either declaration sticks" reasoning as
        // the `HashSet::extend` calls above - a stub override that
        // doesn't repeat `@api` on its own docblock must not un-mark a
        // class the real declaration already tagged.
        self.is_api = self.is_api || other.is_api;
        self.is_api_do_not_implement =
            self.is_api_do_not_implement || other.is_api_do_not_implement;
        self.is_internal = self.is_internal || other.is_internal;
        self.deprecated = self.deprecated.or(other.deprecated);
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

/// PHP 8.4 property hooks: an explicit `set(Type $value) { ... }` hook can
/// accept a wider type than the property's own backing/read type - a real
/// Piwigo pattern (`CurrentConfig`'s own `apiKeyDuration`/`chmodValue`/
/// `defaultFiltersViews`/etc.): `public array $x { set(array $value) {
/// $this->x = sanitize($value); } }` does its own coercion inside the hook
/// body before the real assignment ever happens. A property write is
/// checked against this parameter's own native type when present - the
/// hook's own doc comment (if any) isn't captured by the parser at all
/// (out of scope, same as the rest of a hook body), so only the native
/// type hint is available, which is exactly what every corpus case above
/// already declares. No hook, or an implicit `set { ... }` with no
/// parameter list, returns `None` - the property's own type stays the
/// write sink, unchanged.
fn property_hook_set_type(
    p: &PropertyDecl,
    template_params: &HashMap<String, TemplateParam>,
    ctx: &NameContext,
) -> Option<Type> {
    let (param, _) = p.hooks.as_ref()?.set.as_ref()?;
    let param = param.as_ref()?;
    let ty = resolve_type(None, param.type_hint.as_deref());
    let ty = mark_template_params(ty, template_params);
    Some(resolve_names_in_type(ty, ctx))
}
