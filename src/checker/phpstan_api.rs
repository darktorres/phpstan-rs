//! Flags a third-party project's own references into real PHPStan's
//! `PHPStan\*` namespace that aren't marked `@api` (or, for an
//! `extends`/`implements`/trait `use`, specifically marked
//! `@api-do-not-implement`) - PHPStan's own documented backward-
//! compatibility promise (see `vendor/phpstan-src`'s own `CLAUDE.md`:
//! "Code marked with `@api` must not break backward compatibility... Any
//! code without `@api` may change in minor versions without notice").
//! Mirrors real PHPStan's own `Rules\Api\*` rule family
//! (`ApiInstantiationRule`, `ApiStaticCallRule`, `ApiClassConstFetchRule`,
//! `ApiInstanceofRule`, `ApiClassExtendsRule`, `ApiClassImplementsRule`,
//! `ApiInterfaceExtendsRule`, `ApiTraitUseRule`, `ApiMethodCallRule`,
//! `NodeConnectingVisitorAttributesRule` - the last two checked from the
//! main `Checker` pass instead, see below): this checks code that
//! consumes real PHPStan as a Composer library.
//!
//! Only ever called when `composer::detect_phpstan_dependency` found a
//! real `phpstan/phpstan`/`phpstan/extension-installer` dependency in the
//! analyzed project's own `composer.json` (`Project::check_file`) - a
//! project with no such dependency has no backward-compatibility promise
//! to check anything against, and running this unconditionally would
//! risk flagging a project's own, unrelated `PHPStan\`-namespaced code
//! with a diagnostic that means nothing to it.
//!
//! **Namespace signal plus a vendor-path check**: real PHPStan's own
//! `ApiRuleHelper::isPhpStanCode` additionally confirms the
//! referenced declaration's own file lives under a `vendor/` directory,
//! but *only* when the *calling* scope's own namespace is *also*
//! `PHPStan\*`-prefixed - self-referential PHPStan code (PHPStan
//! analyzing itself) or a PHPStan extension package conventionally
//! namespaced `PHPStan\<Vendor>` (phpstan-doctrine, phpstan-symfony, ...).
//! For the overwhelming majority of real usage (an ordinary project's own
//! non-`PHPStan\*`-namespaced code referencing real PHPStan), this is a
//! pure no-op - `is_phpstan_code` reduces to the same bare namespace
//! check as before. `Registry::phpstan_class_files` (populated during
//! indexing, only for names already `PHPStan\*`-namespaced) carries the
//! one narrow, disclosed crack in this project's usual path-agnostic
//! registry design that this needed.
//!
//! **Reference kinds covered** (each only against a name the registry
//! actually has an entry for - an unresolved/typo'd name already gets its
//! own, more meaningful undefined-class diagnostic elsewhere, and isn't
//! double-reported here): `new X(...)`, `X::staticMethod()`, `X::CONST`
//! (but not `X::class` - see below), `$x instanceof X`, `extends X`,
//! `implements X`, `use X;` (trait). Instance method calls
//! (`$scope->getType(...)`, `phpstanApi.method`) and
//! `$node->getAttribute('parent'|'previous'|'next')`
//! (`phpParser.nodeConnectingAttribute`) are checked separately, in the
//! main `Checker` pass instead (`checker/expr.rs`'s own
//! `MethodCall` arm, via `Checker::check_phpstan_api_method_call`/
//! `check_node_connecting_attribute`), which already computes a method
//! call's real receiver class via full general type inference
//! (`find_method_receiver`) for an unrelated purpose (member resolution) -
//! reusing it recognizes every receiver shape, not just a directly-typed
//! parameter or `$this->prop`.
//!
//! `X::class` is deliberately never flagged: real PHPStan's own
//! `ApiClassConstFetchRule` does flag it, but only after also checking
//! every one of the class's own methods for a stray `@api` in their doc
//! comment (a special case this project has no equivalent machinery for -
//! method-level `@api` isn't parsed into `registry::FunctionSig` at all,
//! only the class-level tag is, via `registry::ClassInfo::is_api`).
//! Flagging bare `SomeExtensionInterface::class` - an extremely common,
//! benign idiom for naming a class in a DI container registration or
//! `#[RegisteredRule]`-style attribute, not actually "using" its API
//! surface - without that nuance would be pure noise.

use std::collections::HashMap;
use std::path::Path;

use crate::ast::*;
use crate::ast_walk::{expr_children, stmt_children};
use crate::checker::{Checker, Diagnostic, class_type_word, literal_string_value};
use crate::composer;
use crate::registry::{FunctionSig, NameContext, Registry, is_phpstan_namespace};
use crate::token::Span;

/// `$variable-name -> resolved fully-qualified class name` for whichever
/// of the enclosing function/method/closure/arrow-fn's own parameters (or
/// closure `use`-captures) carry a single, unambiguous native class type
/// hint - see `param_types_with`'s own doc comment for exactly what
/// counts. Not filtered to `PHPStan\*` at collection time (unlike every
/// other reference kind's own `resolve_if_phpstan` check) - collection
/// stays reusable/simple, and every lookup site re-checks the namespace
/// itself.
type ParamTypes = HashMap<String, String>;

/// `file_path` is `None` in every existing test (`Walker::
/// tests`'s own `diagnostics_for` has no real file on disk to point at) -
/// only `PhpStanNamespaceIn3rdPartyPackageRule` needs it at all (it's the
/// only one of these rules that cares which real package a file belongs
/// to), and simply never fires without one, the same "no signal, stay
/// quiet" leniency every other unresolvable case in this module already
/// gets.
pub(crate) fn check_phpstan_api_usage(
    program: &[Stmt],
    registry: &Registry,
    file_path: Option<&Path>,
) -> Vec<Diagnostic> {
    // Same "last `namespace` seen wins, brace-delimited multi-namespace
    // files are rare enough not to bother" simplification
    // `Checker::check_program` itself already makes for exactly the same
    // reason (see its own comment there).
    let ctx = NameContext::build(program);
    let mut walker = Walker {
        registry,
        diagnostics: Vec::new(),
        current_prop_types: ParamTypes::new(),
        current_class_implements_phpstan_interface: false,
        current_class_fqn: None,
        file_path,
    };
    let empty = ParamTypes::new();
    for stmt in program {
        walker.check_stmt(stmt, &ctx, &empty, "<main>");
    }
    walker.diagnostics
}

