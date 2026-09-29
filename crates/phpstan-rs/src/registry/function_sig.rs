use crate::registry::reflection_attribute::ReflectionAttribute;
use rustc_hash::FxHashMap;
use std::sync::Arc;

use crate::registry::name_context::{collect_class_name_case_refs, resolve_names_in_type};
use crate::registry::type_node::{mentions_mixed, parse_type_node};
use phpdoc::docblock::{self, DocBlock};
use phpdoc::grammar::TypeNode;
use syntax::ast::TypeHint;
use syntax::ast::{ExprKind, FunctionDecl, StaticTarget, Visibility};
use syntax::name_resolution::NameContext;
use types::has_explicit_mixed_nested_somewhere;

use types::{self, Shape, TemplateParam, Type};

mod asserts;
mod generics;
mod type_resolution;

pub(crate) use asserts::resolve_assert_tags;
pub use asserts::{AssertInfo, AssertTarget};
pub(crate) use generics::{
    build_template_params, build_type_aliases, mark_template_params, substitute_type_aliases,
};
pub(crate) use type_resolution::{
    apply_implicit_nullable_default, resolve_type, resolve_type_with_parsed_native,
};

#[derive(Debug, Clone)]
pub struct ParameterReflection {
    pub(in crate::registry) name: Arc<str>,
    pub(in crate::registry) ty: Type,
    // The native, no-docblock-merge type (see `return_type_native`'s
    // doc comment for the same split on the return side).
    // `check_one_method_override`'s param-narrowing check must compare
    // this, not the merged `ty` - otherwise narrowing only via `@param`
    // while leaving a wide native hint (the common
    // Rector/PHPStan-`Collector<T>` idiom) would be wrongly flagged as
    // an LSP violation real PHPStan doesn't check by default.
    pub(in crate::registry) ty_native: Type,
    pub(in crate::registry) variadic: bool,
    pub(in crate::registry) has_default: bool,
    // The default value's own inferred type (`infer_const_type` - a
    // literal, or `Mixed` for anything it can't read), when there is one.
    // Lets a conditional return type resolve against an omitted
    // argument's default the way real PHPStan does
    // (`sendMailNotifications()` with `$action = 'list_to_send'` picks the
    // `$action is 'send'` false branch).
    pub(in crate::registry) default_ty: Option<Type>,
    // This declaration's own `@param` tag type, resolved but not
    // reconciled with the native hint - what real PHPStan's
    // `IncompatiblePhpDocTypeRule` compares against that hint. `None`
    // without an own tag.
    pub(in crate::registry) doc_ty: Option<Type>,
    pub(in crate::registry) by_ref: bool,
    // True when this parameter's own `@param` tag was present on this
    // declaration's docblock; `false` marks it eligible for
    // `Registry::flatten_phpdoc_inheritance` to backfill from an
    // ancestor, which then sets this `true` too - what makes
    // multi-level inheritance resolve one level per fixed-point pass.
    pub(in crate::registry) has_own_doc: bool,
    // Whether this declaration's own native hint/`@param` text spells
    // out `mixed` explicitly - needed so `checker::missing_typehint`
    // still recognizes a backfilled (`flatten_phpdoc_inheritance`)
    // `mixed` as deliberate, not undocumented; that pass copies this
    // field alongside `ty`.
    pub(in crate::registry) explicit_mixed_hint: bool,
    // Every class-name reference `ty` carries, as (raw, resolved-FQN)
    // pairs, collected before `resolve_names_in_type` destroys the
    // original spelling. Empty for a backfilled position - the raw text
    // belongs to an ancestor file's own `use` imports, not this one's
    // `name_ctx`.
    pub(in crate::registry) case_refs: Vec<(Arc<str>, Arc<str>)>,
    // `@param-closure-this Type $name`'s resolved `Type` - what `$this`
    // becomes inside a `Closure`/`ArrowFn` argument at this position.
    // `None` when untagged.
    pub(in crate::registry) closure_this_type: Option<Type>,
    // `@param-out Type $name`'s resolved `Type` - the widened type a
    // by-ref parameter is guaranteed to hold on return. `None` falls
    // back to the native/`@param` type, matching real PHPStan's
    // `getOutType() ?? getType()`.
    pub(in crate::registry) out_ty: Option<Type>,
    pub(in crate::registry) attributes: Vec<ReflectionAttribute>,
}

impl ParameterReflection {
    /// This parameter with `f` applied to its type, native type and out
    /// type.
    pub fn map_types(&self, f: impl Fn(&Type) -> Type) -> ParameterReflection {
        ParameterReflection {
            ty: f(&self.ty),
            ty_native: f(&self.ty_native),
            out_ty: self.out_ty.as_ref().map(&f),
            ..self.clone()
        }
    }

    pub fn name(&self) -> &Arc<str> {
        &self.name
    }

    pub fn ty(&self) -> &Type {
        &self.ty
    }

    pub fn ty_native(&self) -> &Type {
        &self.ty_native
    }

