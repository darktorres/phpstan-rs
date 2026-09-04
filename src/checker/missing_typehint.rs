//! Phase 58: this project's `MissingTypehintCheck`-equivalent (real
//! PHPStan's own `src/Rules/MissingTypehintCheck.php`, consumed by six
//! level-6 rules: `MissingPropertyTypehintRule`,
//! `MissingClassConstantTypehintRule`, `MissingMethodParameterTypehintRule`,
//! `MissingMethodReturnTypehintRule`, `MissingFunctionParameterTypehintRule`,
//! `MissingFunctionReturnTypehintRule`). Flags three shapes of "this
//! declaration is technically typed, but not usefully so": an implicit
//! `mixed` (no hint at all), a bare `array`/`iterable`/`list`/... with no
//! element type, and a bare reference to a class with its own required
//! `@template`s given no type arguments.
//!
//! Deliberately narrower than real PHPStan in two ways, both confirmed
//! against the real source before being dropped rather than guessed:
//!
//! - **No closure/arrow-fn call site.** The roadmap phase text's own intro
//!   paragraph lists "closure, arrow-fn" among the declaration kinds this
//!   phase covers, and cites `src/checker/expr.rs`'s closure-literal
//!   checking sites as a needed call site - but real PHPStan has no
//!   `MissingClosureParameterTypehintRule`/equivalent at all (confirmed via
//!   `grep -rl "RegisteredRule(level: 6)"` across
//!   `vendor/phpstan-src/src/Rules/`: exactly the six rules named above,
//!   none closure-shaped), and the *same* phase text's own "Unlocks" list
//!   names exactly those six rules - none of them closures. Real PHPStan
//!   deliberately doesn't nag on a closure/arrow-fn's own untyped
//!   parameters (they're routinely inferred from context, e.g.
//!   `array_map`'s callback), so inventing that check here would be new,
//!   false-positive-prone behavior with no real rule to model it on -
//!   exactly the kind of stale roadmap detail this project's own
//!   conventions say to correct rather than follow. The five call sites
//!   the roadmap's own audit notes describe (`check_program` for
//!   properties/consts, `check_function` for every param/return, shared
//!   between plain functions and methods) already account for all six
//!   rules with no closure call site needed.
//!
//! - **No `missingType.callable` ("callable has no signature") check.**
//!   Real PHPStan gates this sub-check behind `checkMissingCallableSignature`,
//!   `false` by default in its own `conf/config.neon` - real PHPStan
//!   itself doesn't run this check on an ordinary project either. More
//!   fundamentally, this project's own `types::parse_type_string` collapses
//!   both a bare `callable`/`Closure` *and* a signature-carrying
//!   `callable(int): string`/`Closure(self): T` down to the exact same
//!   `Type::Callable` at parse time (see its own doc comment) - there is no
//!   way to tell "genuinely signature-less" apart from "had a signature
//!   PHPDoc wrote out" once a `Type` exists, so implementing this
//!   sub-check here would flag *every* callable-typed declaration in the
//!   project uniformly, a systematic false-positive generator this
//!   project's own stated philosophy treats as seriously as a missed
//!   detection.
use crate::ast::{ClassConst, ClassKind, FunctionDecl, PropertyDecl};
use crate::checker::{Checker, DiagnosticBuilder};
use crate::docblock;
use crate::token::Span;
use crate::types::Type;

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
const ITERABLE_GENERIC_CLASS_NAMES: [&str; 4] =
    ["Traversable", "Iterator", "IteratorAggregate", "Generator"];

