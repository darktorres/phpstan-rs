use std::collections::HashMap;
use std::sync::Arc;

use crate::ast::{ExprKind, FunctionDecl, Stmt, Visibility};
use crate::checker::{has_explicit_mixed_nested_somewhere, union_types};
use crate::docblock::{self, DocBlock};
use crate::registry::name_context::{NameContext, resolve_names_in_type};
use crate::types::{self, TemplateParam, Type};

mod asserts;
mod generics;
mod type_resolution;

pub(crate) use asserts::resolve_assert_tags;
pub use asserts::{AssertInfo, AssertTarget};
pub(crate) use generics::{
    build_template_params, build_type_aliases, mark_template_params, substitute_type_aliases,
};
pub(crate) use type_resolution::{
    apply_implicit_nullable_default, reconcile_with_native, resolve_type,
};

#[derive(Debug, Clone)]
pub struct ParamSig {
    pub name: String,
    pub ty: Type,
    // The native, no-docblock-merge type this parameter alone declares -
    // the exact `ty`-vs-`ty_native` split `FunctionSig::return_type`/
    // `return_type_native` already has, and for the identical reason
    // (see that field's own doc comment): real PHP/PHPStan only enforce
    // method-override Liskov-compatibility at this native level by
    // default. `check_one_method_override`'s own param-narrowing check
    // used to compare the merged `ty` instead - real corpus false
    // positive, the extremely common Rector/PHPStan-`Collector<T>` idiom
    // of narrowing a parameter via `@param` alone while leaving a wide
    // native hint (`refactor(Node $node)`, narrowed only via `@param
    // SpecificNode $node`) was wrongly flagged as an LSP violation real
    // PHPStan doesn't, by default, check at all. Every other reader of
    // `ty` correctly wants the merged, docblock-refined type and must
    // keep using it unchanged.
    pub ty_native: Type,
    pub variadic: bool,
    pub has_default: bool,
    pub by_ref: bool,
    // Phase 87 (PHPDoc-inheritance backfill): starts as `true` only when
    // this parameter's own `@param` tag was present on this
    // declaration's own docblock (i.e. `from_decl`'s own `doc_ty` was
    // `Some`) - `false` means `ty` is really just `ty_native` (or a
    // phpDoc-less default), making this position eligible for
    // `Registry`'s own `flatten_phpdoc_inheritance` pass to backfill
    // from an ancestor, mirroring real PHPStan's `ResolvedPhpDocBlock::
    // mergeOneParentParamTags` (`array_key_exists($name, $paramTags)`).
    // That pass also *sets* this to `true` once it successfully
    // backfills `ty` from an ancestor - a position is then "documented"
    // (own or inherited) for the purposes of any *further* descendant's
    // own lookup, which is what makes multi-level transitivity resolve
    // one level per fixed-point pass; see that pass's own doc comment
    // for why an ancestor's own bit being `false` (nothing real to
    // inherit, not even from a bare native hint) must gate the merge
    // rather than the ancestor's `ty` being merely present.
    pub has_own_doc: bool,
    // Phase 88: whether *this declaration's own* native hint/`@param`
    // text (whichever produced `ty`) spells out `mixed` somewhere,
    // explicitly - computed once in `from_decl` via `checker::
    // has_explicit_mixed_nested_somewhere`, the exact same text this
    // parameter's own `doc_ty`/native hint locals already are. Exists
    // because `checker::missing_typehint`'s own "was `mixed` written on
    // purpose" rule used to *re-derive* this by re-reading the checked
    // declaration's own raw AST text at check time - correct as long as
    // a type only ever came from that same declaration's own docblock,
    // which stopped being true the moment `Registry::
    // flatten_phpdoc_inheritance` (Phase 87) could hand `ty` down from a
    // *different* declaration's docblock instead. That pass copies this
    // field alongside `ty` whenever it backfills a position, so an
    // inherited `array<string, mixed>`/nested-`mixed` type is still
    // correctly recognized as deliberate, not "no type given at all" -
    // real corpus false positive this fixes: every one of Piwigo's
    // `UrlServiceInterface`/`ExposesPageData`/... implementors and test
    // doubles that correctly omit their own docblock in favor of the
    // interface's.
    pub explicit_mixed_hint: bool,
    // Phase 36: `@param-closure-this Type $name`'s own resolved `Type` -
    // what `$this` becomes inside a `Closure`/`ArrowFn` literal argument
    // passed at this parameter's position, resolved through the same
    // template-marking + name-resolution pipeline `@param`'s own `ty`
    // merge already uses (see `from_decl` below). `None` when no such tag
    // targets this parameter - the overwhelming majority. Consumed at
    // each call-argument-checking site via `checker::calls::
    // check_closure_literal_with_this_and_scope`, the same mechanism
    // `Closure::bind`/`->bindTo()` already use.
    pub closure_this_type: Option<Type>,
}

