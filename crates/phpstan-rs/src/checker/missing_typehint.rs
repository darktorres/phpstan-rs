//! This project's `MissingTypehintCheck`-equivalent (real
//! PHPStan's own `src/Rules/MissingTypehintCheck.php`, consumed by six
//! level-6 rules: `MissingPropertyTypehintRule`,
//! `MissingClassConstantTypehintRule`, `MissingMethodParameterTypehintRule`,
//! `MissingMethodReturnTypehintRule`, `MissingFunctionParameterTypehintRule`,
//! `MissingFunctionReturnTypehintRule`). Flags three shapes of "this
//! declaration is technically typed, but not usefully so": an implicit
//! `mixed` (no hint at all), a bare `array`/`iterable`/`list`/... with no
//! element type, and a bare reference to a class with its own required
//! `@template`s given no type arguments. A fourth shape, a `callable`/
//! `Closure` with no signature (`missingType.callable`), is opt-in
//! (`RulesSettings::check_missing_callable_signature`), matching real
//! PHPStan's own `checkMissingCallableSignature`, `false` by default in
//! its own `conf/config.neon`.
//!
//! No closure/arrow-fn call site: real PHPStan has no
//! `MissingClosureParameterTypehintRule`/equivalent at all - exactly the
//! six rules named above are registered at level 6, none closure-shaped.
//! Real PHPStan deliberately doesn't nag on a closure/arrow-fn's own
//! untyped parameters (they're routinely inferred from context, e.g.
//! `array_map`'s callback), so inventing that check here would be new,
//! false-positive-prone behavior with no real rule to model it on.
use crate::checker::Checker;
use crate::registry::FunctionReflection;
use diagnostics::DiagnosticBuilder;
use phpdoc::docblock;
use phpdoc::grammar::TypeNode;
use syntax::ast::TypeHint;
use syntax::ast::{ClassConst, ClassKind, FunctionDecl, PropertyDecl};
use syntax::token::Span;
use types::Type;
use types::has_explicit_mixed_nested_somewhere;

/// Real PHPStan's own tip text (`MissingTypehintCheck::
/// MISSING_ITERABLE_VALUE_TYPE_TIP`), attached to every
/// `missingType.iterableValue` diagnostic below the same way real PHPStan
/// attaches it.
const MISSING_ITERABLE_VALUE_TYPE_TIP: &str =
    "See: https://phpstan.org/blog/solving-phpstan-no-value-type-specified-in-iterable-type";

/// Mirrors real `MissingTypehintCheck::ITERABLE_GENERIC_CLASS_NAMES` and
/// its `checkGenericIterableClasses` feature toggle (`false` by default in
/// real PHPStan's own `conf/config.neon`): a bare reference to one of
/// these (e.g. a very common `function each(Iterator $it)`) is already
/// `isIterable()` and so already covered by the iterable-value-type check
/// above - phpstorm-stubs annotates all four with their own `@template
/// TKey`/`@template-covariant TValue`, so without this exclusion, this
/// extremely common real-code shape would *also* get flagged as a
/// non-generic use of a generic class, doubling up on the exact same
/// underlying "no element type given" finding under a different message.
pub(crate) const ITERABLE_GENERIC_CLASS_NAMES: [&str; 4] =
    ["Traversable", "Iterator", "IteratorAggregate", "Generator"];

/// `ReflectionMethod`/`ReflectionProperty` carry a `TClass`
/// `@template` this project's own `registry::stubs::patch_reflection_method_
/// and_property_templates` synthesizes internally (neither is generic in
/// real phpstorm-stubs, and real PHPStan itself never flags a bare
/// reference to either) - a bare, untemplated reference to one must never
/// be flagged, the same reasoning as `ITERABLE_GENERIC_CLASS_NAMES` just
/// above, just for a synthetic template rather than a real one.
pub(crate) const SYNTHETIC_GENERIC_CLASS_NAMES: [&str; 2] =
    ["ReflectionMethod", "ReflectionProperty"];