    pub fn variadic(&self) -> bool {
        self.variadic
    }

    pub fn has_default(&self) -> bool {
        self.has_default
    }

    pub fn default_ty(&self) -> &Option<Type> {
        &self.default_ty
    }

    pub fn doc_ty(&self) -> &Option<Type> {
        &self.doc_ty
    }

    pub fn by_ref(&self) -> bool {
        self.by_ref
    }

    pub fn has_own_doc(&self) -> bool {
        self.has_own_doc
    }

    pub fn explicit_mixed_hint(&self) -> bool {
        self.explicit_mixed_hint
    }

    pub fn case_refs(&self) -> &Vec<(Arc<str>, Arc<str>)> {
        &self.case_refs
    }

    pub fn closure_this_type(&self) -> &Option<Type> {
        &self.closure_this_type
    }

    pub fn out_ty(&self) -> &Option<Type> {
        &self.out_ty
    }

    pub fn attributes(&self) -> &Vec<ReflectionAttribute> {
        &self.attributes
    }
}

#[derive(Debug, Clone)]
pub struct FunctionReflection {
    pub(in crate::registry) params: Vec<ParameterReflection>,
    pub(in crate::registry) return_type: Type,
    // The native-only return type, ignoring `@return` -
    // `check_method_overrides`'s return-type check is the only reader;
    // everything else wants the docblock-refined `return_type` above.
    pub(in crate::registry) return_type_native: Type,
    // `ParameterReflection::has_own_doc`'s return-position equivalent.
    pub(in crate::registry) return_has_own_doc: bool,
    // `ParameterReflection::doc_ty`'s return-position equivalent.
    pub(in crate::registry) return_doc_ty: Option<Type>,
    // `@phpstan-self-out Type`: the receiver's type after a call to this
    // method. `None` without the tag.
    pub(in crate::registry) self_out: Option<Type>,
    // `ParameterReflection::case_refs`'s return-position equivalent.
    pub(in crate::registry) return_case_refs: Vec<(Arc<str>, Arc<str>)>,
    // `ParameterReflection::explicit_mixed_hint`'s return-position equivalent.
    pub(in crate::registry) return_explicit_mixed_hint: bool,
    // PHP 8.5's `#[\NoDiscard]` marker - `true` when a bare, unused
    // call to this declaration should be flagged
    // (`Checker::check_no_discard_statement`), matching real PHPStan's
    // short-name, case-insensitive match convention.
    pub(in crate::registry) no_discard: bool,
    // `MustUseResult` (php-language-extensions): `no_discard` while that
    // extension is active.
    pub(in crate::registry) must_use_result: bool,
    // `Visibility::Public` for a top-level function; a method's
    // declared visibility otherwise.
    pub(in crate::registry) visibility: Visibility,
    // `@internal` on this declaration's own doc comment (see
    // `docblock::DocBlock::internal`) - a method also counts as internal
    // via its declaring class, checked separately by the caller.
    pub(in crate::registry) is_internal: bool,
    // `@api` on this declaration's own doc comment - real PHPStan
    // checks both the declaring class's and the member's own tag;
    // `ClassReflection::is_api` only covers the former.
    pub(in crate::registry) is_api: bool,
    // PHP 8.3's `#[\Override]` marker. Stored rather than checked
    // straight off the AST, since a trait method's `#[Override]` is
    // only a real error relative to the *using* class's ancestor chain
    // - must survive `flatten_traits`' merge like `no_discard`.
    pub(in crate::registry) has_override_attr: bool,
    // `@deprecated` on this declaration's own tag only, unlike
    // `is_internal`'s "declaring class also counts" fallback - whether a
    // class-level `@deprecated` implicitly covers members isn't
    // confirmed against real PHPStan's source, so left unclaimed.
    pub(in crate::registry) deprecated: Option<String>,
    // `@throws` on this declaration, resolved and unioned across every
    // tag present. `None` when no `@throws` tag exists at all.
    pub(in crate::registry) throws: Option<Type>,
    // `@phpstan-assert`-family tags, resolved into a `Type` plus a
    // classified `AssertTarget` - a tag whose target doesn't match a
    // real parameter (typo'd, or unclassifiable) is dropped rather than
    // kept half-resolved.
    pub(in crate::registry) assert_if_true: Vec<AssertInfo>,
    pub(in crate::registry) assert_if_false: Vec<AssertInfo>,
    pub(in crate::registry) assert: Vec<AssertInfo>,
    // `None` until `Registry::resolve_transitive_purity` classifies
    // this body; stays `None` for a bodyless signature or one the
    // classifier can never prove effect-free. `Some(vec![])` is the
    // fixed point's base case: no dependencies, unconditionally
    // effect-free.
    pub(in crate::registry) purity_dependencies: Option<Vec<crate::registry::PurityDependency>>,
    // Set once every `purity_dependencies` entry is itself proven
    // pure. The field the bare-discarded-call-to-a-pure-declaration
    // diagnostic actually reads.
    pub(in crate::registry) is_transitively_pure: bool,
    // `true` for every signature `registry::stubs` generates - a stub
    // body is always empty (`{}`), so without this,
    // `registry::purity::classify_body` would wrongly call PHP builtins
    // like `header()` provably pure. `false` for a real, first-party
    // declaration; `registry::stubs` sets this on its own generated
    // signatures, the one place that knows.
    pub(in crate::registry) is_stub: bool,
    // `true` for a signature with no real textual declaration at all -
    // a magic `@method` tag or a synthesized enum method
    // (`cases()`/`from()`/`tryFrom()`). Needed so
    // `Registry::required_abstract_methods` doesn't require every
    // implementor define a real method for a purely magic `@method` tag
    // (real shape: Tracy's own `IBarPanel`, `@method ?string
    // getAgentInfo()`) - `@method` only ever documents a
    // magic-`__call`-backed capability, never a requirement.
    pub(in crate::registry) is_magic: bool,
    // This declaration's own attributes, names resolved.
    pub(in crate::registry) attributes: Vec<ReflectionAttribute>,
    pub(in crate::registry) provenance: crate::registry::provenance::Provenance,
}