/// Case-insensitive `PHPStan\`-prefix match (mirrors real PHPStan's own
/// `ApiRuleHelper::isPhpStanName`'s `stripos($namespace, 'PHPStan\\') ===
/// 0`), excluding the two real, separately-maintained third-party
/// libraries PHPStan bundles under its own namespace umbrella
/// (`phpstan/phpdoc-parser`, `ondrejmirtes/better-reflection`) rather than
/// actual PHPStan API surface - excluded via a case-*sensitive* prefix
/// match, deliberately mirroring `ApiRuleHelper`'s own `str_starts_with`
/// exactly (no case-insensitivity exemption for either sub-namespace).
/// `ApiInstanceofTypeRule`'s own compiled `#[InstanceofDeprecated]`
/// table (`vendor/phpstan-src/src/Type/**/*.php`, one entry per real
/// attribute site). Keyed by lowercase FQN (matching real PHPStan's own `strtolower()`
/// lookup-map construction); the value is the attribute's own
/// `insteadUse` hint, `None` for the two sites that declare the attribute
/// with no replacement suggestion at all.
const INSTANCEOF_DEPRECATED_CLASSES: &[(&str, Option<&str>)] = &[
    ("phpstan\\type\\booleantype", Some("Type::isBoolean()")),
    (
        "phpstan\\type\\arraytype",
        Some("Type::isArray() or Type::getArrays()"),
    ),
    ("phpstan\\type\\floattype", Some("Type::isFloat()")),
    ("phpstan\\type\\iterabletype", Some("Type::isIterable()")),
    (
        "phpstan\\type\\objectshapetype",
        Some("Type::isObject() and Type::hasProperty()"),
    ),
    ("phpstan\\type\\integertype", Some("Type::isInteger()")),
    ("phpstan\\type\\stringtype", Some("Type::isString()")),
    (
        "phpstan\\type\\accessory\\oversizedarraytype",
        Some("Type::isOversizedArray()"),
    ),
    (
        "phpstan\\type\\objectwithoutclasstype",
        Some("Type::isObject()"),
    ),
    (
        "phpstan\\type\\accessory\\accessorynonemptystringtype",
        Some("Type::isNonEmptyString()"),
    ),
    (
        "phpstan\\type\\objecttype",
        Some("Type::isObject() or Type::getObjectClassNames()"),
    ),
    ("phpstan\\type\\voidtype", Some("Type::isVoid()")),
    (
        "phpstan\\type\\accessory\\haspropertytype",
        Some("Type::hasProperty()"),
    ),
    (
        "phpstan\\type\\callabletype",
        Some("Type::isCallable() and Type::getCallableParametersAcceptors()"),
    ),
    (
        "phpstan\\type\\accessory\\accessorytype",
        Some("methods on PHPStan\\Type\\Type"),
    ),
    ("phpstan\\type\\nulltype", Some("Type::isNull()")),
    (
        "phpstan\\type\\constantscalartype",
        Some(
            "Type::isConstantScalarValue() or Type::getConstantScalarTypes() or Type::getConstantScalarValues()",
        ),
    ),
    (
        "phpstan\\type\\accessory\\accessorynonfalsystringtype",
        Some("Type::isNonFalsyString()"),
    ),
    (
        "phpstan\\type\\accessory\\accessoryuppercasestringtype",
        Some("Type::isUppercaseString()"),
    ),
    (
        "phpstan\\type\\typewithclassname",
        Some("Type::getObjectClassNames() or Type::getObjectClassReflections()"),
    ),
    (
        "phpstan\\type\\accessory\\accessorynumericstringtype",
        Some("Type::isNumericString()"),
    ),
    (
        "phpstan\\type\\accessory\\accessoryarraylisttype",
        Some("Type::isList()"),
    ),
    (
        "phpstan\\type\\accessory\\accessorylowercasestringtype",
        Some("Type::isLowercaseString()"),
    ),
    (
        "phpstan\\type\\accessory\\hasoffsettype",
        Some("Type::hasOffsetValueType()"),
    ),
    (
        "phpstan\\type\\accessory\\accessoryliteralstringtype",
        Some("Type::isLiteralString()"),
    ),
    (
        "phpstan\\type\\constant\\constantstringtype",
        Some("Type::getConstantStrings()"),
    ),
    (
        "phpstan\\type\\enum\\enumcaseobjecttype",
        Some("Type::getEnumCaseObject() or Type::getEnumCases()"),
    ),
    (
        "phpstan\\type\\generic\\genericclassstringtype",
        Some("Type::isClassStringType() and Type::getClassStringObjectType()"),
    ),
    (
        "phpstan\\type\\constant\\constantbooleantype",
        Some("Type::isTrue() or Type::isFalse()"),
    ),
    (
        "phpstan\\type\\constant\\constantarraytype",
        Some("Type::getConstantArrays()"),
    ),
    (
        "phpstan\\type\\classstringtype",
        Some("Type::isClassStringType()"),
    ),
    (
        "phpstan\\type\\accessory\\nonemptyarraytype",
        Some("Type::isIterableAtLeastOnce()"),
    ),
    (
        "phpstan\\type\\accessory\\hasmethodtype",
        Some("Type::hasMethod()"),
    ),
];

/// Whether `fqn` transitively derives from, implements, or uses
/// anything under the `PHPStan\` namespace at all -
/// `RuntimeReflectionFunctionRule`/`RuntimeReflectionInstantiationRule`/
/// `OldPhpParser4ClassRule`'s own shared gate (real PHPStan's own
/// `$scope->getClassReflection()->getInterfaces()` walk, matched
/// case-sensitively against `PHPStan\` the identical way those three
/// rules already do). Reuses `Registry::all_ancestor_names` (already
/// walks `extends`/`implements`/`uses` transitively) rather than a
/// narrower "interfaces only" walk of its own - a real class or trait
/// ancestor (not just an interface) is a looser condition than what real
/// PHPStan's `getInterfaces()` checks, favoring false positives over
/// false negatives, this project's usual leaning.
pub(crate) fn class_implements_phpstan_interface(fqn: &str, registry: &Registry) -> bool {
    registry
        .all_ancestor_names(fqn)
        .iter()
        .any(|name| is_phpstan_namespace(name))
}

/// Direct port of `ApiRuleHelper::isPhpStanCode`
/// (`vendor/phpstan-src/src/Rules/Api/ApiRuleHelper.php:19-48`).
/// `is_phpstan_namespace` alone is exactly real PHPStan's own
/// `isPhpStanName()` half - correct for a global-namespace or non-
/// PHPStan calling scope (the overwhelming majority of real usage). This
/// adds the vendor-directory half, which only ever changes the answer
/// when the *calling* scope's own namespace is *also* `PHPStan\*`-
/// prefixed - self-referential PHPStan code, or a PHPStan extension
/// package conventionally namespaced `PHPStan\<Vendor>` (phpstan-
/// doctrine, phpstan-symfony, ...). `target_fqn` is assumed to already
/// have passed `is_phpstan_namespace` at every real call site, so the
/// only real branch this ever takes past that is the vendor-evidence
/// check. Deliberately does NOT replicate real PHPStan's own
/// `ParentDirectoryRelativePathHelper` relative-path walk - this project
/// only ever deals with plain on-disk paths, so "does the declaring
/// file's own absolute path have any component literally named `vendor`"
/// is an exact, simpler equivalent for every case that matters here. A
/// free function (not a `Walker`/`Checker` method) so both the standalone
/// `Walker` pass and the main `Checker`'s own `check_phpstan_api_method_
/// call` can share it.
pub(crate) fn is_phpstan_code(
    scope_namespace: &str,
    target_fqn: &str,
    registry: &Registry,
) -> bool {
    if scope_namespace.is_empty() || !is_phpstan_namespace(scope_namespace) {
        return is_phpstan_namespace(target_fqn);
    }
    if !is_phpstan_namespace(target_fqn) {
        return false;
    }
    registry
        .phpstan_class_files
        .get(target_fqn)
        .is_some_and(|path| path.components().any(|c| c.as_os_str() == "vendor"))
}