#[derive(Debug, Clone)]
pub struct FunctionSig {
    pub params: Vec<ParamSig>,
    pub return_type: Type,
    // The return type the *native* signature alone declares, ignoring any
    // `@return` docblock refinement - real PHP only ever enforces
    // override-compatibility (Liskov) at this native level, and by
    // default so does real PHPStan (`checkPhpDocMethodSignatures` is off
    // unless a project opts in, confirmed neither Piwigo's own
    // `phpstan.neon` nor this project's own corpus config does).
    // `check_method_overrides`'s own return-type check is the only
    // reader - every other consumer of a function's return type wants
    // the docblock-refined `return_type` above instead.
    pub return_type_native: Type,
    // Phase 87 (PHPDoc-inheritance backfill): the `ParamSig::has_own_doc`
    // equivalent for the return position - starts `true` only when this
    // declaration's own docblock carried an `@return` tag (`from_decl`'s
    // own `doc_return` was `Some`), mirroring real PHPStan's
    // `ResolvedPhpDocBlock::mergeOneParentReturnTag`'s own trigger
    // (`$returnTag !== null`). See `ParamSig::has_own_doc`'s own doc
    // comment for why `flatten_phpdoc_inheritance` also sets this to
    // `true` once it backfills `return_type` from an ancestor.
    pub return_has_own_doc: bool,
    // Phase 88: the `ParamSig::explicit_mixed_hint` equivalent for the
    // return position - see that field's own doc comment.
    pub return_explicit_mixed_hint: bool,
    // Phase 37: PHP 8.5's `#[\NoDiscard]` marker attribute - `true` when
    // this declaration carries one, meaning a bare, unused call to it
    // (`foo();` on its own line, return value neither assigned nor
    // otherwise consumed) is a real diagnostic
    // (`Checker::check_no_discard_statement`), matching real PHPStan's own
    // `NativeFunctionReflection::mustUseReturnValue`/
    // `NativeMethodReflection`'s identical convention: case-insensitive
    // match on the attribute's own short name (no namespace resolution,
    // since real PHP's own `#[NoDiscard]` is always the single, global-
    // namespace one - there's no other class this could plausibly mean).
    pub no_discard: bool,
    // Phase 57: `public`/`private`/`protected` - `Visibility::Public` for
    // every plain top-level function (PHP has no way to declare one
    // otherwise), a method's own declared visibility otherwise. Threaded
    // through so a call site can enforce access legality the same way
    // `PropertySig::visibility` does for properties.
    pub visibility: Visibility,
    // Phase 67: `@internal` on this function/method's own doc comment -
    // see `docblock::DocBlock::internal`'s own doc comment for what it
    // means and how it's gated. A method also counts as internal when
    // its *declaring class* does, but that half is checked separately
    // (against `ClassInfo::is_internal`) by whichever call site already
    // has the declaring class in hand - this field only ever reflects
    // the member's own tag.
    pub is_internal: bool,
    // Phase 40: `@api` on this function/method's own doc comment -
    // real PHPStan's `ApiMethodCallRule`/`ApiInstantiationRule` check
    // both the declaring class's own `@api` AND the member's own -
    // `ClassInfo::is_api` only ever covered the former. `DocBlock::api`
    // already exists and is parsed; this is a direct reuse, not new
    // parsing.
    pub is_api: bool,
    // Phase 64 follow-up: PHP 8.3's `#[\Override]` marker - `true` when
    // this declaration carries one. Stored here (mirroring `no_discard`
    // exactly, including its "short-name-only, no namespace resolution"
    // matching convention) rather than checked straight off a fresh
    // `MethodDecl`'s own AST, because a method needs this checked in the
    // context of whichever *using class*'s ancestor chain applies - a
    // trait's own method carrying `#[Override]` is only ever a real
    // fatal error relative to a class that later `use`s it (confirmed
    // directly: the same trait method is fine when the using class has a
    // real ancestor to satisfy it, and fatals - attributed to the
    // *using class's* own method name, not the trait's - when it
    // doesn't), so this needs to survive `flatten_traits`' merge the
    // same way `no_discard` already does.
    pub has_override_attr: bool,
    // Phase 39: `@deprecated` on this function/method's own doc comment -
    // see `docblock::DocBlock::deprecated`'s own doc comment for what it
    // means and how it's gated. Deliberately scoped to the function/
    // method's own tag only, unlike `is_internal`'s "declaring class
    // also counts" fallback - whether a class-level `@deprecated`
    // implicitly deprecates every member too isn't something this
    // project could confirm against real PHPStan's own source (the
    // actual deprecation-detection logic lives in `ondrejmirtes/
    // better-reflection`, not vendored here), so it's left unclaimed
    // rather than guessed.
    pub deprecated: Option<String>,
    // Phase 42's own self-contained first slice: `@throws` on this
    // function/method's own doc comment, resolved and unioned across
    // every tag present (`docblock::DocBlock::throws`'s own doc comment
    // explains why it's a `Vec` there) - `None` when no `@throws` tag
    // exists at all, distinct from a real, resolvable-but-empty case
    // (there isn't one - a tag with no type text simply isn't recorded
    // by `docblock::parse` in the first place). Not yet consumed by any
    // checker code - the base model this phase's later stages (a real
    // body walk collecting `throw` statements, transitive union across
    // the call graph, the opt-in "missing checked exception" rule
    // family) build on.
    pub throws: Option<Type>,
    // `@phpstan-assert`-family tags (`docblock::DocBlock::assert_if_true`'s
    // own doc comment has the full picture), resolved into a real `Type`
    // plus a classified `AssertTarget` - a tag whose own target text
    // doesn't match a real parameter of this declaration (typo'd, or a
    // target shape this project doesn't classify at all, e.g. a method-
    // call or array-index target) is silently dropped rather than kept
    // half-resolved, the usual "don't guess" leniency.
    pub assert_if_true: Vec<AssertInfo>,
    pub assert_if_false: Vec<AssertInfo>,
    pub assert: Vec<AssertInfo>,
    // Phase 62 (premature-access dataflow): this declaration's own real
    // AST body, when it has one - `None` for every synthesized signature
    // this registry ever builds without a real, parseable declaration
    // behind it (a built-in enum method, a native stdlib stub with no
    // vendored source body, ...). Exists specifically so a same-class or
    // cross-file `parent::`/`self::`/`static::`/`$this->` call can be
    // *recursively walked into* from wherever it's actually declared -
    // the per-file `Checker` only ever sees the one file it's currently
    // checking, so without storing bodies here, resolving into an
    // ancestor class declared in a different file would be impossible.
    // `Arc`-wrapped rather than cloned per lookup: a real body can be
    // arbitrarily large, and `FunctionSig` itself is cloned routinely
    // (template substitution, trait flattening, override merging) by
    // code that never touches this field at all. `Arc`, not `Rc`, because
    // `php_stubs.rs`'s own generated-stub caches are `LazyLock<Vec<(String,
    // FunctionSig)>>` statics, which require `Send + Sync`.
    pub body: Option<Arc<Vec<Stmt>>>,
    // Phase 41/82 (transitive purity): `None` until `Registry::
    // resolve_transitive_purity` classifies this declaration's own body
    // (`registry::purity::classify_body`) - stays `None` forever for
    // every synthesized signature with no real body (`body` is also
    // `None`) and for one this project's classifier can never prove
    // effect-free no matter what its dependencies turn out to be (a
    // non-call impure point, an explicit throw, a by-ref parameter, an
    // `@phpstan-assert` tag, or an unresolvable call). `Some(vec![])` -
    // *not* the same as `None` - is the fixed point's own base case: no
    // dependencies at all, unconditionally effect-free already.
    pub purity_dependencies: Option<Vec<crate::registry::PurityDependency>>,
    // Set by the same fixed point once every one of `purity_dependencies`
    // is itself proven pure - `false` for everything until then,
    // including every declaration whose own `purity_dependencies` is
    // `None`. The one field this whole feature's own consuming
    // diagnostic (a bare, discarded call to a now-pure declaration)
    // actually reads.
    pub is_transitively_pure: bool,
    // Phase 41/82: `true` for every signature `php_stubs.rs` generates
    // from vendored `phpstorm-stubs`/`phpstan-src` source - a real,
    // necessary exclusion found via corpus verification, not assumed:
    // a stub file's own function/method body is *always* empty (`{}`) -
    // real PHP builtins like `header()`/`exit()`/`class_exists()` are
    // natively implemented, real side effects included, with nothing
    // resembling their actual behavior expressed in PHP at all. Without
    // this flag, `registry::purity::classify_body` would see an empty
    // body and wrongly conclude every one of PHP's own thousands of
    // built-in functions is provably pure - confirmed directly: the
    // very first full-corpus verification round flagged `header()` as
    // dead code. `false` for every real, first-party declaration
    // (`from_decl`'s own construction has no way to know it's *inside*
    // a stub file at parse time - `php_stubs.rs`'s own
    // `generate_from_source`/`generate_classes_from_source` set this
    // `true` on their own returned signatures afterward instead, the
    // one place that actually knows).
    pub is_stub: bool,
    // `true` for a signature with no real, textual declaration at all -
    // a magic `@method` tag (`class_info::method_tag_sig`) or one of
    // PHP's own built-in enum methods this project synthesizes directly
    // (`cases()`/`from()`/`tryFrom()`). `false` for every real
    // declaration, including one merged from two real overloads
    // (`merge`'s own `&&` - a real declaration on *either* side of a
    // merge makes the result real too). Found necessary via corpus
    // verification, not assumed: `Registry::required_abstract_methods`'s
    // own "an interface's own declared methods are all required" rule
    // used to scan an interface's `methods` map indiscriminately,
    // wrongly treating a purely magic `@method` tag on an interface
    // (real corpus shape: Tracy's own `IBarPanel`, `@method ?string
    // getAgentInfo()`) as something every implementor must also define a
    // real, textual method for - `@method` only ever documents a
    // magic-`__call`-backed capability, never a requirement.
    pub is_magic: bool,
}