impl FunctionReflection {
    /// This signature with `f` applied to every type it declares: its
    /// parameters', return, `@phpstan-self-out`, `@throws` and assertion
    /// types.
    pub fn map_types(&self, f: impl Fn(&Type) -> Type) -> FunctionReflection {
        let asserts = |infos: &[AssertInfo]| -> Vec<AssertInfo> {
            infos
                .iter()
                .map(|info| AssertInfo {
                    ty: f(&info.ty),
                    ..info.clone()
                })
                .collect()
        };
        FunctionReflection {
            params: self.params.iter().map(|p| p.map_types(&f)).collect(),
            return_type: f(&self.return_type),
            return_type_native: f(&self.return_type_native),
            self_out: self.self_out.as_ref().map(&f),
            throws: self.throws.as_ref().map(&f),
            assert_if_true: asserts(&self.assert_if_true),
            assert_if_false: asserts(&self.assert_if_false),
            assert: asserts(&self.assert),
            ..self.clone()
        }
    }

    /// This signature returning `ty` instead.
    pub fn with_return_type(&self, ty: Type) -> FunctionReflection {
        FunctionReflection {
            return_type: ty,
            ..self.clone()
        }
    }

    pub fn params(&self) -> &Vec<ParameterReflection> {
        &self.params
    }

    pub fn return_type(&self) -> &Type {
        &self.return_type
    }

    pub fn return_type_native(&self) -> &Type {
        &self.return_type_native
    }

    pub fn return_has_own_doc(&self) -> bool {
        self.return_has_own_doc
    }

    pub fn return_doc_ty(&self) -> &Option<Type> {
        &self.return_doc_ty
    }

    pub fn self_out(&self) -> &Option<Type> {
        &self.self_out
    }

    pub fn return_case_refs(&self) -> &Vec<(Arc<str>, Arc<str>)> {
        &self.return_case_refs
    }

    pub fn return_explicit_mixed_hint(&self) -> bool {
        self.return_explicit_mixed_hint
    }

    pub fn no_discard(&self) -> bool {
        self.no_discard
    }

    pub fn must_use_result(&self) -> bool {
        self.must_use_result
    }

    pub fn visibility(&self) -> Visibility {
        self.visibility
    }

    pub fn is_internal(&self) -> bool {
        self.is_internal
    }

    pub fn is_api(&self) -> bool {
        self.is_api
    }

    pub fn has_override_attr(&self) -> bool {
        self.has_override_attr
    }

    pub fn deprecated(&self) -> &Option<String> {
        &self.deprecated
    }

    pub fn throws(&self) -> &Option<Type> {
        &self.throws
    }

    pub fn assert_if_true(&self) -> &Vec<AssertInfo> {
        &self.assert_if_true
    }

    pub fn assert_if_false(&self) -> &Vec<AssertInfo> {
        &self.assert_if_false
    }

    pub fn assert(&self) -> &Vec<AssertInfo> {
        &self.assert
    }

    pub fn purity_dependencies(&self) -> &Option<Vec<crate::registry::PurityDependency>> {
        &self.purity_dependencies
    }

    pub fn is_transitively_pure(&self) -> bool {
        self.is_transitively_pure
    }

    pub fn is_stub(&self) -> bool {
        self.is_stub
    }

    pub fn is_magic(&self) -> bool {
        self.is_magic
    }

    pub fn attributes(&self) -> &Vec<ReflectionAttribute> {
        &self.attributes
    }

    pub fn provenance(&self) -> &crate::registry::provenance::Provenance {
        &self.provenance
    }
}

/// A class constant's value is almost always a literal in real code - this
/// covers the common cases the same way `checker.rs`'s own literal
/// inference does, without pulling in the full expression-inference engine
/// (which lives in `checker.rs` and depends on `Registry` itself, so it
/// can't be called from here). Anything more complex than a bare literal
/// (an array, a concatenation, another constant) falls back to `Mixed` -
/// undefined-constant detection only needs the name to be known, not a
/// precise type.
pub(crate) fn infer_const_type(value: &syntax::ast::Expr) -> Type {
    infer_const_type_with(value, &|_, _| None)
}

