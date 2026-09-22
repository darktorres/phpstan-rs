//! This project's `MissingTypehintCheck`-equivalent (real
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
//! Deliberately narrower than real PHPStan in two ways:
//!
//! - **No closure/arrow-fn call site.** Real PHPStan has no
//!   `MissingClosureParameterTypehintRule`/equivalent at all: exactly the
//!   six rules named above are registered at level 6, none closure-shaped.
//!   Real PHPStan deliberately doesn't nag on a closure/arrow-fn's own
//!   untyped parameters (they're routinely inferred from context, e.g.
//!   `array_map`'s callback), so inventing that check here would be new,
//!   false-positive-prone behavior with no real rule to model it on.
//!
//! - **No `missingType.callable` ("callable has no signature") check.**
//!   Real PHPStan gates this sub-check behind `checkMissingCallableSignature`,
//!   `false` by default in its own `conf/config.neon` - real PHPStan
//!   itself doesn't run this check on an ordinary project either. More
//!   fundamentally, this project's own `types::parse_type_string` collapses
//!   both a bare `callable`/`Closure` and a signature-carrying
//!   `callable(int): string`/`Closure(self): T` down to the exact same
//!   `Type::Callable` at parse time (see its own doc comment) - there is no
//!   way to tell "genuinely signature-less" apart from "had a signature
//!   PHPDoc wrote out" once a `Type` exists, so implementing this
//!   sub-check here would flag every callable-typed declaration in the
//!   project uniformly, a systematic false-positive generator this
//!   project's own stated philosophy treats as seriously as a missed
//!   detection.
use crate::ast::{ClassConst, ClassKind, FunctionDecl, PropertyDecl};
use crate::checker::{Checker, DiagnosticBuilder};
use crate::docblock;
use crate::registry::FunctionSig;
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
pub(crate) const ITERABLE_GENERIC_CLASS_NAMES: [&str; 4] =
    ["Traversable", "Iterator", "IteratorAggregate", "Generator"];

/// `ReflectionMethod`/`ReflectionProperty` carry a `TClass`
/// `@template` this project's own `php_stubs::patch_reflection_method_
/// and_property_templates` synthesizes internally (neither is generic in
/// real phpstorm-stubs, and real PHPStan itself never flags a bare
/// reference to either) - a bare, untemplated reference to one must never
/// be flagged, the same reasoning as `ITERABLE_GENERIC_CLASS_NAMES` just
/// above, just for a synthetic template rather than a real one.
pub(crate) const SYNTHETIC_GENERIC_CLASS_NAMES: [&str; 2] =
    ["ReflectionMethod", "ReflectionProperty"];

/// `RecursiveDirectoryIterator` carries a real (not synthetic)
/// `TKey`/`TValue` template pair once patched in
/// (`php_stubs::patch_spl_iterator_templates`) - real PHPStan resolves it
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
/// source writes no `: T` hint at all. This project's `FunctionSig`
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
    *ty == Type::Mixed(false) && !is_explicit_mixed_text(native_hint, doc_type)
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

/// True when `text` mentions the word `mixed` (case-insensitively, as a
/// whole word rather than a substring of some longer identifier like
/// `MixedTypeHelper`) anywhere at all - used to recover real PHPStan's
/// `MixedType::isExplicitMixed()` bit for a value type nested inside an
/// `array`/`iterable`/`list`/... generic, which (like the top-level case
/// `is_explicit_mixed_text` already handles) this project's `Type` model
/// has no room to carry through parsing (see that function's own doc
/// comment). Scoped to exactly one declaration's own single type
/// snippet (never a whole docblock), so any occurrence of the word is,
/// in practice, always the author's own explicit `mixed` written
/// somewhere in that same type - worst case a rare false *negative*
/// (silently accepting a still-implicit `mixed` elsewhere in the same
/// union/intersection), never a new false positive.
fn text_mentions_mixed(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let is_ident_byte = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut search_from = 0;
    while let Some(offset) = lower[search_from..].find("mixed") {
        let start = search_from + offset;
        let end = start + "mixed".len();
        let before_ok = start == 0 || !is_ident_byte(lower.as_bytes()[start - 1]);
        let after_ok = end == lower.len() || !is_ident_byte(lower.as_bytes()[end]);
        if before_ok && after_ok {
            return true;
        }
        search_from = end;
    }
    false
}