/// Method names PHP (and real PHPStan's own `PhpMethodReflection::
/// getReturnType`) give an implicit, non-`mixed` return type even when the
/// source writes no `: T` hint at all - confirmed directly from that
/// method's own source rather than guessed. This project's `FunctionSig`
/// doesn't replicate that reflection-level substitution (a hint-less
/// declaration's `return_type` defaults to plain `Mixed` regardless of
/// which method it is), so without this exemption every ordinary,
/// hint-less `__construct`/`__destruct`/... in the whole project would
/// get flagged as "has no return type specified" - real PHPStan never
/// does, since its own reflection layer already knows `__construct`
/// implicitly returns `void`, not `mixed`.
const MAGIC_METHODS_WITH_IMPLICIT_RETURN_TYPE: [&str; 9] = [
    "__construct",
    "__destruct",
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

fn class_kind_word(kind: ClassKind) -> &'static str {
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
fn is_explicit_mixed_text(native_hint: Option<&str>, doc_type: Option<&str>) -> bool {
    let is_word_mixed = |s: &str| {
        s.trim()
            .trim_start_matches('?')
            .trim()
            .eq_ignore_ascii_case("mixed")
    };
    doc_type.is_some_and(is_word_mixed) || native_hint.is_some_and(is_word_mixed)
}

/// The "no type specified at all" case each of the six rules below leads
/// with - `ty` resolved to plain `Mixed` (this project's stand-in for
/// both "no hint at all" and "explicit `mixed`" alike) where the source
/// itself didn't actually spell out `mixed`.
fn is_implicit_mixed(ty: &Type, native_hint: Option<&str>, doc_type: Option<&str>) -> bool {
    *ty == Type::Mixed && !is_explicit_mixed_text(native_hint, doc_type)
}

/// A bare `iterable`'s own two-member shape (`types::parse_type_string`
/// always builds it as exactly `Union([Array(value), Object("Traversable",
/// [])])` - see that function's own doc comment) - `Some(value)` when
/// `members` is that exact shape, checked by content rather than
/// positionally in case something upstream ever reorders a `Union`'s own
/// members.
fn iterable_union_value_type(members: &[Type]) -> Option<&Type> {
    if members.len() != 2 {
        return None;
    }
    let array_inner = members.iter().find_map(|m| match m {
        Type::Array(_, inner) => Some(inner.as_ref()),
        _ => None,
    })?;
    let has_bare_traversable = members
        .iter()
        .any(|m| matches!(m, Type::Object(name, args) if name == "Traversable" && args.is_empty()));
    has_bare_traversable.then_some(array_inner)
}

/// Walks `ty` for every iterable-shaped member (`array`, the `iterable`
/// pseudo-type, `list`/`non-empty-array`/`non-empty-list`) whose own
/// element type is implicit `mixed` - each finding is that member's own
/// `Display`ed description (`"array"`, `"iterable"`, `"list"`, ...),
/// ready to drop straight into a `"... has no value type specified in
/// {desc}."` message the same way real PHPStan's own
/// `describe(VerbosityLevel::typeOnly())` output is. Mirrors real
/// PHPStan's `MissingTypehintCheck::getIterableTypesWithMissingValueTypehint`,
/// recursing into everything (a class's own generic type arguments, a
/// union/intersection's own members, an array shape's own field types)
/// except a `@template` placeholder itself, matching that function's own
/// `TemplateType` early-return.
fn collect_iterable_missing_value_types(ty: &Type, out: &mut Vec<String>) {
    match ty {
        Type::TemplateParam(_) | Type::Accessory(_) => {}
        Type::Array(_, inner) => {
            if matches!(inner.as_ref(), Type::Mixed) {
                out.push(ty.to_string());
            } else {
                collect_iterable_missing_value_types(inner, out);
            }
        }
        Type::Union(members) => {
            if let Some(inner) = iterable_union_value_type(members) {
                if matches!(inner, Type::Mixed) {
                    out.push("iterable".to_string());
                } else {
                    collect_iterable_missing_value_types(inner, out);
                }
                return;
            }
            for m in members {
                collect_iterable_missing_value_types(m, out);
            }
        }
        Type::Intersection(members) => {
            // `list<V>`/`non-empty-array<V>`/`non-empty-list<V>` - the
            // only shapes this project's own `parse_type_keyword_or_class`
            // ever builds as `Intersection([Array(V), Accessory(_), ...])`
            // (Phase 14) - found by locating the `Array` member directly
            // rather than re-checking which `AccessoryKind` rides along
            // with it.
            if let Some(inner) = members.iter().find_map(|m| match m {
                Type::Array(_, inner) => Some(inner.as_ref()),
                _ => None,
            }) {
                if matches!(inner, Type::Mixed) {
                    out.push(ty.to_string());
                } else {
                    collect_iterable_missing_value_types(inner, out);
                }
                return;
            }
            for m in members {
                collect_iterable_missing_value_types(m, out);
            }
        }
        Type::Object(_, args) => {
            for a in args {
                collect_iterable_missing_value_types(a, out);
            }
        }
        Type::ClassString(inner) => collect_iterable_missing_value_types(inner, out),
        Type::ArrayShape(fields) => {
            for (_, t, _) in fields {
                collect_iterable_missing_value_types(t, out);
            }
        }
        _ => {}
    }
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
        match ty {
            Type::TemplateParam(_) => {}
            Type::Object(name, args) => {
                if !args.is_empty() {
                    for a in args {
                        self.collect_non_generic_generic_class(a, out);
                    }
                    return;
                }
                if ITERABLE_GENERIC_CLASS_NAMES.contains(&name.as_str()) {
                    return;
                }
                if let Some(info) = self.registry.classes.get(name)
                    && info.kind != ClassKind::Trait
                    && !info.templates.is_empty()
                {
                    out.push((
                        format!("{} {name}", class_kind_word(info.kind)),
                        info.templates.join(", "),
                    ));
                }
            }
            Type::Array(key, inner) => {
                self.collect_non_generic_generic_class(key, out);
                self.collect_non_generic_generic_class(inner, out);
            }
            Type::ClassString(inner) => {
                self.collect_non_generic_generic_class(inner, out);
            }
            Type::Union(members) | Type::Intersection(members) => {
                for m in members {
                    self.collect_non_generic_generic_class(m, out);
                }
            }
            Type::ArrayShape(fields) => {
                for (_, t, _) in fields {
                    self.collect_non_generic_generic_class(t, out);
                }
            }
            _ => {}
        }
    }

    /// Runs both walkers above against `ty` and reports one diagnostic per
    /// finding - `iterable_message`/`generics_message` build each specific
    /// declaration kind's own exact wording (matching real PHPStan's own
    /// per-rule `sprintf` templates, which differ enough between "Property
    /// ... type has no value type ..." / "Method ...() has parameter $x
    /// with no value type ..." / etc. that a single shared message
    /// template can't cover all six).
    fn report_missing_typehint_findings(
        &mut self,
        location: &str,
        span: Span,
        iterable_message: impl Fn(&str) -> String,
        generics_message: impl Fn(&str, &str) -> String,
        ty: &Type,
    ) {
        let mut iterable = Vec::new();
        collect_iterable_missing_value_types(ty, &mut iterable);
        for desc in iterable {
            self.report_builder(
                DiagnosticBuilder::new(
                    location.to_string(),
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
                location.to_string(),
                generics_message(&class_desc, &list),
                span,
                6,
                "missingType.generics",
            );
        }
    }

    /// `MissingPropertyTypehintRule`'s equivalent - every real
    /// (non-promoted, non-magic-`@property`) property a class declares
    /// directly. A promoted constructor property isn't in `properties`
    /// at all (it lives in `c.methods`'s own `__construct` params
    /// instead - see `registry::ClassInfo::from_decl`'s own comment), so
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
                .and_then(|c| c.properties.get(&p.name))
                .map(|sig| sig.ty.clone())
            else {
                continue;
            };
            let doc_var_type = p
                .doc_comment
                .as_deref()
                .map(docblock::parse)
                .and_then(|d| d.var_type);
            if is_implicit_mixed(&ty, p.type_hint.as_deref(), doc_var_type.as_deref()) {
                self.report(
                    fqn.to_string(),
                    format!("Property {fqn}::${} has no type specified.", p.name),
                    p.span,
                    6,
                    "missingType.property",
                );
                continue;
            }
            let name = p.name.clone();
            self.report_missing_typehint_findings(
                fqn,
                p.span,
                |desc| format!("Property {fqn}::${name} type has no value type specified in {desc}."),
                |class_desc, list| {
                    format!(
                        "Property {fqn}::${name} with generic {class_desc} does not specify its types: {list}"
                    )
                },
                &ty,
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
            let has_doc_type = c
                .doc_comment
                .as_deref()
                .map(docblock::parse)
                .is_some_and(|d| d.var_type.is_some());
            if !has_doc_type {
                continue;
            }
            let Some(ty) = self
                .registry
                .classes
                .get(fqn)
                .and_then(|info| info.consts.get(&c.name))
                .cloned()
            else {
                continue;
            };
            let name = c.name.clone();
            self.report_missing_typehint_findings(
                fqn,
                c.span,
                |desc| format!("Constant {fqn}::{name} type has no value type specified in {desc}."),
                |class_desc, list| {
                    format!(
                        "Constant {fqn}::{name} with generic {class_desc} does not specify its types: {list}"
                    )
                },
                &ty,
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
    pub(crate) fn check_missing_function_typehints(
        &mut self,
        f: &FunctionDecl,
        class_name: Option<&str>,
        label: &str,
        resolved_params: &[(String, Type, Span)],
        declared_return: Option<&Type>,
    ) {
        let doc = f.doc_comment.as_deref().map(docblock::parse);
        let kind_word = if class_name.is_some() {
            "Method"
        } else {
            "Function"
        };

        for (name, ty, span) in resolved_params {
            let native_hint = f
                .params
                .iter()
                .find(|p| &p.name == name)
                .and_then(|p| p.type_hint.as_deref());
            let doc_ty = doc
                .as_ref()
                .and_then(|d| d.params.get(name))
                .map(String::as_str);
            if is_implicit_mixed(ty, native_hint, doc_ty) {
                self.report(
                    label.to_string(),
                    format!("{kind_word} {label}() has parameter ${name} with no type specified."),
                    *span,
                    6,
                    "missingType.parameter",
                );
                continue;
            }
            self.report_missing_typehint_findings(
                label,
                *span,
                |desc| {
                    format!(
                        "{kind_word} {label}() has parameter ${name} with no value type specified in {desc}."
                    )
                },
                |class_desc, list| {
                    format!(
                        "{kind_word} {label}() has parameter ${name} with generic {class_desc} but does not specify its types: {list}"
                    )
                },
                ty,
            );
        }

        let native_return = f.return_type.as_deref();
        let doc_return = doc.as_ref().and_then(|d| d.return_type.as_deref());

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

        let return_ty = declared_return.cloned().unwrap_or(Type::Mixed);
        if is_implicit_mixed(&return_ty, native_return, doc_return) {
            self.report(
                label.to_string(),
                format!("{kind_word} {label}() has no return type specified."),
                f.span,
                6,
                "missingType.return",
            );
            return;
        }
        self.report_missing_typehint_findings(
            label,
            f.span,
            |desc| format!("{kind_word} {label}() return type has no value type specified in {desc}."),
            |class_desc, list| {
                format!(
                    "{kind_word} {label}() return type with generic {class_desc} does not specify its types: {list}"
                )
            },
            &return_ty,
        );
    }
}