/// `RecursiveDirectoryIterator` carries a real (not synthetic)
/// `TKey`/`TValue` template pair once patched in
/// (`registry::stubs::patch_spl_iterator_templates`) - real PHPStan resolves it
/// as generic too (confirmed: a mismatched `@extends` argument there
/// triggers `generics.notCompatible`, which only fires past the
/// `isGeneric()` gate), yet a real, installed `phpstan/phpstan` never
/// flags a bare, untemplated reference to it with `missingType.generics`
/// either - confirmed directly: the identical probe that flagged every
/// other class `patch_spl_iterator_templates` patches left this one
/// alone. An observed real-PHPStan leniency to mirror, not this
/// project's own invention (unlike `SYNTHETIC_GENERIC_CLASS_NAMES`
/// above).
pub(crate) const MISSING_TYPEHINT_LENIENT_GENERIC_CLASS_NAMES: [&str; 1] =
    ["RecursiveDirectoryIterator"];

/// Method names PHP (and real PHPStan's own `PhpMethodReflection::
/// getReturnType`) give an implicit, non-`mixed` return type even when the
/// source writes no `: T` hint at all. This project's `FunctionReflection`
/// doesn't replicate that reflection-level substitution (a hint-less
/// declaration's `return_type` defaults to plain `Mixed` regardless of
/// which method it is), so without this exemption every ordinary,
/// hint-less `__construct`/`__destruct`/... in the whole project would
/// get flagged as "has no return type specified" - real PHPStan never
/// does, since its own reflection layer already knows `__construct`
/// implicitly returns `void`, not `mixed`.
const MAGIC_METHODS_WITH_IMPLICIT_RETURN_TYPE: [&str; 10] = [
    "__construct",
    "__destruct",
    "__set",
    "__unset",
    "__wakeup",
    "__clone",
    "__tostring",
    "__isset",
    "__sleep",
    "__set_state",
];

fn is_magic_method_with_implicit_return_type(name: &str) -> bool {
    MAGIC_METHODS_WITH_IMPLICIT_RETURN_TYPE
        .iter()
        .any(|m| name.eq_ignore_ascii_case(m))
}

pub(crate) fn class_kind_word(kind: ClassKind) -> &'static str {
    match kind {
        ClassKind::Class => "class",
        ClassKind::Interface => "interface",
        ClassKind::Trait => "trait",
        ClassKind::Enum => "enum",
    }
}

/// True when the type actually written for this declaration - its native
/// hint, or its own PHPDoc type tag - is the literal word `mixed`
/// (case-insensitively, tolerating a leading `?` some PHPDoc authors
/// still write out of habit even though `mixed` alone already includes
/// `null`). This project's `Type` model has exactly one `Mixed` variant
/// for both "the author wrote `mixed`" and "the author wrote nothing at
/// all" - real PHPStan's own `MixedType::isExplicitMixed()` distinguishes
/// the two at the type level (`types::parse_type_string` doesn't carry
/// that bit through, see its own doc comments on why), so the distinction
/// is recovered here instead, straight from the original source text.
fn is_explicit_mixed_text(native_hint: Option<&str>, doc_type: Option<&TypeNode>) -> bool {
    let is_word_mixed = |s: &str| {
        s.trim()
            .trim_start_matches('?')
            .trim()
            .eq_ignore_ascii_case("mixed")
    };
    let is_mixed_node = |node: &TypeNode| {
        let node = match node {
            TypeNode::Nullable(inner) => inner.as_ref(),
            other => other,
        };
        matches!(node, TypeNode::Identifier(name) if name.eq_ignore_ascii_case("mixed"))
    };
    doc_type.is_some_and(is_mixed_node) || native_hint.is_some_and(is_word_mixed)
}

/// Whether the native hint or the PHPDoc type spells out `mixed` anywhere,
/// nested included.
fn mentions_mixed(native_hint: Option<&str>, doc_type: Option<&TypeNode>) -> bool {
    has_explicit_mixed_nested_somewhere(native_hint, None)
        || doc_type.is_some_and(crate::registry::mentions_mixed)
}