/// A class constant's value is almost always a literal in real code - this
/// covers the common cases the same way `checker.rs`'s own literal
/// inference does, without pulling in the full expression-inference engine
/// (which lives in `checker.rs` and depends on `Registry` itself, so it
/// can't be called from here). Anything more complex than a bare literal
/// (an array, a concatenation, another constant) falls back to `Mixed` -
/// undefined-constant detection only needs the name to be known, not a
/// precise type.
pub(crate) fn infer_const_type(value: &crate::ast::Expr) -> Type {
    use crate::ast::Literal;
    match &value.kind {
        ExprKind::Literal(Literal::Int(n)) => Type::IntLiteral(*n),
        ExprKind::Literal(Literal::Float(_)) => Type::Float,
        ExprKind::Literal(Literal::String(s)) => Type::StringLiteral(s.clone()),
        ExprKind::Literal(Literal::Bool(true)) => Type::True,
        ExprKind::Literal(Literal::Bool(false)) => Type::False,
        ExprKind::Literal(Literal::Null) => Type::Null,
        // Phase 23: `const int OFF = -1;` parses as a literal `1` wrapped
        // in a unary negation, not a single negative-literal token - a
        // negative-valued typed constant (real shape: `Piwigo\Core\
        // Logger::OFF`, the one negative severity level alongside seven
        // non-negative ones) otherwise silently fell to `Mixed` below.
        ExprKind::Unary(crate::ast::UnaryOp::Neg, inner) => match &inner.kind {
            ExprKind::Literal(Literal::Int(n)) => Type::IntLiteral(-n),
            ExprKind::Literal(Literal::Float(_)) => Type::Float,
            _ => Type::Mixed,
        },
        // A round of Piwigo-corpus bucketing found this real, common
        // shape (`AppInfo::URL = 'https://' . self::DOMAIN;`) - a
        // const's own value referencing *another* const via `self::`
        // can't be resolved here at all (this function has no class
        // context, and no registry to look the other const up in even
        // if it did), but real PHP string concatenation always
        // produces a `string` regardless of either operand's own type,
        // so this is sound information even without resolving anything
        // - unlike falling back to `Mixed` below, which silently
        // widened every array literal/argument this const then fed
        // into.
        ExprKind::Binary(crate::ast::BinOp::Concat, ..) => Type::String,
        _ => Type::Mixed,
    }
}