/// `OldPhpParser4ClassRule`'s own hardcoded rename table -
/// `vendor/phpstan-src/src/Rules/Api/OldPhpParser4ClassRule.php`'s own
/// `NAME_MAPPING` constant, ported verbatim (old nikic/php-parser v4
/// class name, lowercased for the case-insensitive match real PHPStan's
/// own `array_change_key_case` gives it -> new v5 name).
const OLD_PHP_PARSER_4_CLASS_NAMES: &[(&str, &str)] = &[
    (
        "phpparser\\node\\scalar\\lnumber",
        "PhpParser\\Node\\Scalar\\Int_",
    ),
    (
        "phpparser\\node\\scalar\\dnumber",
        "PhpParser\\Node\\Scalar\\Float_",
    ),
    (
        "phpparser\\node\\scalar\\encapsed",
        "PhpParser\\Node\\Scalar\\InterpolatedString",
    ),
    (
        "phpparser\\node\\scalar\\encapsedstringpart",
        "PhpParser\\Node\\InterpolatedStringPart",
    ),
    (
        "phpparser\\node\\expr\\arrayitem",
        "PhpParser\\Node\\ArrayItem",
    ),
    (
        "phpparser\\node\\expr\\closureuse",
        "PhpParser\\Node\\ClosureUse",
    ),
    (
        "phpparser\\node\\stmt\\declaredeclare",
        "PhpParser\\Node\\DeclareItem",
    ),
    (
        "phpparser\\node\\stmt\\propertyproperty",
        "PhpParser\\Node\\PropertyItem",
    ),
    (
        "phpparser\\node\\stmt\\staticvar",
        "PhpParser\\Node\\StaticVar",
    ),
    ("phpparser\\node\\stmt\\useuse", "PhpParser\\Node\\UseItem"),
];

/// The five reflection function names
/// `RuntimeReflectionFunctionRule` flags.
const RUNTIME_REFLECTION_FUNCTIONS: &[&str] = &[
    "is_a",
    "is_subclass_of",
    "class_parents",
    "class_implements",
    "class_uses",
];

/// The thirteen reflection class names
/// `RuntimeReflectionInstantiationRule` flags - `vendor/phpstan-src`'s
/// own `NAME_MAPPING`-adjacent `in_array` list, ported verbatim (two of
/// these, `ReflectionEnum`/`ReflectionEnumBackedCase`/`ReflectionFiber`,
/// are real PHP built-ins with no corresponding phpstorm-stubs `use`
/// import in the original - kept as bare strings there too).
const RUNTIME_REFLECTION_CLASSES: &[&str] = &[
    "ReflectionMethod",
    "ReflectionClass",
    "ReflectionClassConstant",
    "ReflectionEnum",
    "ReflectionEnumBackedCase",
    "ReflectionZendExtension",
    "ReflectionExtension",
    "ReflectionFunction",
    "ReflectionObject",
    "ReflectionParameter",
    "ReflectionProperty",
    "ReflectionGenerator",
    "ReflectionFiber",
];

/// A single class-shaped native type hint (`Param::type_hint`) resolved to
/// a fully-qualified name, or `None` for anything not simple/unambiguous
/// enough to trust: no hint at all, a union/intersection (`Foo|Bar`,
/// `A&B`), or an empty string. A leading `?` (nullable) is stripped first -
/// `?Scope $scope` still names `Scope` unambiguously. No primitive/pseudo-
/// type keyword filter is needed here: `is_phpstan_namespace` already
/// rejects anything a keyword like `self`/`static`/`array`/`int` could
/// ever resolve to, so a keyword slipping through this function costs
/// nothing.
fn resolved_param_class(p: &Param, ctx: &NameContext) -> Option<String> {
    resolved_class_from_hint(p.type_hint.as_deref(), ctx)
}

/// The shared native-hint-resolution core `resolved_param_class` (above)
/// and `class_prop_types` (below) both need.
fn resolved_class_from_hint(hint: Option<&str>, ctx: &NameContext) -> Option<String> {
    let hint = hint?;
    let bare = hint.strip_prefix('?').unwrap_or(hint);
    if bare.is_empty() || bare.contains('|') || bare.contains('&') {
        return None;
    }
    Some(ctx.resolve(bare))
}

/// A minimal, class-scoped sibling to `ParamTypes` - every
/// property (ordinary, or constructor-promoted) whose own native type
/// hint names a single, unambiguous class, keyed by bare property name
/// (no leading `$`). Lets `check_expr`'s `MethodCall` arm recognize one
/// additional common receiver shape real `ApiMethodCallRule`'s own full
/// receiver-type inference would otherwise catch, that a bare
/// `$variable` lookup against `ParamTypes` alone can't:
/// `$this->prop->method()` through an injected/promoted dependency -
/// still not general receiver-type inference (this walker has none, and
/// still doesn't after this), just one more syntactically-typed shape,
/// matching the module's own documented "known, disclosed gap" note on
/// this exact case.
fn class_prop_types(c: &ClassDecl, ctx: &NameContext) -> ParamTypes {
    let mut out = ParamTypes::new();
    for p in &c.properties {
        if let Some(fqn) = resolved_class_from_hint(p.type_hint.as_deref(), ctx) {
            out.insert(p.name.to_string(), fqn);
        }
    }
    if let Some(ctor) = c
        .methods
        .iter()
        .find(|m| m.function.name.eq_ignore_ascii_case("__construct"))
    {
        for p in &ctor.function.params {
            if p.promoted_visibility.is_some()
                && let Some(fqn) = resolved_param_class(p, ctx)
            {
                out.insert(p.name.to_string(), fqn);
            }
        }
    }
    out
}

/// Builds a fresh `ParamTypes` map for one function-like body: every entry
/// from `base` not shadowed by one of `params`' own names, plus a fresh
/// entry (or an explicit removal, if the param's own hint isn't a simple
/// class reference) for each of `params` themselves. Shared by
/// `check_function` (called with an empty `base` - an ordinary function/
/// method has no enclosing lexical scope to inherit from) and the
/// `Closure`/`ArrowFn` arms of `check_expr` (called with whatever the
/// enclosing scope already resolved, filtered down first for `Closure`'s
/// own explicit `use (...)` capture list - PHP's real capture rule for a
/// full closure, unlike an arrow function's own implicit
/// capture-everything-referenced rule, which is why `ArrowFn` passes its
/// *entire* enclosing map through unfiltered instead).
fn param_types_with(base: &ParamTypes, params: &[Param], ctx: &NameContext) -> ParamTypes {
    let mut out = base.clone();
    for p in params {
        out.remove(p.name.as_ref());
        if let Some(fqn) = resolved_param_class(p, ctx) {
            out.insert(p.name.to_string(), fqn);
        }
    }
    out
}

impl<'a> Checker<'a> {
    /// `ApiMethodCallRule`'s general receiver-type-inference
    /// half, folded into the main checker pass - `check_expr`'s own
    /// `MethodCall` arm already computes `owner`/`sig` via full general
    /// inference (`find_method_receiver`) for an unrelated purpose
    /// (member resolution), so this only needs to reuse them, not
    /// recompute anything. Supersedes `Walker::check_expr`'s own
    /// previous, syntactically-typed-only `phpstanApi.method` reporting
    /// (a directly-typed parameter, or `$this->prop`, receiver only) -
    /// this recognizes *every* receiver shape the main checker's own
    /// type inference does. Mirrors `Walker::report_static_method_not_
    /// covered`'s own identical declaring-class + method-level `@api`
    /// resolution (the `__construct` class-level-`@api` exemption
    /// included) - real PHPStan's `isCovered()`, confirmed by reading it
    /// directly.
    pub(crate) fn check_phpstan_api_method_call(
        &mut self,
        owner: &str,
        method: &str,
        sig: &FunctionSig,
        span: Span,
        label: &str,
    ) {
        if !self.registry.phpstan_api_check_enabled
            || sig.is_api
            || !is_phpstan_code(self.name_ctx.namespace(), owner, self.registry)
        {
            return;
        }
        let Some(owner_info) = self.registry.classes.get(owner) else {
            return;
        };
        if owner_info.is_api && method != "__construct" {
            return;
        }
        self.report(
            label.to_string(),
            format!(
                "calling {owner}::{method}() is not covered by PHPStan's backward compatibility promise (missing @api on {owner})"
            ),
            span,
            0,
            "phpstanApi.method",
        );
    }