/// The "no type specified at all" case each of the six rules below leads
/// with - `ty` resolved to plain `Mixed` (this project's stand-in for
/// both "no hint at all" and "explicit `mixed`" alike) where the source
/// itself didn't actually spell out `mixed`.
fn is_implicit_mixed(ty: &Type, native_hint: Option<&str>, doc_type: Option<&TypeNode>) -> bool {
    ty.is_implicit_mixed() && !is_explicit_mixed_text(native_hint, doc_type)
}

impl<'a> Checker<'a> {
    /// Walks `ty` for every bare reference (no `<...>` type arguments at
    /// all) to a class this project knows has its own `@template`s -
    /// mirrors real PHPStan's `MissingTypehintCheck::
    /// getNonGenericObjectTypesWithGenericClass`. Each finding is `(class
    /// description, comma-joined @template names)`, e.g. `("class
    /// Collection", "TKey, TValue")`. A class with type arguments already
    /// given still has those arguments walked (`Collection<array>` still
    /// flags `array`'s own missing value type, just via the sibling
    /// iterable-value walk, not this one) - a class this project has no
    /// real data for at all is silently skipped, the same "don't know,
    /// don't guess" leniency `check_generic_bounds`'s own
    /// `is_type_fully_known` documents for the identical situation.
    pub(crate) fn collect_non_generic_generic_class(
        &self,
        ty: &Type,
        out: &mut Vec<(String, String)>,
    ) {
        for name in ty.bare_object_class_names() {
            if ITERABLE_GENERIC_CLASS_NAMES.contains(&name.as_str())
                || SYNTHETIC_GENERIC_CLASS_NAMES.contains(&name.as_str())
                || MISSING_TYPEHINT_LENIENT_GENERIC_CLASS_NAMES.contains(&name.as_str())
            {
                continue;
            }
            // Only templates without a default must be given.
            if let Some(info) = self.registry.classes.get(name.as_str())
                && info.kind() != ClassKind::Trait
            {
                let total = info.templates().len();
                let required = info
                    .templates()
                    .iter()
                    .filter(|t| !info.template_defaults().contains_key(*t))
                    .count();
                if required > 0 {
                    let mut list = info.templates().join(", ");
                    if required != total {
                        list.push_str(&format!(" ({required}-{total} required)"));
                    }
                    out.push((format!("{} {name}", class_kind_word(info.kind())), list));
                }
            }
        }
    }

    /// Runs both walkers above against `ty` and reports one diagnostic per
    /// finding - `iterable_message`/`generics_message` build each specific
    /// declaration kind's own exact wording (matching real PHPStan's own
    /// per-rule `sprintf` templates, which differ enough between "Property
    /// ... type has no value type ..." / "Method ...() has parameter $x
    /// with no value type ..." / etc. that a single shared message
    /// template can't cover all six).
    pub(crate) fn report_missing_typehint_findings(
        &mut self,
        span: Span,
        iterable_message: impl Fn(&str) -> String,
        generics_message: impl Fn(&str, &str) -> String,
        ty: &Type,
        explicit_mixed: bool,
    ) {
        let mut iterable = Vec::new();
        iterable.extend(ty.iterable_types_missing_value_type(explicit_mixed));
        for desc in iterable {
            self.report_builder(
                DiagnosticBuilder::new(
                    iterable_message(&desc),
                    span,
                    6,
                    "missingType.iterableValue",
                )
                .tip(MISSING_ITERABLE_VALUE_TYPE_TIP),
            );
        }
        let mut generics = Vec::new();
        self.collect_non_generic_generic_class(ty, &mut generics);
        for (class_desc, list) in generics {
            self.report(
                generics_message(&class_desc, &list),
                span,
                6,
                "missingType.generics",
            );
        }
        if self.settings.rules.check_missing_callable_signature {
            let mut callables = Vec::new();
            callables.extend(ty.callables_missing_signature());
            // Real PHPStan's own `missingType.callable` wording is each
            // rule's `missingType.iterableValue` sentence with "no value
            // type specified in" swapped for "no signature specified
            // for", in all six rules - derived here rather than threading
            // a third per-kind message closure through every caller.
            for desc in callables {
                let message = iterable_message(&desc)
                    .replace("no value type specified in", "no signature specified for");
                self.report(message, span, 6, "missingType.callable");
            }
        }
    }