/// Phase 37: whether an `Attribute`'s own raw (possibly namespaced) name
/// names the real PHP 8.5 `#[\NoDiscard]` marker - real PHPStan's own
/// `NativeFunctionReflection::mustUseReturnValue` matches by short name
/// only, case-insensitively (`strtolower($attrib->getName()) ===
/// 'nodiscard'`), not a fully-resolved, namespace-aware class reference -
/// mirrored exactly here rather than resolving `name` against `use`
/// imports the way an ordinary attribute-class reference would, since
/// real PHP's own attribute is always the single global-namespace one and
/// there's no other plausible class this could mean.
pub(crate) fn is_no_discard_attribute_name(name: &str) -> bool {
    name.trim_start_matches('\\')
        .rsplit('\\')
        .next()
        .unwrap_or(name)
        .eq_ignore_ascii_case("NoDiscard")
}

/// Phase 64 follow-up: same idea as `is_no_discard_attribute_name` just
/// above, for PHP 8.3's `#[\Override]` marker - real PHPStan's own
/// `OverridingMethodRule::hasOverrideAttribute` matches the identical way
/// (`$attr->name->toLowerString() === 'override'` - short-name-only,
/// case-insensitive, no fully-resolved namespace check), deliberately
/// *not* the stricter engine-level namespace resolution the real PHP
/// runtime itself performs for this same attribute (confirmed directly:
/// a userland `App\Override` class doesn't trigger the real fatal error
/// at all) - this project follows real PHPStan's own rule here, not the
/// engine.
pub(crate) fn is_override_attribute_name(name: &str) -> bool {
    name.trim_start_matches('\\')
        .rsplit('\\')
        .next()
        .unwrap_or(name)
        .eq_ignore_ascii_case("Override")
}