/// `infer_const_type` for a *class* constant's own value, which may also
/// reference another constant (`self::A`, `Foo::BAR`): that reference
/// becomes a class constant reference (the same one a `Foo::BAR` PHPDoc
/// type parses to), resolved once every class's constants exist by
/// `Registry::resolve_deferred_value_of`'s constant pre-pass. Only class
/// constants go through that pass, so only they get this; a global
/// constant keeps plain `infer_const_type`.
pub(crate) fn infer_class_const_type(value: &syntax::ast::Expr) -> Type {
    infer_const_type_with(value, &|class, name| {
        Some(Type::class_constant(
            types::parse_type_string(class),
            name.to_string(),
        ))
    })
}

/// The shared body of `infer_const_type`/`infer_class_const_type`.
/// `class_const_ref` decides what a `Class::NAME` reference inside the
/// value becomes (`None` falls back to `Mixed`).
fn infer_const_type_with(
    value: &syntax::ast::Expr,
    class_const_ref: &dyn Fn(&str, &str) -> Option<Type>,
) -> Type {
    syntax::stack::guard(|| {
        use syntax::ast::Literal;
        match &value.kind {
            ExprKind::Literal(Literal::Int(n)) => Type::int_literal(*n),
            ExprKind::Literal(Literal::Float(_)) => Type::float(),
            ExprKind::Literal(Literal::String(s)) => Type::string_literal(s.to_string().into()),
            ExprKind::Literal(Literal::Bool(true)) => Type::bool_literal(true),
            ExprKind::Literal(Literal::Bool(false)) => Type::bool_literal(false),
            ExprKind::Literal(Literal::Null) => Type::null(),
            // `const int OFF = -1;` parses as a literal `1` wrapped
            // in a unary negation, not a single negative-literal token - a
            // negative-valued typed constant (real shape: `Piwigo\Core\
            // Logger::OFF`, the one negative severity level alongside seven
            // non-negative ones) would otherwise silently fall to `Mixed`
            // below.
            ExprKind::Unary(syntax::ast::UnaryOp::Neg, inner) => match &inner.kind {
                ExprKind::Literal(Literal::Int(n)) => Type::int_literal(-n),
                ExprKind::Literal(Literal::Float(_)) => Type::float(),
                _ => Type::mixed(),
            },
            // A real, common shape (`AppInfo::URL = 'https://' . self::DOMAIN;`)
            // - a const's own value referencing *another* const via `self::`
            // can't be resolved here at all (this function has no class
            // context, and no registry to look the other const up in even
            // if it did), but real PHP string concatenation always
            // produces a `string` regardless of either operand's own type,
            // so this is sound information even without resolving anything
            // - unlike falling back to `Mixed` below, which would silently
            // widen every array literal/argument this const then feeds
            // into.
            ExprKind::Binary(syntax::ast::BinOp::Concat, ..) => Type::string(),
            ExprKind::Grouping(inner) | ExprKind::ErrorSuppress(inner) => {
                infer_const_type_with(inner, class_const_ref)
            }
            ExprKind::ClassConstFetch(StaticTarget::Name(class, _), name) => {
                class_const_ref(class, name).unwrap_or(Type::mixed())
            }
            // A constant array literal is a real, exact array shape in
            // real PHPStan (`ConstantArrayType`): keys as written, or the
            // next auto-index; PHP canonicalizes a decimal-integer string
            // key to an int. A spread, or a key that isn't a literal, can't
            // be laid out as a shape, so the value degrades to a plain
            // `array` of the values' union.
            ExprKind::ArrayLiteral(items) => {
                let mut fields: Vec<(String, Type, bool)> = Vec::new();
                let mut next_index = types::NextArrayIndex::default();
                let mut exact = true;
                for item in items {
                    let value_ty = infer_const_type_with(&item.value, class_const_ref);
                    let key = if item.spread {
                        None
                    } else {
                        match item.key.as_ref().map(|k| &k.kind) {
                            // Appending past `PHP_INT_MAX` throws: the
                            // constant has no value.
                            None => match next_index.take() {
                                types::AppendKey::Key(n) => Some(n.to_string()),
                                types::AppendKey::Unknown => None,
                                types::AppendKey::Throws => return Type::never(),
                            },
                            Some(ExprKind::Literal(Literal::Int(n))) => Some(n.to_string()),
                            Some(ExprKind::Literal(Literal::String(s))) => Some(
                                types::canonical_int_key(&s.to_str_lossy())
                                    .map_or_else(|| s.to_string(), |n| n.to_string()),
                            ),
                            _ => None,
                        }
                    };
                    let Some(key) = key else {
                        exact = false;
                        fields.push((String::new(), value_ty, false));
                        continue;
                    };
                    if let Some(n) = types::canonical_int_key(&key) {
                        next_index.observe(n);
                    }
                    match fields.iter_mut().find(|(k, _, _)| *k == key) {
                        Some(existing) => existing.1 = value_ty,
                        None => fields.push((key, value_ty, false)),
                    }
                }
                if exact {
                    Type::array_shape(Shape::from_named(fields))
                } else {
                    let mut values: Vec<Type> = Vec::new();
                    for (_, t, _) in fields {
                        if !values.contains(&t) {
                            values.push(t);
                        }
                    }
                    let value = match values.len() {
                        0 => Type::mixed(),
                        1 => values.pop().expect("len checked"),
                        _ => Type::union_of(values),
                    };
                    Type::array(Type::unknown_array_key(), value)
                }
            }
            _ => Type::mixed(),
        }
    })
}