    /// `MissingPropertyTypehintRule`'s equivalent - every real
    /// (non-promoted, non-magic-`@property`) property a class declares
    /// directly. A promoted constructor property isn't in `properties`
    /// at all (it lives in `c.methods`'s own `__construct` params
    /// instead - see `registry::ClassReflection::from_decl`'s own comment), so
    /// it's naturally covered once, by the param-typehint check below,
    /// the same "checked exactly once, on whichever side actually wrote
    /// it" real PHPStan's own `isPromoted()` guard achieves.
    pub(crate) fn check_missing_property_typehints(
        &mut self,
        fqn: &str,
        properties: &[PropertyDecl],
    ) {
        for p in properties {
            let Some(ty) = self
                .registry
                .classes
                .get(fqn)
                .and_then(|c| c.properties().get(p.name.as_ref()))
                .map(|sig| sig.ty().clone())
            else {
                continue;
            };
            let doc_var_type = p
                .doc_comment
                .as_deref()
                .map(docblock::parse)
                .and_then(|d| d.var_type);
            if is_implicit_mixed(
                &ty,
                p.type_hint.as_ref().map(TypeHint::to_string).as_deref(),
                doc_var_type.as_ref(),
            ) {
                self.report(
                    format!("Property {fqn}::${} has no type specified.", p.name),
                    p.span,
                    6,
                    "missingType.property",
                );
                continue;
            }
            let name = p.name.clone();
            self.report_missing_typehint_findings(
                p.span,
                |desc| format!("Property {fqn}::${name} type has no value type specified in {desc}."),
                |class_desc,
                list| {
                    format!(
                        "Property {fqn}::${name} with generic {class_desc} does not specify its types: {list}"
                    )
                },
                &ty,
                mentions_mixed(p.type_hint.as_ref().map(TypeHint::to_string).as_deref(), doc_var_type.as_ref()),
            );
        }
    }

    /// `MissingClassConstantTypehintRule`'s equivalent. Unlike every other
    /// call site here, real PHPStan's own rule only ever runs against a
    /// constant's own *PHPDoc* type (`ClassReflection::getConstant(...)
    /// ->getPhpDocType()`) - a constant with no `@var` skips entirely,
    /// with no "no type specified" finding of its own the way a
    /// property/param/return gets. `infer_const_type` (this project's
    /// own const-type inference from the value expression alone) never
    /// actually produces an iterable-with-no-value-type or
    /// bare-generic-class shape on its own either (a plain `const X =
    /// [1, 2]` infers bare `Mixed`, not `Array(Mixed)` - see its own doc
    /// comment), so this guard mostly documents the same behavior real
    /// PHPStan has rather than changing an outcome this project would
    /// otherwise reach anyway.
    pub(crate) fn check_missing_class_constant_typehints(
        &mut self,
        fqn: &str,
        consts: &[ClassConst],
    ) {
        for c in consts {
            let doc_var_type = c
                .doc_comment
                .as_deref()
                .map(docblock::parse)
                .and_then(|d| d.var_type);
            if doc_var_type.is_none() {
                continue;
            }
            let Some(ty) = self
                .registry
                .classes
                .get(fqn)
                .and_then(|info| info.consts().get(c.name.as_ref()))
                .map(|cs| cs.ty().clone())
            else {
                continue;
            };
            let name = c.name.clone();
            self.report_missing_typehint_findings(
                c.span,
                |desc| format!("Constant {fqn}::{name} type has no value type specified in {desc}."),
                |class_desc,
                list| {
                    format!(
                        "Constant {fqn}::{name} with generic {class_desc} does not specify its types: {list}"
                    )
                },
                &ty,
                mentions_mixed(c.type_hint.as_ref().map(TypeHint::to_string).as_deref(), doc_var_type.as_ref()),
            );
        }
    }