/// Mirrors `is_explicit_mixed_text`'s own two-source convention (a
/// native hint and a separate PHPDoc type can each independently spell
/// out `mixed`) but for the nested-value-type case `text_mentions_mixed`
/// covers - see its own doc comment for why a substring/whole-word check
/// is the right (and only practical) level of precision here.
///
/// `pub(crate)` - `registry::function_sig::FunctionSig::
/// from_decl` calls this too, to compute `ParamSig::explicit_mixed_hint`/
/// `FunctionSig::return_explicit_mixed_hint` once, from this
/// declaration's own text, at the same time it resolves `ty`/
/// `return_type` - see those fields' own doc comments for why a fresh
/// per-check re-derivation isn't enough once a type can be inherited
/// from a different declaration's own docblock.
pub(crate) fn has_explicit_mixed_nested_somewhere(
    native_hint: Option<&str>,
    doc_type: Option<&str>,
) -> bool {
    native_hint.is_some_and(text_mentions_mixed) || doc_type.is_some_and(text_mentions_mixed)
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
///
/// `explicit_mixed` suppresses a finding whose own element type is
/// `Mixed` when the source text actually spelled `mixed` out explicitly
/// (e.g. `array<string, mixed>`) - real PHPStan's own `$iterableValue
/// instanceof MixedType && !$iterableValue->isExplicitMixed()` guard.
/// Writing `mixed` explicitly is a common,
/// deliberate idiom for "I don't know/care about the element type, and
/// I don't want this rule to nag about it" - unlike a bare `array` (no
/// generic arguments at all), which real PHPStan still flags exactly as
/// before.
fn collect_iterable_missing_value_types(ty: &Type, explicit_mixed: bool, out: &mut Vec<String>) {
    match ty {
        Type::TemplateParam(_) | Type::Accessory(_) => {}
        Type::Array(_, inner) => {
            if matches!(inner.as_ref(), Type::Mixed(_)) {
                if !explicit_mixed {
                    out.push(ty.to_string());
                }
            } else {
                collect_iterable_missing_value_types(inner, explicit_mixed, out);
            }
        }
        Type::Union(members) => {
            if let Some(inner) = iterable_union_value_type(members) {
                if matches!(inner, Type::Mixed(_)) {
                    if !explicit_mixed {
                        out.push("iterable".to_string());
                    }
                } else {
                    collect_iterable_missing_value_types(inner, explicit_mixed, out);
                }
                return;
            }
            for m in members {
                collect_iterable_missing_value_types(m, explicit_mixed, out);
            }
        }
        Type::Intersection(members) => {
            // `list<V>`/`non-empty-array<V>`/`non-empty-list<V>` - the
            // only shapes this project's own `parse_type_keyword_or_class`
            // ever builds as `Intersection([Array(V), Accessory(_), ...])`
            // - found by locating the `Array` member directly
            // rather than re-checking which `AccessoryKind` rides along
            // with it.
            if let Some(inner) = members.iter().find_map(|m| match m {
                Type::Array(_, inner) => Some(inner.as_ref()),
                _ => None,
            }) {
                if matches!(inner, Type::Mixed(_)) {
                    if !explicit_mixed {
                        out.push(ty.to_string());
                    }
                } else {
                    collect_iterable_missing_value_types(inner, explicit_mixed, out);
                }
                return;
            }
            for m in members {
                collect_iterable_missing_value_types(m, explicit_mixed, out);
            }
        }
        Type::Object(_, args) => {
            for a in args {
                collect_iterable_missing_value_types(a, explicit_mixed, out);
            }
        }
        Type::ClassString(inner) => {
            collect_iterable_missing_value_types(inner, explicit_mixed, out)
        }
        Type::ArrayShape(fields) => {
            for (_, t, _) in fields {
                collect_iterable_missing_value_types(t, explicit_mixed, out);
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
                if ITERABLE_GENERIC_CLASS_NAMES.contains(&name.as_str())
                    || SYNTHETIC_GENERIC_CLASS_NAMES.contains(&name.as_str())
                    || MISSING_TYPEHINT_LENIENT_GENERIC_CLASS_NAMES.contains(&name.as_str())
                {
                    return;
                }
                if let Some(info) = self.registry.classes.get(name.as_str())
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
        explicit_mixed: bool,
    ) {
        let mut iterable = Vec::new();
        collect_iterable_missing_value_types(ty, explicit_mixed, &mut iterable);
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
                .and_then(|c| c.properties.get(p.name.as_ref()))
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
                has_explicit_mixed_nested_somewhere(p.type_hint.as_deref(), doc_var_type.as_deref()),
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
                .and_then(|info| info.consts.get(c.name.as_ref()))
                .map(|cs| cs.ty.clone())
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
                has_explicit_mixed_nested_somewhere(c.type_hint.as_deref(), doc_var_type.as_deref()),
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
    /// `sig` is the same `Option<&FunctionSig>` `check_function`
    /// itself already looked up - when present, its `ParamSig::
    /// explicit_mixed_hint`/`FunctionSig::return_explicit_mixed_hint`
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
        sig: Option<&FunctionSig>,
    ) {
        // The registry-computed `sig` (built once at index time via
        // `FunctionSig::from_decl`) already carries every per-param/
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
            let native_hint = f
                .params
                .iter()
                .find(|p| *p.name == **name)
                .and_then(|p| p.type_hint.as_deref());
            let doc_ty = doc
                .as_ref()
                .and_then(|d| d.params.get(name))
                .map(String::as_str);
            let explicit_mixed = match sig.and_then(|s| s.params.get(i)) {
                Some(param_sig) => param_sig.explicit_mixed_hint,
                None => has_explicit_mixed_nested_somewhere(native_hint, doc_ty),
            };
            if *ty == Type::Mixed(false) && !explicit_mixed {
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
                explicit_mixed,
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
            Some(s) => s.return_explicit_mixed_hint,
            None => has_explicit_mixed_nested_somewhere(native_return, doc_return),
        };
        let return_ty = declared_return.cloned().unwrap_or(Type::Mixed(false));
        if return_ty == Type::Mixed(false) && !return_explicit_mixed {
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
            return_explicit_mixed,
        );
    }
}