/// Whether an `Attribute`'s own raw (possibly namespaced) name
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

/// Same idea as `is_no_discard_attribute_name` just
/// above, for PHP 8.3's `#[\Override]` marker - real PHPStan's own
/// `OverridingMethodRule::hasOverrideAttribute` matches the identical way
/// (`$attr->name->toLowerString() === 'override'` - short-name-only,
/// case-insensitive, no fully-resolved namespace check), deliberately
/// *not* the stricter engine-level namespace resolution the real PHP
/// runtime itself performs for this same attribute (a userland
/// `App\Override` class doesn't trigger the real fatal error at all) -
/// this project follows real PHPStan's own rule here, not the engine.
pub(crate) fn is_override_attribute_name(name: &str) -> bool {
    name.trim_start_matches('\\')
        .rsplit('\\')
        .next()
        .unwrap_or(name)
        .eq_ignore_ascii_case("Override")
}

/// Same idea again, for PHP 8.4's native `#[\Deprecated]`
/// marker - real PHPStan's own `ClassAttributesRule`/`TraitAttributesRule`
/// both use `getAttributes('Deprecated')`, a bare short-name match against
/// the class-like node's *own* `reflectionClass`-level attributes (not the
/// general-purpose, fully-resolved `attribute.notFound`/`.target` engine
/// this project's `check_attributes`/`check_attribute_targets` already
/// run) - this is a real, unconditional PHP fatal error on a class/
/// interface/enum declaration (`#[\Deprecated] class
/// Foo {}` fatals "Cannot apply #[\Deprecated] to class Foo"), and a
/// PHP-8.5-gated one on a trait (silent until 8.5 support - a real
/// deprecation warning fires when the trait is used, not a fatal
/// error).
pub(crate) fn is_deprecated_attribute_name(name: &str) -> bool {
    name.trim_start_matches('\\')
        .rsplit('\\')
        .next()
        .unwrap_or(name)
        .eq_ignore_ascii_case("Deprecated")
}

/// Same idea again, for PHP 8.2's native
/// `#[\AllowDynamicProperties]` marker - real PHPStan's own
/// `ClassAttributesRule`/`TraitAttributesRule`, flagging it as pointless/
/// wrong on a `readonly class`, `enum`, `interface`, or (unconditionally)
/// `trait` - none of which can ever have a real dynamic property to
/// permit in the first place.
pub(crate) fn is_allow_dynamic_properties_attribute_name(name: &str) -> bool {
    name.trim_start_matches('\\')
        .rsplit('\\')
        .next()
        .unwrap_or(name)
        .eq_ignore_ascii_case("AllowDynamicProperties")
}