impl FunctionSig {
    /// `class_templates` is the enclosing class's own `@template` metadata
    /// (empty for a plain function, or a method on a non-generic class) -
    /// merged in alongside the function/method's own, so a method body
    /// referencing a template its *class* declares (not itself) still
    /// gets it correctly recognized as `TemplateParam` rather than an
    /// ordinary (nonexistent) class name. This function/method's own
    /// same-named template (rare, but not invalid PHP) wins on a
    /// collision, matching a nested scope shadowing an outer one.
    // `class_type_aliases` is this method's own declaring class's
    // `@phpstan-type` table (`ClassInfo::from_decl`'s own `build_type_
    // aliases`) - empty for a plain top-level function or an enum's own
    // synthesized method (neither has a class-level docblock these could
    // meaningfully come from). A method's *own* docblock declaring
    // further `@phpstan-type`s of its own isn't supported - real
    // PHPStan allows this too, but every real corpus shape found so far
    // (`json-schema`'s own `ErrorBag`) declares aliases once, on the
    // class, referenced from many members - not attempted here.
    pub(crate) fn from_decl(
        f: &FunctionDecl,
        ctx: &NameContext,
        class_templates: &HashMap<String, TemplateParam>,
        class_type_aliases: &HashMap<String, Type>,
        visibility: Visibility,
    ) -> FunctionSig {
        let doc: Option<DocBlock> = f.doc_comment.as_deref().map(docblock::parse);
        let mut template_params: HashMap<String, TemplateParam> = doc
            .as_ref()
            .map(|d| build_template_params(d, &f.name, ctx))
            .unwrap_or_default();
        for (name, tp) in class_templates {
            template_params
                .entry(name.clone())
                .or_insert_with(|| tp.clone());
        }

        let params = f
            .params
            .iter()
            .map(|p| {
                // Phase 93: a promoted constructor property very commonly
                // carries its own inline `/** @var T */` directly above
                // it (real idiom - it doubles as that property's own
                // docblock), rather than an `@param $name` tag on the
                // enclosing `__construct()`'s own docblock. Only
                // consulted when the outer method-level tag is absent -
                // an explicit `@param` naming this exact parameter always
                // wins, the same "more specific declaration wins" rule
                // every other doc-source precedence in this project
                // already follows.
                let doc_ty: Option<String> = doc
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
                let ty = resolve_names_in_type(ty, ctx);
                let ty = substitute_type_aliases(ty, class_type_aliases);
                let ty = apply_implicit_nullable_default(ty, p.default.as_ref());
                // Mirrors `ty` just above exactly, but with `doc_ty`
                // forced to `None` - the same "native signature alone,
                // no docblock refinement" split `return_type_native`
                // already has. Still passes through `apply_implicit_
                // nullable_default`, unlike `return_type_native` (which
                // has no equivalent default-value-implies-nullable rule
                // to apply) - that transformation reflects a genuinely
                // native-level PHP fact (an untyped-`?` native hint with
                // a `null` default is implicitly nullable), independent
                // of any docblock merge.
                let ty_native = resolve_type(None, p.type_hint.as_deref());
                let ty_native = mark_template_params(ty_native, &template_params);
                let ty_native = resolve_names_in_type(ty_native, ctx);
                let ty_native = apply_implicit_nullable_default(ty_native, p.default.as_ref());
                // A real `= value` default always wins; failing that, a
                // `[optional]` marker on this param's own `@param` tag is
                // trusted too (see `DocBlock::optional_params` - phpstorm-
                // stubs models several functions, e.g. `mt_rand`/
                // `array_keys`, only as their highest-arity overload, with
                // trailing params marked optional in the docblock but
                // carrying no native default at all).
                let has_default = p.default.is_some()
                    || doc
                        .as_ref()
                        .is_some_and(|d| d.optional_params.contains(&p.name));
                // Phase 36: `@param-closure-this Type $name` - resolved
                // through the identical mark-then-resolve-then-substitute
                // pipeline `ty` above already uses for the same reason
                // (a generic class/function's own `@template` name, or a
                // local `@phpstan-type` alias, can appear here too).
                let closure_this_type = doc
                    .as_ref()
                    .and_then(|d| d.param_closure_this.get(&p.name))
                    .map(|raw| {
                        let ty = types::parse_type_string(raw);
                        let ty = mark_template_params(ty, &template_params);
                        let ty = resolve_names_in_type(ty, ctx);
                        substitute_type_aliases(ty, class_type_aliases)
                    });
                ParamSig {
                    name: p.name.clone(),
                    ty,
                    ty_native,
                    variadic: p.variadic,
                    has_default,
                    by_ref: p.by_ref,
                    has_own_doc: doc_ty.is_some(),
                    explicit_mixed_hint: has_explicit_mixed_nested_somewhere(
                        p.type_hint.as_deref(),
                        doc_ty.as_deref(),
                    ),
                    closure_this_type,
                }
            })
            .collect();
        let params = dedup_versioned_params(params);

        let doc_return = doc.as_ref().and_then(|d| d.return_type.as_deref());
        let return_type = resolve_type(doc_return, f.return_type.as_deref());
        let return_type = mark_template_params(return_type, &template_params);
        let return_type = resolve_names_in_type(return_type, ctx);
        let return_type = substitute_type_aliases(return_type, class_type_aliases);

        let return_type_native = resolve_type(None, f.return_type.as_deref());
        let return_type_native = mark_template_params(return_type_native, &template_params);
        let return_type_native = resolve_names_in_type(return_type_native, ctx);

        let no_discard = f
            .attrs
            .iter()
            .any(|a| is_no_discard_attribute_name(&a.name));
        let has_override_attr = f.attrs.iter().any(|a| is_override_attribute_name(&a.name));

        let is_internal = doc.as_ref().is_some_and(|d| d.internal);
        let is_api = doc.as_ref().is_some_and(|d| d.api);
        let deprecated = doc.as_ref().and_then(|d| d.deprecated.clone());

        let throws =
            doc.as_ref()
                .into_iter()
                .flat_map(|d| &d.throws)
                .fold(None::<Type>, |acc, raw| {
                    let ty = types::parse_type_string(raw);
                    let ty = mark_template_params(ty, &template_params);
                    let ty = resolve_names_in_type(ty, ctx);
                    Some(match acc {
                        Some(prev) => union_types(prev, ty),
                        None => ty,
                    })
                });

        let param_names: Vec<&str> = f.params.iter().map(|p| p.name.as_str()).collect();
        let (assert_if_true, assert_if_false, assert) = doc
            .as_ref()
            .map(|d| {
                (
                    resolve_assert_tags(&d.assert_if_true, &param_names, &template_params, ctx),
                    resolve_assert_tags(&d.assert_if_false, &param_names, &template_params, ctx),
                    resolve_assert_tags(&d.assert, &param_names, &template_params, ctx),
                )
            })
            .unwrap_or_default();

        FunctionSig {
            params,
            return_type,
            return_type_native,
            return_has_own_doc: doc_return.is_some(),
            return_explicit_mixed_hint: has_explicit_mixed_nested_somewhere(
                f.return_type.as_deref(),
                doc_return,
            ),
            no_discard,
            visibility,
            is_internal,
            is_api,
            has_override_attr,
            deprecated,
            throws,
            assert_if_true,
            assert_if_false,
            assert,
            body: Some(Arc::new(f.body.clone())),
            purity_dependencies: None,
            is_transitively_pure: false,
            is_stub: false,
            is_magic: false,
        }
    }