    /// `NodeConnectingVisitorAttributesRule`, folded in
    /// alongside `check_phpstan_api_method_call` above for the identical
    /// reason - `check_expr`'s own `MethodCall` arm already has the real
    /// receiver class in hand. Unlike that check, gated on the *calling*
    /// class implementing a `PHPStan\*` interface
    /// (`class_implements_phpstan_interface`), not the receiver's own
    /// namespace - `$node` is typically a plain `PhpParser\Node` subtype,
    /// not itself `PHPStan\*`-namespaced, so `is_phpstan_namespace`
    /// would never fire here at all.
    pub(crate) fn check_node_connecting_attribute(
        &mut self,
        recv_class: &str,
        method: &str,
        args: &[Arg],
        span: Span,
        label: &str,
    ) {
        if !self.registry.phpstan_api_check_enabled {
            return;
        }
        if !self
            .current_class
            .as_deref()
            .is_some_and(|c| class_implements_phpstan_interface(c, self.registry))
        {
            return;
        }
        if !method.eq_ignore_ascii_case("getAttribute") {
            return;
        }
        if !self.registry.is_subclass_of(recv_class, "PhpParser\\Node") {
            return;
        }
        let Some(first) = args.first() else { return };
        let Some(name) = literal_string_value(&first.value) else {
            return;
        };
        if !matches!(name.as_ref(), "parent" | "previous" | "next") {
            return;
        }
        self.report_builder(
            crate::checker::DiagnosticBuilder::new(
                label.to_string(),
                format!("Node attribute '{name}' is no longer available."),
                span,
                0,
                "phpParser.nodeConnectingAttribute",
            )
            .tip("See: https://phpstan.org/blog/preprocessing-ast-for-custom-rules"),
        );
    }
}

struct Walker<'a> {
    registry: &'a Registry,
    diagnostics: Vec<Diagnostic>,
    // Set once per `check_class`, from that class's own
    // properties/promoted constructor params (`class_prop_types`) - PHP
    // classes never nest, so there's no enclosing class's own map to
    // save/restore the way `current_function_is_generator` and friends
    // need to in the main `Checker`. Empty outside any class (top-level
    // functions have no `$this` to look anything up through anyway).
    current_prop_types: ParamTypes,
    // Set once per `check_class`, alongside `current_prop_
    // types` above, for the same reason (no nesting, no save/restore
    // needed) - `RuntimeReflectionFunctionRule`/`RuntimeReflection
    // InstantiationRule`/`OldPhpParser4ClassRule` all gate on this exact
    // condition (real PHPStan's own `$scope->isInClass()` plus "does the
    // enclosing class implement a `PHPStan\*` interface").
    current_class_implements_phpstan_interface: bool,
    // Set once per `check_class`, alongside the two fields
    // above (no nesting, no save/restore needed - PHP classes never
    // nest) - `resolve_if_phpstan`'s own `self`/`static`/`parent`
    // pseudo-class resolution needs this; `NameContext::resolve` has no
    // special-casing for any of the three at all (confirmed by its own
    // doc comment), and this standalone `Walker` has no other "current
    // class" state of its own the way the main `Checker` does.
    current_class_fqn: Option<String>,
    // `PhpStanNamespaceIn3rdPartyPackageRule`'s own real file
    // path on disk, used only to find the nearest `composer.json` upward
    // from it. `None` in every test (no real file on disk to point at) -
    // that one rule just never fires without it.
    file_path: Option<&'a Path>,
}

impl<'a> Walker<'a> {
    /// `self`/`static`/`parent` need resolving against
    /// `current_class_fqn` before ever reaching `ctx.resolve` - that
    /// function has no special-casing for any of the three (confirmed by
    /// its own doc comment: "`self`/`parent`/`static`... never reach
    /// here" is a promise the *main* `Checker`'s own callers uphold, not
    /// something `NameContext` itself enforces), so without this,
    /// `parent::someMethod()` inside a class extending a `PHPStan\*` base
    /// silently resolved `"parent"` as a literal, always-unqualified bare
    /// name and never matched `is_phpstan_namespace` at all.
    fn resolve_if_phpstan(&self, name: &str, ctx: &NameContext) -> Option<String> {
        self.resolve_class_name(name, ctx)
            .filter(|fqn| is_phpstan_namespace(fqn))
    }

    /// Same `self`/`static`/`parent`-aware resolution as `resolve_if_
    /// phpstan`, without its own `is_phpstan_namespace` filter - needed
    /// at the `StaticCall` site, which must resolve the FQN
    /// unconditionally: real `ApiStaticCallRule` gates on the resolved
    /// method's declaring class, never on the literally-written class
    /// name's own namespace, so `MyNonPhpStanSubclass::
    /// inheritedPhpStanMethod()` (where `MyNonPhpStanSubclass extends
    /// PHPStan\Foo`) must still reach `report_static_method_not_covered`.
    fn resolve_class_name(&self, name: &str, ctx: &NameContext) -> Option<String> {
        match name {
            "self" | "static" => self.current_class_fqn.clone(),
            "parent" => {
                let current = self.current_class_fqn.as_deref()?;
                self.registry
                    .classes
                    .get(current)?
                    .extends
                    .first()
                    .map(|s| s.to_string())
            }
            _ => Some(ctx.resolve(name)),
        }
    }

    /// Direct port of `ApiRuleHelper::isPhpStanCode`
    /// (`vendor/phpstan-src/src/Rules/Api/ApiRuleHelper.php:19-48`).
    /// `is_phpstan_namespace` alone is exactly real PHPStan's own
    /// `isPhpStanName()` half - correct for a global-namespace or non-
    /// PHPStan calling scope (the overwhelming majority of real usage).
    /// This adds the vendor-directory half, which only ever changes the
    /// answer when the *calling* scope's own namespace is *also*
    /// `PHPStan\*`-prefixed - self-referential PHPStan code, or a PHPStan
    /// extension package conventionally namespaced `PHPStan\<Vendor>`
    /// (phpstan-doctrine, phpstan-symfony, ...). `target_fqn` is assumed
    /// to already have passed `is_phpstan_namespace` (every real call
    /// site here reaches this only via `resolve_if_phpstan`'s own
    /// filter), so the only real branch this ever takes past that is the
    /// vendor-evidence check. Deliberately does NOT replicate real
    /// PHPStan's own `ParentDirectoryRelativePathHelper` relative-path
    /// walk - this project only ever deals with plain on-disk paths, so
    /// "does the declaring file's own absolute path have any component
    /// literally named `vendor`" is an exact, simpler equivalent for
    /// every case that matters here.
    fn is_phpstan_code(&self, scope_namespace: &str, target_fqn: &str) -> bool {
        is_phpstan_code(scope_namespace, target_fqn, self.registry)
    }