    /// `MissingMethodParameterTypehintRule`/`MissingMethodReturnTypehintRule`
    /// /`MissingFunctionParameterTypehintRule`/
    /// `MissingFunctionReturnTypehintRule`'s shared equivalent - all four
    /// real rules key off the exact same two things (a function/method's
    /// own already-resolved parameter types and return type), so they're
    /// implemented together here the same way `check_function` itself
    /// already checks both functions and methods through one shared body.
    /// `resolved_params` is `(name, resolved type, AST span)` per
    /// parameter, built by `check_function` itself (before its own
    /// variadic-collects-into-`array` rewrite, which would otherwise mask
    /// e.g. a genuinely untyped `...$args`'s own per-argument `mixed`
    /// behind a already-array-shaped `Type::Array(Mixed)`).
    ///
    /// `sig` is the same `Option<&FunctionReflection>` `check_function`
    /// itself already looked up - when present, its `ParameterReflection::
    /// explicit_mixed_hint`/`FunctionReflection::return_explicit_mixed_hint`
    /// (computed once in `from_decl`, from whichever declaration's
    /// docblock `ty`/`return_type` actually came from - this declaration's
    /// own, or an ancestor's via `Registry::flatten_phpdoc_inheritance`)
    /// are used instead of re-deriving "was `mixed` written explicitly"
    /// from `f`'s own raw text: re-deriving from `f` alone would silently
    /// miss every inherited `array<string, mixed>`/nested-`mixed` type
    /// on a class implementing an interface with no docblock of its own.
    /// `sig` is only ever `None` for an anonymous class (never indexed
    /// into the registry - see `lookup_own_signature`'s own doc comment),
    /// where the text-based fallback below still applies.
    pub(crate) fn check_missing_function_typehints(
        &mut self,
        f: &FunctionDecl,
        class_name: Option<&str>,
        label: &str,
        resolved_params: &[(String, Type, Span)],
        declared_return: Option<&Type>,
        sig: Option<&FunctionReflection>,
    ) {
        // The registry-computed `sig` (built once at index time via
        // `FunctionReflection::from_decl`) already carries every per-param/
        // return "was there an explicit `mixed` somewhere" verdict this
        // function would otherwise re-derive by re-parsing the identical
        // raw docblock text again here - a real, measured cost at
        // `check_function`'s own call volume. Skip the full parse
        // whenever `sig` already covers every param (`resolved_params`
        // and `sig.params` are always built 1:1 from the same `f.params`,
        // so `sig.is_some()` alone is enough - see `lookup_own_signature`'s
        // own doc comment: `sig` is only ever `None` for an anonymous
        // class method) *and* the magic-method short-circuit just below
        // can't possibly need `doc_return`'s real presence/absence either
        // (checked here, cheaply, via the same name-table lookup that
        // short-circuit itself uses - not a substring probe, to avoid any
        // false-positive risk on that carefully-tuned check).
        let magic_method_may_need_doc_return = class_name.is_some()
            && f.return_type.is_none()
            && is_magic_method_with_implicit_return_type(&f.name);
        let doc = if sig.is_none() || magic_method_may_need_doc_return {
            f.doc_comment.as_deref().map(docblock::parse)
        } else {
            None
        };
        let kind_word = if class_name.is_some() {
            "Method"
        } else {
            "Function"
        };

        for (i, (name, ty, span)) in resolved_params.iter().enumerate() {
            let native_hint_text = f
                .params
                .iter()
                .find(|p| *p.name == **name)
                .and_then(|p| p.type_hint.as_ref())
                .map(TypeHint::to_string);
            let native_hint = native_hint_text.as_deref();
            let doc_ty = doc.as_ref().and_then(|d| d.params.get(name));
            let explicit_mixed = match sig.and_then(|s| s.params().get(i)) {
                Some(param_sig) => param_sig.explicit_mixed_hint(),
                None => mentions_mixed(native_hint, doc_ty),
            };
            if ty.is_mixed() && !explicit_mixed {
                self.report(
                    format!("{kind_word} {label}() has parameter ${name} with no type specified."),
                    *span,
                    6,
                    "missingType.parameter",
                );
                continue;
            }
            self.report_missing_typehint_findings(
                *span,
                |desc| {
                    format!(
                        "{kind_word} {label}() has parameter ${name} with no value type specified in {desc}."
                    )
                },
                |class_desc,
                list| {
                    format!(
                        "{kind_word} {label}() has parameter ${name} with generic {class_desc} but does not specify its types: {list}"
                    )
                },
                ty,
                explicit_mixed,
            );
        }

        let native_return_text = f.return_type.as_ref().map(TypeHint::to_string);

        let native_return = native_return_text.as_deref();
        let doc_return = doc.as_ref().and_then(|d| d.return_type.as_ref());

        // Real PHPStan's own reflection layer only substitutes a magic
        // method's own implicit return type (void/string/bool/...) when
        // the source itself writes no return type at *all* - a
        // `__sleep(): array` that explicitly (if incompletely) types its
        // own return is still real PHPStan's to flag, same as any other
        // declaration. See `MAGIC_METHODS_WITH_IMPLICIT_RETURN_TYPE`'s own
        // doc comment for why the fully-implicit case needs this at all.
        if class_name.is_some()
            && native_return.is_none()
            && doc_return.is_none()
            && is_magic_method_with_implicit_return_type(&f.name)
        {
            return;
        }

        // A second, narrower magic-method override real PHPStan's own
        // reflection layer applies unconditionally
        // (`PhpMethodFromParserNodeReflection`'s own constructor) -
        // unlike `MAGIC_METHODS_WITH_IMPLICIT_RETURN_TYPE` above, this one
        // fires even when the source does write an explicit `: array`
        // hint with no `@return` PHPDoc at all: `__serialize()`'s own
        // real return type is hardcoded to `array<mixed, mixed>` with
        // explicit (not implicit) `mixed` key/value types (an ordinary,
        // non-magic method with the identical bare `: array` hint and
        // body is flagged; `__serialize()` never is). An explicit-mixed
        // value type is exactly what `missingType.iterableValue` exists
        // to require - it's satisfied unconditionally here, not merely
        // absent a stricter one, so this exempts only that specific
        // check (and its sibling `missingType.generics`, moot for a
        // plain `Array` type either way), not the whole function.
        // `__debuginfo()` gets the identical `array<mixed, mixed>|null`
        // override in the same real source location.
        if class_name.is_some()
            && doc_return.is_none()
            && matches!(
                f.name.to_ascii_lowercase().as_str(),
                "__serialize" | "__debuginfo"
            )
        {
            return;
        }

        let return_explicit_mixed = match sig {
            Some(s) => s.return_explicit_mixed_hint(),
            None => mentions_mixed(native_return, doc_return),
        };
        let return_ty = declared_return.cloned().unwrap_or(Type::mixed());
        if return_ty.is_mixed() && !return_explicit_mixed {
            self.report(
                format!("{kind_word} {label}() has no return type specified."),
                f.span,
                6,
                "missingType.return",
            );
            return;
        }
        self.report_missing_typehint_findings(
            f.span,
            |desc| format!("{kind_word} {label}() return type has no value type specified in {desc}."),
            |class_desc,
            list| {
                format!(
                    "{kind_word} {label}() return type with generic {class_desc} does not specify its types: {list}"
                )
            },
            &return_ty,
            return_explicit_mixed,
        );
    }
}