    /// Phase 32 Finding C: merges two *unconditional* declarations of the
    /// same function name - the real-overload shape phpstorm-stubs uses
    /// for `strtr` (a 3-string-argument form and a 2-argument, `array
    /// $replace_pairs` form, as two separate top-level declarations
    /// sharing one name), mirroring `ClassInfo::merge`'s own "later
    /// declaration wins on conflict" convention.
    pub(crate) fn merge(self, other: FunctionSig) -> FunctionSig {
        let return_type = union_types(self.return_type, other.return_type);
        let return_type_native = union_types(self.return_type_native, other.return_type_native);
        // Permissive, same direction as `variadic`/`has_default` below:
        // if either real overload actually carries the marker, the merged
        // signature should too - a caller ignoring the return value must
        // not silently escape the obligation just because *one* overload
        // (of what's really the same underlying declaration, PHP-version-
        // split) happened not to repeat the attribute.
        let no_discard = self.no_discard || other.no_discard;
        let has_override_attr = self.has_override_attr || other.has_override_attr;
        let len = self.params.len().max(other.params.len());
        let mut self_params = self.params.into_iter();
        let mut other_params = other.params.into_iter();
        let mut params = Vec::with_capacity(len);
        for _ in 0..len {
            let a = self_params.next();
            let b = other_params.next();
            let merged = match (a, b) {
                (Some(a), Some(b)) => ParamSig {
                    // Later declaration wins on conflict, matching
                    // `ClassInfo::merge`'s own convention.
                    name: b.name,
                    ty: union_types(a.ty, b.ty),
                    ty_native: union_types(a.ty_native, b.ty_native),
                    // Permissive: accepting a variadic-style tail call
                    // when *either* real overload allows it costs nothing
                    // a stricter rule would have caught.
                    variadic: a.variadic || b.variadic,
                    // A position required by only one real overload can't
                    // be required in a callable meant to accept either.
                    has_default: a.has_default || b.has_default,
                    // Unlike the above, NOT permissive: `by_ref` is a
                    // restriction placed on the *caller* (an lvalue is
                    // required), not something being accepted - OR-ing it
                    // would wrongly flag a call matching the non-by-ref
                    // overload's own shape just because the *other*
                    // overload needs a reference there. AND keeps the
                    // merged signature only as strict as every real
                    // overload agrees it should be.
                    by_ref: a.by_ref && b.by_ref,
                    // Permissive, same direction as `has_default` above:
                    // if either real overload's own docblock already
                    // documents this position, the merged signature
                    // shouldn't look like a further-inheritance candidate.
                    has_own_doc: a.has_own_doc || b.has_own_doc,
                    // Same permissive either-overload-wins rule as
                    // `has_own_doc` just above, for the identical reason.
                    explicit_mixed_hint: a.explicit_mixed_hint || b.explicit_mixed_hint,
                    // Later declaration wins on conflict, matching `name`
                    // above - falls back to the earlier one when only it
                    // declared the tag.
                    closure_this_type: b.closure_this_type.or(a.closure_this_type),
                },
                // A position present in only one real overload can't be
                // required in the merged callable either.
                (Some(mut only), None) | (None, Some(mut only)) => {
                    only.has_default = true;
                    only
                }
                (None, None) => unreachable!("loop bound is the longer side's own length"),
            };
            params.push(merged);
        }
        FunctionSig {
            params,
            return_type,
            return_type_native,
            // Same permissive "either overload's own marker wins" as
            // `no_discard`/`is_internal` below.
            return_has_own_doc: self.return_has_own_doc || other.return_has_own_doc,
            return_explicit_mixed_hint: self.return_explicit_mixed_hint
                || other.return_explicit_mixed_hint,
            no_discard,
            // Both real overloads share one declaration (a stub-modeled
            // PHP-version split of the same underlying function/method,
            // never two independently-declared visibilities) - either
            // side's own value is equally correct.
            visibility: self.visibility,
            // Same permissive "either overload's own marker wins" as
            // `no_discard` just above.
            is_internal: self.is_internal || other.is_internal,
            // Same permissive "either overload's own marker wins" as
            // `no_discard`/`is_internal` above.
            is_api: self.is_api || other.is_api,
            has_override_attr,
            // Same permissive "either overload's own marker wins" as
            // `no_discard`/`is_internal` above.
            deprecated: self.deprecated.or(other.deprecated),
            // Both real overloads can genuinely throw whatever either
            // one's own `@throws` documents - a caller has to be
            // prepared for either, whichever one actually runs.
            throws: match (self.throws, other.throws) {
                (Some(a), Some(b)) => Some(union_types(a, b)),
                (Some(t), None) | (None, Some(t)) => Some(t),
                (None, None) => None,
            },
            // Both real overloads' own assertions still hold regardless
            // of which one actually ran - simple concatenation, not a
            // type union, since each entry is an independent claim about
            // a target, not a competing type for the same one.
            assert_if_true: [self.assert_if_true, other.assert_if_true].concat(),
            assert_if_false: [self.assert_if_false, other.assert_if_false].concat(),
            assert: [self.assert, other.assert].concat(),
            // Same permissive "either overload's own body wins" as
            // `deprecated` above - both real overloads are stub-modeled
            // splits of the same underlying declaration, so either one's
            // own body is an equally valid absorption target; `self`'s
            // own (if it has one) is preferred only for determinism.
            body: self.body.or(other.body),
            // Merging happens during indexing/stub-merging, always
            // before `Registry::resolve_transitive_purity` ever runs -
            // both sides are always `None`/`false` at this point, same
            // as any other not-yet-classified signature.
            purity_dependencies: None,
            is_transitively_pure: false,
            // `||`, not `&&`: a real corpus find (Piwigo's own vendored
            // *copy* of `jetbrains/phpstorm-stubs`, analyzed as ordinary
            // project source rather than excluded like this project's
            // own scoped config does - the unscoped, whole-`vendor/`
            // corpus check specifically) - merging a stub-derived
            // signature (this project's own internal `header()` stub)
            // with a *second*, independently-parsed declaration of the
            // same name (Piwigo's own vendored stub file, itself
            // *also* just as fake/bodyless) must stay excluded from
            // purity classification either way, since neither body is a
            // real implementation - hardcoding `false` here (an earlier
            // version of this fix) silently un-marked exactly this
            // shape and reintroduced the original `header()`/`exit()`
            // false-positive bug through a second path.
            is_stub: self.is_stub || other.is_stub,
            // `&&`, not `||`: a real declaration on *either* side of a
            // merge (this function's own doc comment - both sides are
            // always real, textual declarations in practice) makes the
            // merged result real too, the same "real declaration always
            // wins" precedent `@method`/`@property` synthesis already
            // hold elsewhere.
            is_magic: self.is_magic && other.is_magic,
        }
    }
}