impl FunctionReflection {
    /// `class_templates` is the enclosing class's own `@template` metadata
    /// (empty for a plain function, or a method on a non-generic class) -
    /// merged in alongside the function/method's own, so a method body
    /// referencing a template its *class* declares (not itself) still
    /// gets it correctly recognized as `TemplateParam` rather than an
    /// ordinary (nonexistent) class name. This function/method's own
    /// same-named template (rare, but not invalid PHP) wins on a
    /// collision, matching a nested scope shadowing an outer one.
    // `class_type_aliases` is this method's own declaring class's
    // `@phpstan-type` table (`ClassReflection::from_decl`'s own `build_type_
    // aliases`) - empty for a plain top-level function or an enum's own
    // synthesized method (neither has a class-level docblock these could
    // meaningfully come from). A method's own docblock declaring further
    // `@phpstan-type`s of its own isn't supported - real code
    // overwhelmingly declares aliases once, on the class, referenced
    // from many members - not attempted here.
    #[hotpath::measure]
    pub(crate) fn from_decl(
        f: &FunctionDecl,
        ctx: &NameContext,
        class_templates: &FxHashMap<String, TemplateParam>,
        class_type_aliases: &FxHashMap<Arc<str>, Type>,
        visibility: Visibility,
    ) -> FunctionReflection {
        let doc: Option<DocBlock> = f.doc_comment.as_deref().map(docblock::parse);
        let mut template_params: FxHashMap<String, TemplateParam> = doc
            .as_ref()
            .map(|d| build_template_params(d, &f.name, ctx))
            .unwrap_or_default();
        for (name, tp) in class_templates {
            template_params
                .entry(name.clone())
                .or_insert_with(|| tp.clone());
        }

        let resolve_doc_type = |node: &TypeNode| {
            let ty = parse_type_node(node);
            let ty = mark_template_params(ty, &template_params);
            let ty = resolve_names_in_type(ty, ctx);
            substitute_type_aliases(ty, class_type_aliases)
        };
        let params = f
            .params
            .iter()
            .map(|p| {
                // A promoted constructor property's own inline
                // `/** @var T */` doubles as its docblock - only
                // consulted when the outer method-level `@param` tag is
                // absent.
                let doc_ty: Option<TypeNode> = doc
                    .as_ref()
                    .and_then(|d| d.params.get(p.name.as_ref()))
                    .cloned()
                    .or_else(|| {
                        p.doc_comment
                            .as_deref()
                            .map(docblock::parse)
                            .and_then(|d| d.var_type)
                    });
                // Parsed once, shared by both the doc-reconciled `ty`
                // pipeline below and the native-only `ty_native` one -
                // `resolve_type`/`resolve_type(None, ...)` used to each
                // independently re-parse this same hint string.
                let native_hint_text = p.type_hint.as_ref().map(TypeHint::to_string);
                let native_hint = native_hint_text.as_deref();
                let native_parsed = native_hint.map(types::parse_native_type_string);
                let doc_reconciled = resolve_type_with_parsed_native(
                    doc_ty.as_ref(),
                    native_hint,
                    native_parsed.as_ref(),
                    &template_params,
                );
                // Whenever the docblock contributed nothing beyond the
                // native hint (no doc tag at all, or `reconcile_with_
                // native` collapsed back to exactly the native type), the
                // `ty` and `ty_native` pipelines are computing the
                // identical value through `mark_template_params`/
                // `resolve_names_in_type` - both are pure, deterministic
                // functions, so reusing `ty`'s own result for `ty_native`
                // is exact, not an approximation.
                let shared = Some(&doc_reconciled) == native_parsed.as_ref();
                let ty = mark_template_params(doc_reconciled, &template_params);
                let mut case_refs = Vec::new();
                collect_class_name_case_refs(&ty, ctx, &mut case_refs);
                let ty = resolve_names_in_type(ty, ctx);
                let ty_native = if shared {
                    ty.clone()
                } else {
                    let native_only = native_parsed.unwrap_or(Type::mixed());
                    let t = mark_template_params(native_only, &template_params);
                    resolve_names_in_type(t, ctx)
                };
                let ty = substitute_type_aliases(ty, class_type_aliases);
                let ty = apply_implicit_nullable_default(ty, p.default.as_ref());
                // Mirrors `ty` with `doc_ty` forced to `None` - the
                // native-only split `return_type_native` also has.
                // Still passes through `apply_implicit_nullable_default`,
                // since that's a native-level PHP fact (an untyped `?`
                // hint with a `null` default is implicitly nullable),
                // independent of any docblock merge.
                let ty_native = apply_implicit_nullable_default(ty_native, p.default.as_ref());
                // A real `= value` default always wins; failing that, a
                // `[optional]` marker on the `@param` tag is trusted too
                // (phpstorm-stubs models several functions, e.g.
                // `mt_rand`/`array_keys`, only as their highest-arity
                // overload, with trailing params marked optional but no
                // native default).
                let has_default = p.default.is_some()
                    || doc
                        .as_ref()
                        .is_some_and(|d| d.optional_params.contains(p.name.as_ref()));
                // `@param-closure-this Type $name` - same
                // mark-then-resolve-then-substitute pipeline `ty` uses.
                let closure_this_type = doc
                    .as_ref()
                    .and_then(|d| d.param_closure_this.get(p.name.as_ref()))
                    .map(&resolve_doc_type);
                let out_ty = doc
                    .as_ref()
                    .and_then(|d| d.param_out.get(p.name.as_ref()))
                    .map(&resolve_doc_type);
                ParameterReflection {
                    attributes: ReflectionAttribute::all(&p.attrs, ctx),
                    name: p.name.clone(),
                    ty,
                    ty_native,
                    variadic: p.variadic,
                    has_default,
                    default_ty: p.default.as_ref().map(infer_const_type),
                    doc_ty: doc
                        .as_ref()
                        .and_then(|d| d.params.get(p.name.as_ref()))
                        .map(&resolve_doc_type),
                    by_ref: p.by_ref,
                    has_own_doc: doc_ty.is_some(),
                    explicit_mixed_hint: has_explicit_mixed_nested_somewhere(
                        p.type_hint.as_ref().map(TypeHint::to_string).as_deref(),
                        None,
                    ) || doc_ty.as_ref().is_some_and(mentions_mixed),
                    case_refs,
                    closure_this_type,
                    out_ty,
                }
            })
            .collect();
        let params = dedup_versioned_params(params);

        let doc_return = doc.as_ref().and_then(|d| d.return_type.as_ref());
        let native_return_hint_text = f.return_type.as_ref().map(TypeHint::to_string);
        let native_return_hint = native_return_hint_text.as_deref();
        let native_return_parsed = native_return_hint.map(types::parse_native_type_string);
        let doc_reconciled_return = resolve_type_with_parsed_native(
            doc_return,
            native_return_hint,
            native_return_parsed.as_ref(),
            &template_params,
        );
        let return_shared = Some(&doc_reconciled_return) == native_return_parsed.as_ref();
        let return_type = mark_template_params(doc_reconciled_return, &template_params);
        let mut return_case_refs = Vec::new();
        collect_class_name_case_refs(&return_type, ctx, &mut return_case_refs);
        let return_type = resolve_names_in_type(return_type, ctx);
        let return_type_native = if return_shared {
            return_type.clone()
        } else {
            let native_only = native_return_parsed.unwrap_or(Type::mixed());
            let t = mark_template_params(native_only, &template_params);
            resolve_names_in_type(t, ctx)
        };
        let return_type = substitute_type_aliases(return_type, class_type_aliases);

        let no_discard = f
            .attrs
            .iter()
            .any(|a| is_no_discard_attribute_name(&a.name));
        let must_use_result =
            crate::registry::access_attributes::has_must_use_result(&f.attrs, ctx);
        let has_override_attr = f.attrs.iter().any(|a| is_override_attribute_name(&a.name));
        let attributes = ReflectionAttribute::all(&f.attrs, ctx);

        let is_internal = doc.as_ref().is_some_and(|d| d.internal);
        let is_api = doc.as_ref().is_some_and(|d| d.api);
        let deprecated = doc.as_ref().and_then(|d| d.deprecated.clone());

        let throws =
            doc.as_ref()
                .into_iter()
                .flat_map(|d| &d.throws)
                .fold(None::<Type>, |acc, node| {
                    let ty = parse_type_node(node);
                    let ty = mark_template_params(ty, &template_params);
                    let ty = resolve_names_in_type(ty, ctx);
                    Some(match acc {
                        Some(prev) => Type::union(prev, ty),
                        None => ty,
                    })
                });

        let param_names: Vec<&str> = f.params.iter().map(|p| p.name.as_ref()).collect();
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

        FunctionReflection {
            provenance: crate::registry::provenance::Provenance::default(),
            params,
            return_type,
            return_type_native,
            return_has_own_doc: doc_return.is_some(),
            return_doc_ty: doc
                .as_ref()
                .and_then(|d| d.return_type.as_ref())
                .map(resolve_doc_type),
            self_out: doc
                .as_ref()
                .and_then(|d| d.self_out.as_ref())
                .map(resolve_doc_type),
            return_case_refs,
            return_explicit_mixed_hint: has_explicit_mixed_nested_somewhere(
                f.return_type.as_ref().map(TypeHint::to_string).as_deref(),
                None,
            ) || doc_return.is_some_and(mentions_mixed),
            no_discard,
            must_use_result,
            visibility,
            is_internal,
            is_api,
            has_override_attr,
            attributes,
            deprecated,
            throws,
            assert_if_true,
            assert_if_false,
            assert,
            purity_dependencies: None,
            is_transitively_pure: false,
            is_stub: false,
            is_magic: false,
        }
    }