    /// `ApiInstanceofTypeRule`, direct port - a wholly
    /// separate rule from `ApiInstanceofRule` above (this one fires
    /// independent of `@api` status entirely, keyed only on
    /// `INSTANCEOF_DEPRECATED_CLASSES`). Two real, narrow exemptions
    /// ported alongside the table itself: the enclosing class
    /// implementing `PHPStan\Type\TypeTraverserCallable` (real
    /// PHPStan's own internal type-traversal callback idiom
    /// legitimately needs this exact pattern), and the target class
    /// itself being `AccessoryType` or one of its own real subtypes
    /// (`ClassReflection::is()`'s self-inclusive semantics, confirmed by
    /// reading it directly - every concrete `Accessory*` type is itself
    /// an `AccessoryType` subtype, so this exemption is real for the
    /// whole family, not just the bare marker interface, however
    /// counterintuitive that looks against the same family's own
    /// individual table entries).
    fn check_instanceof_deprecated(&mut self, fqn: &str, span: Span, label: &str) {
        let Some((_, hint)) = INSTANCEOF_DEPRECATED_CLASSES
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(fqn))
        else {
            return;
        };
        if let Some(current) = &self.current_class_fqn
            && self
                .registry
                .is_subclass_of(current, "PHPStan\\Type\\TypeTraverserCallable")
        {
            return;
        }
        if self
            .registry
            .is_subclass_of(fqn, "PHPStan\\Type\\Accessory\\AccessoryType")
        {
            return;
        }
        let message = match hint {
            Some(h) => {
                format!("Doing instanceof {fqn} is error-prone and deprecated. Use {h} instead.")
            }
            None => format!("Doing instanceof {fqn} is error-prone and deprecated."),
        };
        self.diagnostics.push(Diagnostic {
            location: label.to_string(),
            message,
            span,
            level: 0,
            identifier: "phpstanApi.instanceofType".to_string(),
            tip: Some(
                "Learn more: https://phpstan.org/blog/why-is-instanceof-type-wrong-and-getting-deprecated"
                    .to_string(),
            ),
            non_ignorable: false,
            metadata: HashMap::new(),
            fix: None,
        });
    }

    /// "Referencing this at all requires `@api`" - covers every reference
    /// kind except the inheritance-shaped ones (`report_inheritance`
    /// below handles those, since an `@api-do-not-implement` class is
    /// still perfectly fine to *call*, just not to `extends`/
    /// `implements`/`use`).
    /// `identifier` matches real PHPStan's own `Rules\Api\*` identifier
    /// strings one-for-one (see e.g. `ApiMethodCallRule`'s
    /// `phpstanApi.method`) - threaded in by each call site below rather
    /// than derived here, since the identifier depends on the reference
    /// *kind* (method call vs. instantiation vs. instanceof, ...), not on
    /// anything `report_not_covered` itself knows.
    fn report_not_covered(
        &mut self,
        fqn: &str,
        action: &str,
        span: Span,
        label: &str,
        identifier: &str,
        ctx: &NameContext,
    ) {
        if !self.is_phpstan_code(ctx.namespace(), fqn) {
            return;
        }
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        if info.is_api {
            return;
        }
        self.diagnostics.push(Diagnostic {
            location: label.to_string(),
            message: format!(
                "{action} is not covered by PHPStan's backward compatibility promise (missing @api on {fqn})"
            ),
            span,
            level: 0,
            identifier: identifier.to_string(),
            tip: None,
            non_ignorable: false,
            metadata: HashMap::new(),
            fix: None,
        });
    }

    /// `ApiStaticCallRule::isCovered()`'s real resolution -
    /// the referenced class's own `is_api` isn't the right question at
    /// all: a static call is covered when either the method's own
    /// declaring class carries `@api` (except for `__construct`, which
    /// real PHPStan explicitly exempts from this half - a constructor is
    /// never covered by a class-level `@api` alone) or the method's own
    /// docblock carries `@api` directly, regardless of `__construct`.
    /// `lookup_method_with_owner` (already exists, `registry/
    /// hierarchy.rs`) gives both the real declaring class and its
    /// resolved `FunctionSig.is_api` in one call - a referenced class
    /// inheriting a method from an unrelated, non-`@api` ancestor must
    /// not silently pass just because the referenced class's own
    /// `is_api` happens to be true. The declaring class
    /// itself must also genuinely be PHPStan-namespaced code (not just
    /// the referenced one) - the same `is_phpstan_namespace` gate every
    /// other check here already applies, re-checked against `owner`
    /// specifically since inheriting a method from an unrelated,
    /// non-PHPStan base class is real and shouldn't be flagged at all. A
    /// method the registry has no record of at all (a dynamic/unresolved
    /// call) is silently skipped, matching real PHPStan's own
    /// `!hasNativeMethod` bail.
    fn report_static_method_not_covered(
        &mut self,
        class: &str,
        method: &str,
        span: Span,
        label: &str,
        ctx: &NameContext,
    ) {
        let Some((owner, sig)) = self.registry.lookup_method_with_owner(class, method) else {
            return;
        };
        if sig.is_api {
            return;
        }
        if !self.is_phpstan_code(ctx.namespace(), &owner) {
            return;
        }
        let Some(owner_info) = self.registry.classes.get(&owner) else {
            return;
        };
        if owner_info.is_api && method != "__construct" {
            return;
        }
        let action = format!("calling {owner}::{method}()");
        self.diagnostics.push(Diagnostic {
            location: label.to_string(),
            message: format!(
                "{action} is not covered by PHPStan's backward compatibility promise (missing @api on {owner})"
            ),
            span,
            level: 0,
            identifier: "phpstanApi.method".to_string(),
            tip: None,
            non_ignorable: false,
            metadata: HashMap::new(),
            fix: None,
        });
    }

    /// An unconditional report with no `@api` short-
    /// circuit at all - real PHPStan's own `ApiInstantiationRule` (once
    /// the constructor-own-`@api` exemption has already been checked by
    /// the caller) and `ApiTraitUseRule` (no exemption of any kind) both
    /// need this shape, unlike `report_not_covered`'s own class-level-
    /// `@api`-checks-out early return.
    fn report_always(
        &mut self,
        fqn: &str,
        action: &str,
        span: Span,
        label: &str,
        identifier: &str,
        ctx: &NameContext,
    ) {
        if !self.is_phpstan_code(ctx.namespace(), fqn) {
            return;
        }
        self.diagnostics.push(Diagnostic {
            location: label.to_string(),
            message: format!(
                "{action} is not covered by PHPStan's backward compatibility promise (missing @api on {fqn})"
            ),
            span,
            level: 0,
            identifier: identifier.to_string(),
            tip: None,
            non_ignorable: false,
            metadata: HashMap::new(),
            fix: None,
        });
    }

    /// `check_do_not_implement`: real PHPStan's own `@api-do-not-implement`
    /// short-circuit is semantically interface-only ("don't implement/
    /// extend me" - `@api-do-not-implement` is meaningless for the
    /// "extending a class" relationship real `ApiClassExtendsRule` checks:
    /// that rule reads only `isApi()`, never
    /// `isApiDoNotImplement()`). `false` for a class's own `extends`
    /// call site; `true` everywhere else (interface
    /// `extends`, `implements`, and this function's own general shape).
    /// `ctx` (needed for the vendor-directory `is_phpstan_code` check)
    /// pushed this past clippy's default 7-argument threshold - every
    /// argument here is already its own independently-meaningful piece
    /// of this one report site's own shape, not a bag of fields crying
    /// out for a struct (mirrors this project's own existing precedent
    /// for the identical situation, `main/check_project.rs::run_check_project`).
    #[allow(clippy::too_many_arguments)]
    fn report_inheritance(
        &mut self,
        fqn: &str,
        action: &str,
        span: Span,
        label: &str,
        identifier: &str,
        check_do_not_implement: bool,
        ctx: &NameContext,
    ) {
        if !self.is_phpstan_code(ctx.namespace(), fqn) {
            return;
        }
        let Some(info) = self.registry.classes.get(fqn) else {
            return;
        };
        if check_do_not_implement && info.is_api_do_not_implement {
            self.diagnostics.push(Diagnostic {
                location: label.to_string(),
                message: format!(
                    "{action} is not supported: {fqn} is marked @api-do-not-implement and may gain new members in a minor version"
                ),
                span,
                level: 0,
                identifier: identifier.to_string(),
                tip: None,
                non_ignorable: false,
                metadata: HashMap::new(),
                fix: None,
            });
            return;
        }
        if !info.is_api {
            self.diagnostics.push(Diagnostic {
                location: label.to_string(),
                message: format!(
                    "{action} is not covered by PHPStan's backward compatibility promise (missing @api on {fqn})"
                ),
                span,
                level: 0,
                identifier: identifier.to_string(),
                tip: None,
                non_ignorable: false,
                metadata: HashMap::new(),
                fix: None,
            });
        }
    }

    fn check_function(&mut self, f: &FunctionDecl, ctx: &NameContext, label: &str) {
        let param_types = param_types_with(&ParamTypes::new(), &f.params, ctx);
        for s in f.body.iter() {
            self.check_stmt(s, ctx, &param_types, label);
        }
        for p in &f.params {
            if let Some(d) = &p.default {
                self.check_expr(d, ctx, &param_types, label);
            }
        }
    }

    fn check_class(&mut self, c: &ClassDecl, ctx: &NameContext) {
        let fqn = ctx.declared_name(&c.name);
        for parent in &c.extends {
            if let Some(pfqn) = self.resolve_if_phpstan(parent, ctx) {
                // Real `ApiClassExtendsRule`'s own narrow
                // carve-out - extending `PHPStan\Analyser\MutatingScope`
                // itself is exempt, a `Class_`-only concept (an interface's
                // `extends` list is never checked against this, and
                // couldn't target `MutatingScope` meaningfully anyway -
                // it isn't an interface).
                if c.kind == ClassKind::Class && pfqn == "PHPStan\\Analyser\\MutatingScope" {
                    continue;
                }
                let action = format!("extending {pfqn}");
                // Real PHPStan splits this into two separate
                // rules by the declaring node's own kind -
                // `ApiClassExtendsRule` (`phpstanApi.class`, `Class_`-only,
                // single parent) vs `ApiInterfaceExtendsRule`
                // (`phpstanApi.interface`, `Interface_`-only, a real
                // multi-parent list) - `c.kind` already tells us which.
                // `@api-do-not-implement` is semantically
                // interface-only - real `ApiClassExtendsRule` checks only
                // `@api`, never it, for a real class's own `extends`.
                let identifier = if c.kind == ClassKind::Class {
                    "phpstanApi.class"
                } else {
                    "phpstanApi.interface"
                };
                self.report_inheritance(
                    &pfqn,
                    &action,
                    c.span,
                    &fqn,
                    identifier,
                    c.kind != ClassKind::Class,
                    ctx,
                );
            }
        }
        for iface in &c.implements {
            if let Some(ifqn) = self.resolve_if_phpstan(iface, ctx) {
                let action = format!("implementing {ifqn}");
                self.report_inheritance(
                    &ifqn,
                    &action,
                    c.span,
                    &fqn,
                    "phpstanApi.interface",
                    true,
                    ctx,
                );
            }
        }
        for used in &c.uses {
            if let Some(ufqn) = self.resolve_if_phpstan(used, ctx)
                && self.registry.classes.contains_key(ufqn.as_str())
            {
                // Real `ApiTraitUseRule` flags trait use
                // unconditionally - no `@api`/`@api-do-not-implement`
                // exemption of any kind, unlike extends/implements.
                let action = format!("using trait {ufqn}");
                self.report_always(&ufqn, &action, c.span, &fqn, "phpstanApi.trait", ctx);
            }
        }
        let empty = ParamTypes::new();
        for p in &c.properties {
            if let Some(d) = &p.default {
                self.check_expr(d, ctx, &empty, &fqn);
            }
        }
        for cst in &c.consts {
            self.check_expr(&cst.value, ctx, &empty, &fqn);
        }
        self.current_prop_types = class_prop_types(c, ctx);
        self.current_class_implements_phpstan_interface =
            class_implements_phpstan_interface(&fqn, self.registry);
        self.current_class_fqn = Some(fqn.clone());
        for m in &c.methods {
            let method_label = format!("{fqn}::{}", m.function.name);
            self.check_function(&m.function, ctx, &method_label);
        }
        self.current_prop_types = ParamTypes::new();
        self.current_class_implements_phpstan_interface = false;
        self.current_class_fqn = None;
    }

    fn check_enum(&mut self, e: &EnumDecl, ctx: &NameContext) {
        let fqn = ctx.declared_name(&e.name);
        for iface in &e.implements {
            if let Some(ifqn) = self.resolve_if_phpstan(iface, ctx) {
                let action = format!("implementing {ifqn}");
                self.report_inheritance(
                    &ifqn,
                    &action,
                    e.span,
                    &fqn,
                    "phpstanApi.interface",
                    true,
                    ctx,
                );
            }
        }
        for used in &e.uses {
            if let Some(ufqn) = self.resolve_if_phpstan(used, ctx)
                && self.registry.classes.contains_key(ufqn.as_str())
            {
                let action = format!("using trait {ufqn}");
                self.report_always(&ufqn, &action, e.span, &fqn, "phpstanApi.trait", ctx);
            }
        }
        let empty = ParamTypes::new();
        for cst in &e.consts {
            self.check_expr(&cst.value, ctx, &empty, &fqn);
        }
        for case in &e.cases {
            if let Some(v) = &case.value {
                self.check_expr(v, ctx, &empty, &fqn);
            }
        }
        self.current_class_implements_phpstan_interface =
            class_implements_phpstan_interface(&fqn, self.registry);
        self.current_class_fqn = Some(fqn.clone());
        for m in &e.methods {
            let method_label = format!("{fqn}::{}", m.function.name);
            self.check_function(&m.function, ctx, &method_label);
        }
        self.current_class_implements_phpstan_interface = false;
        self.current_class_fqn = None;
    }

    /// `PhpStanNamespaceIn3rdPartyPackageRule`, direct port
    /// (`vendor/phpstan-src/src/Rules/Api/
    /// PhpStanNamespaceIn3rdPartyPackageRule.php`). Unlike the other two
    /// rules in this phase, not gated on `current_class_implements_
    /// phpstan_interface` at all - a bare `namespace PHPStan\...;`
    /// declaration is the thing being checked, independent of whatever
    /// classes the file goes on to declare inside it.
    fn check_namespace_declaration(&mut self, ns: &str, span: Span, label: &str) {
        if !is_phpstan_namespace(ns) {
            return;
        }
        let Some(dir) = self.file_path.and_then(Path::parent) else {
            return;
        };
        if let Some(package_name) = composer::find_composer_package_name(dir)
            && package_name.starts_with("phpstan/")
        {
            return;
        }
        self.diagnostics.push(Diagnostic {
            location: label.to_string(),
            message: "Declaring PHPStan namespace is not allowed in 3rd party packages."
                .to_string(),
            span,
            level: 0,
            identifier: "phpstanApi.phpstanNamespace".to_string(),
            tip: Some(
                "See:\n   https://phpstan.org/developing-extensions/backward-compatibility-promise"
                    .to_string(),
            ),
            non_ignorable: false,
            metadata: HashMap::new(),
            fix: None,
        });
    }

    /// `OldPhpParser4ClassRule`'s own reporting half - the
    /// name-mapping lookup itself lives in `OLD_PHP_PARSER_4_CLASS_NAMES`.
    /// `name` is the as-written reference text (matches real PHPStan's
    /// own `$node->toString()` in the reported message).
    fn check_old_php_parser4_class(&mut self, name: &str, span: Span, label: &str) {
        if !self.current_class_implements_phpstan_interface {
            return;
        }
        let lower = name.trim_start_matches('\\').to_ascii_lowercase();
        let Some((_, new_name)) = OLD_PHP_PARSER_4_CLASS_NAMES
            .iter()
            .find(|(old, _)| *old == lower)
        else {
            return;
        };
        self.diagnostics.push(Diagnostic {
            location: label.to_string(),
            message: format!(
                "Class {name} not found. It has been renamed to {new_name} in PHP-Parser v5."
            ),
            span,
            level: 0,
            identifier: "phpParser.classRenamed".to_string(),
            tip: None,
            non_ignorable: false,
            metadata: HashMap::new(),
            fix: None,
        });
    }

    /// `RuntimeReflectionFunctionRule`'s own reporting half.
    fn check_runtime_reflection_function(&mut self, name: &str, span: Span, label: &str) {
        if !self.current_class_implements_phpstan_interface {
            return;
        }
        let bare = name.rsplit('\\').next().unwrap_or(name);
        let Some(matched) = RUNTIME_REFLECTION_FUNCTIONS
            .iter()
            .find(|f| bare.eq_ignore_ascii_case(f))
        else {
            return;
        };
        self.diagnostics.push(Diagnostic {
            location: label.to_string(),
            message: format!(
                "Function {matched}() is a runtime reflection concept that might not work in PHPStan because it uses fully static reflection engine. Use objects retrieved from ReflectionProvider instead."
            ),
            span,
            level: 0,
            identifier: "phpstanApi.runtimeReflection".to_string(),
            tip: None,
            non_ignorable: false,
            metadata: HashMap::new(),
            fix: None,
        });
    }

    /// `RuntimeReflectionInstantiationRule`'s own reporting half.
    fn check_runtime_reflection_instantiation(
        &mut self,
        name: &str,
        ctx: &NameContext,
        span: Span,
        label: &str,
    ) {
        if !self.current_class_implements_phpstan_interface {
            return;
        }
        let resolved = ctx.resolve(name);
        let Some(matched) = RUNTIME_REFLECTION_CLASSES
            .iter()
            .find(|c| resolved.eq_ignore_ascii_case(c))
        else {
            return;
        };
        self.diagnostics.push(Diagnostic {
            location: label.to_string(),
            message: format!(
                "Creating new {matched} is a runtime reflection concept that might not work in PHPStan because it uses fully static reflection engine. Use objects retrieved from ReflectionProvider instead."
            ),
            span,
            level: 0,
            identifier: "phpstanApi.runtimeReflection".to_string(),
            tip: None,
            non_ignorable: false,
            metadata: HashMap::new(),
            fix: None,
        });
    }

    /// Mirrors `deps::collect_stmt`'s exact shape - see that function's
    /// own doc comment for why the recursion is split this way
    /// (`stmt_children` for nested *statements*, this match for a
    /// variant's own non-statement fields, with `FunctionDecl`/
    /// `ClassDecl`/`EnumDecl` needing both since they have their own
    /// dedicated declaration-walker instead of being flattened into an
    /// ordinary statement list).
    fn check_stmt(
        &mut self,
        stmt: &Stmt,
        ctx: &NameContext,
        param_types: &ParamTypes,
        label: &str,
    ) {
        match &stmt.kind {
            StmtKind::Block(_)
            | StmtKind::InlineHtml(_)
            | StmtKind::Break(_)
            | StmtKind::Continue(_)
            | StmtKind::Use(_)
            | StmtKind::Global(_)
            | StmtKind::Goto(_)
            | StmtKind::Label(_)
            | StmtKind::Declare { .. } => {}
            StmtKind::Namespace(ns) => self.check_namespace_declaration(ns, stmt.span, label),
            StmtKind::Expression(e) | StmtKind::Throw(e) => {
                self.check_expr(e, ctx, param_types, label)
            }
            StmtKind::Return(e) => {
                if let Some(e) = e {
                    self.check_expr(e, ctx, param_types, label);
                }
            }
            StmtKind::Echo(exprs) => {
                for e in exprs {
                    self.check_expr(e, ctx, param_types, label);
                }
            }
            StmtKind::If { cond, .. }
            | StmtKind::While { cond, .. }
            | StmtKind::DoWhile { cond, .. } => {
                self.check_expr(cond, ctx, param_types, label);
            }
            StmtKind::For {
                init, cond, step, ..
            } => {
                for e in init.iter().chain(cond).chain(step) {
                    self.check_expr(e, ctx, param_types, label);
                }
            }
            StmtKind::Foreach {
                subject,
                key,
                value,
                ..
            } => {
                self.check_expr(subject, ctx, param_types, label);
                if let Some(k) = key {
                    self.check_expr(k, ctx, param_types, label);
                }
                self.check_expr(value, ctx, param_types, label);
            }
            StmtKind::FunctionDecl(f) => self.check_function(f, ctx, &f.name),
            StmtKind::ClassDecl(c) => self.check_class(c, ctx),
            StmtKind::EnumDecl(e) => self.check_enum(e, ctx),
            StmtKind::GlobalConst(consts) => {
                for c in consts {
                    self.check_expr(&c.value, ctx, param_types, label);
                }
            }
            StmtKind::StaticVar(vars) => {
                for (_, d) in vars {
                    if let Some(e) = d {
                        self.check_expr(e, ctx, param_types, label);
                    }
                }
            }
            StmtKind::Switch { subject, cases } => {
                self.check_expr(subject, ctx, param_types, label);
                for case in cases {
                    if let Some(t) = &case.test {
                        self.check_expr(t, ctx, param_types, label);
                    }
                }
            }
            StmtKind::Try { .. } => {}
        }
        for child in stmt_children(stmt) {
            self.check_stmt(child, ctx, param_types, label);
        }
    }

    /// Mirrors `deps::collect_expr`'s exact shape - see that function's
    /// own doc comment. Each name-bearing reference kind is checked
    /// directly against `self.registry`'s `@api`/`@api-do-not-implement`
    /// state (never generically recursed into via `expr_children` a
    /// second time for the same span - every arm below either fully
    /// covers its own children by hand, matching what `expr_children`
    /// would otherwise return, or falls through to the catch-all).
    fn check_expr(&mut self, e: &Expr, ctx: &NameContext, param_types: &ParamTypes, label: &str) {
        match &e.kind {
            ExprKind::Call(callee, args) => {
                if let ExprKind::Name(n) = &callee.kind {
                    self.check_runtime_reflection_function(n, e.span, label);
                }
                self.check_expr(callee, ctx, param_types, label);
                for a in args {
                    self.check_expr(&a.value, ctx, param_types, label);
                }
            }
            ExprKind::New(callee, args) => {
                if let ExprKind::Name(n) = &callee.kind {
                    self.check_old_php_parser4_class(n, e.span, label);
                    self.check_runtime_reflection_instantiation(n, ctx, e.span, label);
                    if let Some(fqn) = self.resolve_if_phpstan(n, ctx)
                        && let Some(info) = self.registry.classes.get(fqn.as_str())
                    {
                        // Real PHPStan's own `ApiInstantiationRule`
                        // flags instantiation unconditionally by default - a
                        // class-level `@api` does NOT exempt it at all. The
                        // only exemption: the class's own, non-inherited
                        // constructor (a direct `info.methods.get` lookup,
                        // not `lookup_method`, since an inherited
                        // constructor doesn't count) itself carries `@api`.
                        let has_own_api_constructor = info
                            .methods
                            .get("__construct")
                            .is_some_and(|ctor| ctor.is_api);
                        if !has_own_api_constructor {
                            let action = format!("instantiating {fqn}");
                            self.report_always(
                                &fqn,
                                &action,
                                e.span,
                                label,
                                "phpstanApi.constructor",
                                ctx,
                            );
                        }
                    }
                } else {
                    self.check_expr(callee, ctx, param_types, label);
                }
                for a in args {
                    self.check_expr(&a.value, ctx, param_types, label);
                }
            }
            ExprKind::StaticCall(target, method, args) => {
                // Resolved unconditionally, NOT
                // pre-filtered through `resolve_if_phpstan` - see
                // `resolve_class_name`'s own doc comment for why.
                // `report_static_method_not_covered` does its own
                // `is_phpstan_code` gating against the method's real
                // *declaring* class once it looks it up. A genuinely
                // dynamic receiver has no name to resolve here (this
                // pass has no type-inference machinery of its own to
                // fall back on, unlike `check_static_call`'s own real
                // resolution) - `unwrap_or("?")` matches the identical
                // "unresolvable" outcome the parser's old permanent `"?"`
                // sentinel already produced for this exact case.
                if let Some(fqn) = self.resolve_class_name(target.as_name().unwrap_or("?"), ctx) {
                    self.report_static_method_not_covered(&fqn, method, e.span, label, ctx);
                }
                for a in args {
                    self.check_expr(&a.value, ctx, param_types, label);
                }
            }
            // `X::class` is deliberately exempt from the ordinary `@api`
            // class-constant check - see this module's own doc comment
            // for why - but is still a real bare name reference, so
            // `OldPhpParser4ClassRule` still applies to it.
            ExprKind::StaticAccess(class_name, member) if member == "class" => {
                self.check_old_php_parser4_class(class_name, e.span, label);
            }
            ExprKind::StaticAccess(class_name, member) if member != "class" => {
                if let Some(fqn) = self.resolve_if_phpstan(class_name, ctx) {
                    let action = format!("accessing {fqn}::{member}");
                    self.report_not_covered(
                        &fqn,
                        &action,
                        e.span,
                        label,
                        "phpstanApi.classConstant",
                        ctx,
                    );
                }
            }
            // `phpstanApi.method`/`phpParser.
            // nodeConnectingAttribute` reporting for a method call is
            // handled by the main `Checker` pass (`checker/expr.rs`'s own
            // `MethodCall` arm, via `Checker::check_phpstan_api_method_
            // call`/`check_node_connecting_attribute`) - general receiver-
            // type inference there now fully subsumes this walker's own
            // previous syntactically-typed-only recognition (a directly-
            // typed parameter, or `$this->prop`). Still recurses into the
            // receiver/args, since other reference kinds this walker
            // covers (`new X()`, `X::staticMethod()`, ...) can appear
            // nested inside either.
            ExprKind::MethodCall(recv, _method, args, _) => {
                self.check_expr(recv, ctx, param_types, label);
                for a in args {
                    self.check_expr(&a.value, ctx, param_types, label);
                }
            }
            ExprKind::Instanceof(inner, InstanceofTarget::Name(n, _)) => {
                self.check_old_php_parser4_class(n, e.span, label);
                // Real PHPStan's own `ApiInstanceofRule` - two
                // genuinely different diagnostics depending on whether the
                // class is `@api` at all, not one identifier reused for
                // both. A non-`@api` class is flagged with a kind-specific
                // identifier and the ordinary "not covered" message; an
                // `@api` class is still flagged (unless it's `PHPStan\
                // Type\Type` itself or an interface) with a different
                // identifier/message warning that the instanceof
                // assumption itself might break in a minor version. This
                // project has no live type-narrowing pass available to
                // this separate walker, so the "already provably true/
                // false"/"static type already narrows" suppressions real
                // PHPStan also applies aren't replicated here.
                if let Some(fqn) = self.resolve_if_phpstan(n, ctx) {
                    // Independent of `@api` status entirely -
                    // see `check_instanceof_deprecated`'s own doc comment.
                    self.check_instanceof_deprecated(&fqn, e.span, label);
                }
                if let Some(fqn) = self.resolve_if_phpstan(n, ctx)
                    && let Some(info) = self.registry.classes.get(fqn.as_str())
                {
                    if !info.is_api {
                        let action = format!("instanceof {fqn}");
                        let identifier =
                            format!("phpstanApi.{}", class_type_word(info.kind).to_lowercase());
                        self.report_always(&fqn, &action, e.span, label, &identifier, ctx);
                    } else if fqn != "PHPStan\\Type\\Type" && info.kind != ClassKind::Interface {
                        self.diagnostics.push(Diagnostic {
                            location: label.to_string(),
                            message: format!(
                                "Although {fqn} is covered by backward compatibility promise, this instanceof assumption might break in a minor PHPStan version."
                            ),
                            span: e.span,
                            level: 0,
                            identifier: "phpstanApi.instanceofAssumption".to_string(),
                            tip: None,
                            non_ignorable: false,
                            metadata: HashMap::new(),
                            fix: None,
                        });
                    }
                }
                self.check_expr(inner, ctx, param_types, label);
            }
            ExprKind::NewAnonymousClass(decl, args) => {
                self.check_class(decl, ctx);
                for a in args {
                    self.check_expr(&a.value, ctx, param_types, label);
                }
            }
            ExprKind::Closure {
                params, uses, body, ..
            } => {
                let captured: ParamTypes = uses
                    .iter()
                    .filter_map(|u| {
                        param_types
                            .get(u.name.as_ref())
                            .map(|ty| (u.name.to_string(), ty.clone()))
                    })
                    .collect();
                let nested = param_types_with(&captured, params, ctx);
                for s in body {
                    self.check_stmt(s, ctx, &nested, label);
                }
            }
            ExprKind::ArrowFn { params, body, .. } => {
                let nested = param_types_with(param_types, params, ctx);
                self.check_expr(body, ctx, &nested, label);
            }
            _ => {
                for child in expr_children(e) {
                    self.check_expr(child, ctx, param_types, label);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