/// phpstorm-stubs represents "this parameter's type/default changed
/// across PHP versions" by declaring the *same conceptual parameter
/// twice*, once per era, each gated by its own
/// `#[PhpStormStubsElementAvailable(from: X, to: Y)]` attribute (real
/// shape: `pg_query`'s `$connection`, `fsockopen`'s `$port`,
/// `SQLite3::open`'s `$flags`/`$encryptionKey`) - but real PHP cannot
/// declare two parameters with the same name in one function (a fatal
/// error), so any duplicate name reaching here is unconditionally this
/// versioning artifact, never a legitimate shape, regardless of which
/// attribute produced it or whether it has a default. The lexer's own
/// `skip_stub_availability_attribute` heuristic already drops some of
/// these (a closed-range duplicate with no default), but keeps one that
/// does have a default - correct for a different real shape
/// (`imagefilter`'s uniquely-named `$arg1`..`$arg4` before a
/// differently-named variadic) but wrong when the kept duplicate's name
/// is reused by a later parameter, producing a function with one extra
/// position and every real argument after it binding to the wrong
/// parameter. Collapsing here, after the lexer, needs no knowledge of
/// that versioning idiom at all: keep each name's *last* occurrence
/// (phpstorm-stubs always writes the legacy era first, the modern one
/// last), at the *position* of its *first* occurrence, so later
/// uniquely-named parameters aren't shifted.
fn dedup_versioned_params(params: Vec<ParamSig>) -> Vec<ParamSig> {
    let mut order: Vec<String> = Vec::new();
    let mut by_name: HashMap<String, ParamSig> = HashMap::new();
    for p in params {
        if !by_name.contains_key(&p.name) {
            order.push(p.name.clone());
        }
        by_name.insert(p.name.clone(), p);
    }
    order
        .into_iter()
        .map(|name| by_name.remove(&name).expect("just inserted for this name"))
        .collect()
}

/// Renders `sig` as a real PHP-looking signature (`strlen(string $string):
/// int<0, max>`) - shared between the `demo` subcommand's own output and
/// the LSP's hover text (Phase 17), so the two never drift apart.
pub fn format_signature(name: &str, sig: &FunctionSig) -> String {
    let params: Vec<String> = sig
        .params
        .iter()
        .map(|p| {
            let dots = if p.variadic { "..." } else { "" };
            format!("{} {dots}${}", p.ty, p.name)
        })
        .collect();
    format!("{name}({}): {}", params.join(", "), sig.return_type)
}