    /// Merges two *unconditional* declarations of the
    /// same function name - the real-overload shape phpstorm-stubs uses
    /// for `strtr` (a 3-string-argument form and a 2-argument, `array
    /// $replace_pairs` form, as two separate top-level declarations
    /// sharing one name), mirroring `ClassReflection::merge`'s own "later
    /// declaration wins on conflict" convention.
    pub(crate) fn merge(self, other: FunctionReflection) -> FunctionReflection {
        let return_type = Type::union(self.return_type, other.return_type);
        let return_type_native = Type::union(self.return_type_native, other.return_type_native);
        // Permissive, same direction as `variadic`/`has_default` below:
        // if either real overload actually carries the marker, the merged
        // signature should too - a caller ignoring the return value must
        // not silently escape the obligation just because *one* overload
        // (of what's really the same underlying declaration, PHP-version-
        // split) happened not to repeat the attribute.
        let no_discard = self.no_discard || other.no_discard;
        let must_use_result = self.must_use_result || other.must_use_result;
        let has_override_attr = self.has_override_attr || other.has_override_attr;
        let len = self.params.len().max(other.params.len());
        let mut self_params = self.params.into_iter();
        let mut other_params = other.params.into_iter();
        let mut params = Vec::with_capacity(len);
        for _ in 0..len {
            let a = self_params.next();
            let b = other_params.next();
            let merged = match (a, b) {
                (Some(a), Some(b)) => ParameterReflection {
                    attributes: b.attributes,
                    // Later declaration wins on conflict, matching
                    // `ClassReflection::merge`'s own convention.
                    name: b.name,
                    ty: Type::union(a.ty, b.ty),
                    ty_native: Type::union(a.ty_native, b.ty_native),
                    // Permissive: accepting a variadic-style tail call
                    // when *either* real overload allows it costs nothing
                    // a stricter rule would have caught.
                    variadic: a.variadic || b.variadic,
                    // A position required by only one real overload can't
                    // be required in a callable meant to accept either.
                    has_default: a.has_default || b.has_default,
                    // Only a default both overloads agree on is known.
                    default_ty: (a.default_ty == b.default_ty)
                        .then(|| a.default_ty.clone())
                        .flatten(),
                    doc_ty: b.doc_ty.or(a.doc_ty),
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
                    // Both real overloads' own case refs are real,
                    // independent class-name references worth checking -
                    // concatenated, not chosen between, matching this
                    // function's own permissive "either side contributes"
                    // convention throughout.
                    case_refs: a.case_refs.into_iter().chain(b.case_refs).collect(),
                    // Later declaration wins on conflict, matching `name`
                    // above - falls back to the earlier one when only it
                    // declared the tag.
                    closure_this_type: b.closure_this_type.or(a.closure_this_type),
                    // Same later-declaration-wins-on-conflict-falling-
                    // back-to-earlier rule as `closure_this_type` above.
                    out_ty: b.out_ty.or(a.out_ty),
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
        FunctionReflection {
            provenance: self.provenance.clone(),
            params,
            return_type,
            return_type_native,
            // Same permissive "either overload's own marker wins" as
            // `no_discard`/`is_internal` below.
            return_has_own_doc: self.return_has_own_doc || other.return_has_own_doc,
            return_doc_ty: other.return_doc_ty.or(self.return_doc_ty),
            self_out: other.self_out.or(self.self_out),
            return_case_refs: self
                .return_case_refs
                .into_iter()
                .chain(other.return_case_refs)
                .collect(),
            return_explicit_mixed_hint: self.return_explicit_mixed_hint
                || other.return_explicit_mixed_hint,
            no_discard,
            must_use_result,
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
            // Both real overloads' own attributes are real, independent
            // facts worth checking - concatenated, not chosen between,
            // matching `case_refs`'s own identical reasoning below.
            attributes: self
                .attributes
                .into_iter()
                .chain(other.attributes)
                .collect(),
            // Same permissive "either overload's own marker wins" as
            // `no_discard`/`is_internal` above.
            deprecated: self.deprecated.or(other.deprecated),
            // Both real overloads can genuinely throw whatever either
            // one's own `@throws` documents - a caller has to be
            // prepared for either, whichever one actually runs.
            throws: match (self.throws, other.throws) {
                (Some(a), Some(b)) => Some(Type::union(a, b)),
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
            // Merging happens during indexing/stub-merging, always
            // before `Registry::resolve_transitive_purity` ever runs -
            // both sides are always `None`/`false` at this point, same
            // as any other not-yet-classified signature.
            purity_dependencies: None,
            is_transitively_pure: false,
            // `||`, not `&&`: merging a stub-derived signature with a
            // *second*, independently-parsed declaration of the same
            // name (e.g. a vendored copy of the same stub file, itself
            // also just as fake/bodyless) must stay excluded from purity
            // classification either way, since neither body is a real
            // implementation - hardcoding `false` here would silently
            // un-mark exactly this shape and reintroduce a `header()`/
            // `exit()`-style false positive.
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
/// shape: `pg_query`'s `$connection`, `fsockopen`'s `$port`). Real PHP
/// can't declare two same-named parameters (a fatal error), so any
/// duplicate name reaching here is unconditionally this versioning
/// artifact. The lexer's own `skip_stub_availability_attribute`
/// heuristic already drops some of these, but not one with a default
/// (needed for a different real shape: `imagefilter`'s uniquely-named
/// `$arg1`..`$arg4` before a variadic) - if that kept duplicate's name
/// is reused by a later parameter, every real argument after it would
/// bind to the wrong parameter. Keep each name's *last* occurrence
/// (phpstorm-stubs always writes the legacy era first), at the
/// *position* of its *first*, so later uniquely-named parameters aren't
/// shifted.
fn dedup_versioned_params(params: Vec<ParameterReflection>) -> Vec<ParameterReflection> {
    let mut order: Vec<Arc<str>> = Vec::new();
    let mut by_name: FxHashMap<Arc<str>, ParameterReflection> = FxHashMap::default();
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
/// the LSP's hover text, so the two never drift apart.
/// One parameter's own display label (`"int ...$xs"`, `"string $s"`) -
/// shared by `format_signature` and `signature_help`'s own per-parameter
/// `ParameterInformation` list, so the two never drift apart on how a
/// parameter is rendered.
pub(crate) fn param_label(p: &ParameterReflection) -> String {
    let dots = if p.variadic { "..." } else { "" };
    format!("{} {dots}${}", p.ty, p.name)
}

pub fn format_signature(name: &str, sig: &FunctionReflection) -> String {
    let params: Vec<String> = sig.params.iter().map(param_label).collect();
    format!("{name}({}): {}", params.join(", "), sig.return_type)
}
