//! Generates built-in function signatures by parsing real stub sources
//! with this project's own lexer/parser/registry, rather than
//! hand-transcribing approximations one function at a time. A hand-
//! curated overlay (`stubs.rs`) existed alongside this before Phase 8
//! ingested the real stub corpus below, but every name it covered turned
//! out to already be present here too (`Registry::merge_stubs`'s
//! first-registered-wins semantics meant the overlay had already become
//! dead weight, not an active correction layer) - removed rather than
//! kept as a doc-comment inaccuracy waiting to mislead the next person
//! who reads it (Finding 5 from the architecture audit).
//!
//! The vendored corpus, under `vendor/phpstorm-stubs/`, is
//! `jetbrains/phpstorm-stubs` at commit
//! 748ab87d16253a5b5d648b5fe4dae1ff4152bb03 (Apache-2.0 - see
//! `vendor/phpstorm-stubs/LICENSE`) - the stub source real PHPStan itself
//! depends on, not php-src's own `*.stub.php` files (an earlier version
//! of this pipeline vendored two hand-picked php-src extension stubs
//! instead; php-src's stubs turned out to be the wrong target, since real
//! PHPStan doesn't use them). Of phpstorm-stubs' 1052 `.php` files, 558
//! are vendored here: its own `tests/` (481 files) and a handful of
//! non-stub housekeeping files (the generated `PhpStormStubsMap.php`, 11
//! `.phpstorm.meta.php` files, and its `.php-cs-fixer.php` config) are
//! excluded, since none of them contribute function signatures and the
//! map/meta files would otherwise bloat the vendored tree by nearly 1MB
//! for zero benefit. `build.rs` generates `PHPSTORM_STUBS_SOURCES` (a
//! `&[&str]` of every vendored file's contents) at compile time, so this
//! file doesn't need 558 hand-written `include_str!` calls.
//!
//! A real implementation would refresh the vendored snapshot
//! automatically at build time against a pinned tag; here it's a fixed,
//! manually-refreshed one.
//!
//! `PHPSTAN_SRC_SOURCES` (Finding 4): a second vendored corpus,
//! `vendor/phpstan-src/src/` - real PHPStan's own MIT-licensed runtime
//! source (already vendored in this project, previously only used to
//! derive test fixtures - see the many `// Derived from vendor/
//! phpstan-src/tests/...` comments throughout `checker.rs`), reused here
//! for the identical reason `phpstorm-stubs` is: a real corpus vendor
//! package authoring a PHPStan rule/extension references `PHPStan\Type\*`/
//! `PHPStan\Rules\Rule`/etc., none of which exist as scannable source in
//! `phpstan/phpstan`'s own phar distribution. `generated_stubs`/
//! `generated_stub_classes` chain `PHPSTAN_SRC_SOURCES` in after
//! `PHPSTORM_STUBS_SOURCES`, so real PHP builtins keep their existing
//! first-occurrence-wins priority - phpstan-src can only add names, never
//! take priority over one phpstorm-stubs already claimed. Confirmed
//! before landing this: `phpstan-rs check vendor/phpstan-src/src` (0 of
//! 1793 files failed to parse) and `phpstan-rs check-project
//! vendor/phpstan-src/src` (the full `Registry::build`/`ClassInfo::
//! from_decl` path this pipeline actually exercises, a strict superset of
//! what stub generation needs) both completed cleanly with no panic. Not
//! closed by this: `vendor/phpstan-src/src` itself references classes
//! from its own dependency, `nikic/php-doc-parser` (`PHPStan\PhpDocParser//! Ast\Type\*`), not vendored here - a smaller, disclosed residual (see
//! `ROADMAP.md`'s "Known limitations").

use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::registry::{ClassInfo, FunctionSig, Registry, register_global_constant};
use crate::types::{AccessoryKind, TemplateParam, Type};

include!(concat!(env!("OUT_DIR"), "/phpstorm_stubs_generated.rs"));

/// Parses one stub file's source into `(name, FunctionSig)` pairs,
/// reusing the exact same pipeline real project source goes through -
/// stub files are just PHP with empty function bodies, so nothing new was
/// needed to read them.
pub fn generate_from_source(source: &str) -> Vec<(String, FunctionSig)> {
    let tokens = Lexer::new(source).tokenize_with_spans();
    let program = Parser::new(tokens)
        .parse_program()
        .expect("vendored phpstorm-stubs source failed to parse");
    let registry = Registry::index_and_flatten(&program);
    registry
        .functions
        .into_iter()
        .map(|(name, mut sig)| {
            // See `FunctionSig::is_stub`'s own doc comment: a stub file's
            // own body is never real - `Registry::index_and_flatten` has
            // no way to know that on its own, so this is set here
            // instead, the one place that does.
            sig.is_stub = true;
            (name, sig)
        })
        .collect()
}

/// Phase 89: phpstorm-stubs' own real (and, per real PHP's own C-level `arg_info`,
/// genuinely accurate) declaration of `array_multisort`'s variadic tail
/// is `&...$rest` - every "rest" argument marked by-reference uniformly.
/// Real PHP's own actual runtime behavior for this specific function is
/// looser than that declaration suggests, though: a call like
/// `array_multisort($a, SORT_DESC, SORT_NUMERIC, $b)` legitimately
/// passes literal sort-order/sort-flag constants into by-ref-declared
/// positions, which every *other* by-ref parameter in the entire
/// language would reject outright ("Only variables can be passed by
/// reference"). Real PHPStan's own bundled `resources/functionMap.php`
/// (confirmed directly) declares this same function with a *plain*,
/// non-`&`-prefixed signature (`SignatureMapParser`'s own convention:
/// a `&`-prefixed parameter *name* is what marks by-ref, and this
/// project's entry has none) - i.e. real PHPStan deliberately treats
/// none of `array_multisort`'s arguments as reference-checked at all,
/// the exact trade-off (losing precision on the genuinely-by-ref array
/// slots to avoid a false positive on the interleaved sort-flag ones)
/// mirrored here. This project has no general hand-curated stub-overlay
/// mechanism any more (see this module's own doc comment - the last one
/// was dead code, removed before Phase 8) - a single, narrow, explicitly
/// justified post-generation patch for the one function this project's
/// own real corpus actually evidenced the gap on is more honest than
/// reintroducing a whole overlay system for it.
fn patch_array_multisort_by_ref(stubs: &mut [(String, FunctionSig)]) {
    for (name, sig) in stubs.iter_mut() {
        if name != "array_multisort" {
            continue;
        }
        for param in &mut sig.params {
            param.by_ref = false;
        }
    }
}

/// `debug_backtrace()`'s own phpstorm-stubs signature is a bare native
/// `array` (no refining PHPDoc tag) - real PHPStan hardcodes the same
/// `list<array{function: string, ...}>` shape `Exception::getTrace()`
/// shares (`backtrace_frame_list_type`, `patch_reflection_and_exception_
/// list_types`'s own doc comment), confirmed via `\PHPStan\dumpType()`.
fn patch_debug_backtrace(stubs: &mut [(String, FunctionSig)]) {
    for (name, sig) in stubs.iter_mut() {
        if name == "debug_backtrace" {
            sig.return_type = backtrace_frame_list_type();
        }
    }
}

/// `usleep()`'s own phpstorm-stubs PHPDoc refines its native `int`
/// parameter to `int<0,max>` - real PHP's own C-level `arg_info` has no
/// such refinement, so this is phpstorm-stubs' own text going further
/// than the runtime signature itself, the same situation `patch_array_
/// multisort_by_ref` above documents in general. Real PHPStan's own
/// `resources/functionMap.php` declares `usleep` with a *plain* `int`
/// parameter (no range) - and, confirmed directly via `\PHPStan\
/// dumpType()`/a real `vendor/bin/phpstan analyse` run at level 10 with
/// `bleedingEdge.neon`, neither an unconstrained `int` variable nor a
/// literal `-5` passed to `usleep()` is flagged at all. `NativeFunction
/// ReflectionProvider::findFunctionReflection` (`vendor/phpstan-src/src/
/// Reflection/SignatureMap/NativeFunctionReflectionProvider.php`)
/// explains why: any function `functionMap.php` already covers (the
/// overwhelming majority of internal functions, `usleep` included) never
/// even reaches phpstorm-stubs' own PHPDoc for its native parameter
/// types - only PHPStan's own separate, much smaller internal `stubs/`
/// bundle (a distinct resource from phpstorm-stubs) can refine a
/// `functionMap.php`-covered signature further, and it has no entry for
/// `usleep`. Real Piwigo corpus false positive this fixes:
/// `RegisterControllerTest::registerMaturedKey`'s own `usleep((int)
/// ($remainingSeconds * 1_000_000))` inside an `if ($remainingSeconds >
/// 0)` guard - flagged here because this project has no float-range-
/// narrowing mechanism to prove the cast's own result is non-negative,
/// a gap that's moot once `usleep`'s own parameter is corrected to match
/// what real PHPStan actually checks against.
///
/// This is likely one instance of a broader, unconfirmed pattern (any
/// `functionMap.php`-covered function whose phpstorm-stubs PHPDoc adds a
/// refinement `functionMap.php` itself doesn't have could be equally
/// unenforced by real PHPStan) - not chased further without more direct
/// corpus evidence, the same narrow, one-function-at-a-time discipline
/// `patch_array_multisort_by_ref` above already established rather than
/// reintroducing a whole stub-overlay system for a still-hypothetical
/// wider gap.
fn patch_usleep(stubs: &mut [(String, FunctionSig)]) {
    for (name, sig) in stubs.iter_mut() {
        if name == "usleep" {
            for param in &mut sig.params {
                param.ty = Type::Int;
            }
        }
    }
}

/// All signatures generated from the vendored phpstorm-stubs sources,
/// merged (first occurrence wins). A few functions are declared more than
/// once across the corpus (e.g. a version-conditional pair of signatures
/// for the same function in different files); taking the first is a
/// simplification, not a precision-critical choice, since `stubs.rs`'s
/// hand-curated overlay is what wins for anything precision-sensitive.
pub fn generated_stubs() -> Vec<(String, FunctionSig)> {
    // Chaining PHPSTAN_SRC_SOURCES in (Finding 4) took the combined
    // corpus from 558 to 2351 files - re-parsing that from scratch on
    // every call (as this used to do) is fine for the real CLI binary,
    // which calls this exactly once per process, but made the test suite
    // (hundreds of `diagnostics_for`/`Project::new` calls, each
    // previously re-parsing the whole corpus independently) measurably
    // slower - confirmed directly: the lib test suite went from ~40s to
    // ~143s. `LazyLock` computes the expensive part (parsing every file)
    // exactly once per process regardless of caller count; each call
    // here still returns a fresh, independently-owned `Vec` (cloning
    // already-built `FunctionSig`s, not re-parsing anything) so this
    // function's own public signature - and every one of its call sites,
    // which all consume the returned `Vec` by value - stays unchanged.
    static CACHE: std::sync::LazyLock<Vec<(String, FunctionSig)>> =
        std::sync::LazyLock::new(|| {
            let mut seen = std::collections::HashSet::new();
            let mut result = Vec::new();
            for source in PHPSTORM_STUBS_SOURCES
                .iter()
                .chain(PHPSTAN_SRC_SOURCES.iter())
            {
                for (name, sig) in generate_from_source(source) {
                    if seen.insert(name.clone()) {
                        result.push((name, sig));
                    }
                }
            }
            patch_array_multisort_by_ref(&mut result);
            patch_debug_backtrace(&mut result);
            patch_usleep(&mut result);
            result
        });
    CACHE.clone()
}

/// Parses one stub file's source into `(name, ClassInfo)` pairs for every
/// class/interface/enum it declares - undefined-class detection (Phase 9)
/// needs built-in classes to actually be known, the same way
/// undefined-function detection needs `generated_stubs`' functions to be
/// known; without this, every reference to a built-in like `Exception` or
/// `ArrayObject` would look undefined.
pub fn generate_classes_from_source(source: &str) -> Vec<(String, ClassInfo)> {
    let tokens = Lexer::new(source).tokenize_with_spans();
    let program = Parser::new(tokens)
        .parse_program()
        .expect("vendored phpstorm-stubs source failed to parse");
    let registry = Registry::index_and_flatten(&program);
    registry
        .classes
        .into_iter()
        .map(|(name, mut info)| {
            // Same reasoning as `generate_from_source`'s own
            // `FunctionSig::is_stub` marking, one level down: every
            // method a stub class declares has an equally empty,
            // unreal body.
            for sig in info.methods.values_mut() {
                sig.is_stub = true;
            }
            (name, info)
        })
        .collect()
}

/// ROADMAP "Known but not yet scheduled": the constant-side sibling of
/// `generate_classes_from_source` just above - undefined-`use const`/
/// bare-constant detection needs built-in/extension constants
/// (`JSON_PRETTY_PRINT`, `SORT_*`, `CURLOPT_*`, ...) to actually be
/// known, the same way undefined-function detection needs
/// `generated_stubs()`'s functions to be known. Real phpstorm-stubs
/// declares these via ordinary top-level `define('NAME', value)` calls
/// (6781 of them across the vendored corpus) - `Registry::index_and_
/// flatten` already collects those into its own `global_constants` map
/// via the identical `define_call_constant`/`register_global_constant`
/// path project source goes through, so there's nothing new to parse,
/// only a new place to read the result from.
pub fn generate_constants_from_source(source: &str) -> Vec<(String, Type)> {
    let tokens = Lexer::new(source).tokenize_with_spans();
    let program = Parser::new(tokens)
        .parse_program()
        .expect("vendored phpstorm-stubs source failed to parse");
    let registry = Registry::index_and_flatten(&program);
    registry.global_constants.into_iter().collect()
}

/// All constants generated from the vendored phpstorm-stubs sources,
/// merged via `register_global_constant`'s own "poison to Mixed on any
/// real disagreement" policy - unlike `generated_stubs`/`generated_
/// stub_classes`'s first-occurrence-wins dedup (a stub function/class
/// name conflict is resolved by this project's own conditional-
/// declaration tiebreaking instead), a bare global constant has no such
/// tiebreaking mechanism, so this reuses the exact same conflict policy
/// already used *within* one file's own `global_constants` map, across
/// the whole corpus at once - the identical merge `Registry::
/// merge_stub_constants` (`registry/index.rs`) applies again for a
/// project's own constants layered on top.
pub fn generated_stub_constants() -> Vec<(String, Type)> {
    // See `generated_stubs`' own doc comment - identical `LazyLock`
    // caching, same reason.
    static CACHE: std::sync::LazyLock<Vec<(String, Type)>> = std::sync::LazyLock::new(|| {
        let mut merged: rustc_hash::FxHashMap<String, Type> = rustc_hash::FxHashMap::default();
        for source in PHPSTORM_STUBS_SOURCES
            .iter()
            .chain(PHPSTAN_SRC_SOURCES.iter())
        {
            for (name, ty) in generate_constants_from_source(source) {
                register_global_constant(&mut merged, name, ty);
            }
        }
        merged.into_iter().collect()
    });
    CACHE.clone()
}

/// Phase 54/77 follow-up: real phpstorm-stubs' own `ReflectionClass::
/// __construct` (confirmed directly, `vendor/phpstorm-stubs/Reflection/
/// ReflectionClass.php`) declares its single parameter's own `@param` tag
/// as `class-string<T>|T $objectOrClass`, referencing the class's own
/// `@template T of object` - but that doc type never reaches `ParamSig::ty`
/// (confirmed directly: `new ReflectionClass($x)` binds nothing, and a
/// deliberately-mistyped call still only reports the native `object|string`
/// hint in its error message, not the union with `class-string<T>`) - a
/// real, disclosed gap in this project's own doc-merge pipeline for a
/// parameter whose native hint additionally carries phpstorm-stubs' own
/// `#[LanguageLevelTypeAware(...)]` attribute, not chased down further
/// here since item 4a's own general constructor-binding mechanism already
/// verified correct for the identical `class-string<T>|T` shape on a
/// project-local class with no such attribute. Reuses the exact template
/// metadata `ReflectionClass`'s own already-correctly-parsed `ClassInfo`
/// carries for `T` (`templates`/`template_bounds`/`covariant_templates`/
/// `contravariant_templates`/`template_defaults` - all four already
/// correctly populated, confirmed via `ReflectionClass<Foo>` parsing and
/// resolving fine as an explicit type reference) rather than hand-guessing
/// a fresh bound/variance/default - `TemplateParam` is a plain value type,
/// not identity-sensitive, so building one from those exact field values
/// is equivalent to "reusing" any other node that would carry them.
///
/// Union member order matters: `ClassString<T>` must come first, since
/// `bind_template_param`'s own `Union` handling is first-`or_insert`-wins -
/// `new ReflectionClass(Foo::class)` needs the `ClassString` arm to bind
/// `T=Foo` before the bare `T` arm would otherwise wrongly bind `T` to the
/// `ClassString<Foo>` value itself.
///
/// Disclosed residual limitation, not attempted here: `new
/// ReflectionClass('Foo')` - a bare string literal, not `Foo::class` -
/// still won't bind `T` correctly (`bind_template_param` has no arm
/// treating a string literal's own *value* as a class-name reference, only
/// a real `Type::ClassString` construction via `::class`/`get_class()`
/// triggers that arm) - the `Foo::class`/object-instance shapes this fixes
/// are the idiomatic PHP 8+ styles anyway.
fn patch_reflection_class_constructor_template(classes: &mut [(String, ClassInfo)]) {
    let Some((_, info)) = classes
        .iter_mut()
        .find(|(name, _)| name == "ReflectionClass")
    else {
        return;
    };
    if !info.templates.iter().any(|t| t == "T") {
        return;
    }
    let tp = TemplateParam {
        name: "T".to_string(),
        bound: Box::new(
            info.template_bounds
                .get("T")
                .cloned()
                .unwrap_or(Type::Mixed),
        ),
        variance: if info.covariant_templates.contains("T") {
            crate::types::Variance::Covariant
        } else if info.contravariant_templates.contains("T") {
            crate::types::Variance::Contravariant
        } else {
            crate::types::Variance::Invariant
        },
        scope: Some("ReflectionClass".to_string()),
        default: info.template_defaults.get("T").cloned().map(Box::new),
    };
    let Some(ctor) = info.methods.get_mut("__construct") else {
        return;
    };
    let Some(param) = ctor.params.first_mut() else {
        return;
    };
    param.ty = Type::Union(vec![
        Type::ClassString(Box::new(Type::TemplateParam(tp.clone()))),
        Type::TemplateParam(tp.clone()),
    ]);
    // `ReflectionClass::newInstanceArgs()` is the one instance-producing
    // method phpstorm-stubs (`vendor/phpstorm-stubs/Reflection/
    // ReflectionClass.php`) doesn't document a `@return T` tag for at all
    // - `newInstance()`/`newInstanceWithoutConstructor()` both already
    // carry one, correctly resolving to `T` with no patch needed, but
    // `newInstanceArgs()` is left at its bare native `?object` hint,
    // matching the raw signature exactly (nothing to merge in). Real
    // PHPStan gets its own more precise `@return T` here from a
    // completely separate, PHPStan-specific bundled stub override
    // (`vendor/phpstan-src/stubs/ReflectionClass.stub`, a different
    // directory from the `vendor/phpstan-src/src/` runtime source this
    // project already vendors for symbol resolution) that this project
    // doesn't otherwise ingest - a small, targeted patch here, mirroring
    // this same function's own constructor-param patch just above,
    // matches this project's existing "hand-patch this one specific,
    // confirmed ReflectionClass generic gap" precedent far better than
    // standing up a second stub-source ingestion pipeline for one
    // missing tag in one file. Real corpus false positive this fixes:
    // `MailServiceTestTransportSwap::with`'s own `$reflClass->
    // newInstanceArgs($args)` returning `T|null` instead of plain `T`.
    if let Some(new_instance_args) = info.methods.get_mut("newInstanceArgs") {
        new_instance_args.return_type = Type::TemplateParam(tp);
    }
}

/// Phase 77: `ReflectionMethod`/`ReflectionProperty` are not generic in
/// real phpstorm-stubs at all - unlike `ReflectionClass`'s own real `T`
/// (patched just above), this synthesizes a brand-new `@template-covariant
/// TClass of object` on both, purely as an internal enrichment, so that:
/// - `ReflectionClass<T>::getMethod()`/`getMethods()`/`getProperty()`/
///   `getProperties()` can propagate the receiver's own `T` into the
///   returned reflector's new `TClass` slot instead of a bare, untemplated
///   `ReflectionMethod`/`ReflectionProperty`;
/// - `ReflectionMethod<TClass>::getDeclaringClass()`/
///   `ReflectionProperty<TClass>::getDeclaringClass()` can hand it
///   straight back as `ReflectionClass<TClass>` - both real stubs
///   currently return a bare, untemplated `ReflectionClass`.
///
/// No new substitution engine work was needed for this: `checker::
/// generics::substitute_class_templates`'s existing `substitute_scoped`
/// already recurses through a returned `Type::Object`'s own type-argument
/// list (Phase 54's own "mirrors `types::substitute_template`'s identical
/// fix" arm) - the identical mechanism any ordinary fluent generic
/// collection API (a `Collection<T>`-shaped method returning another
/// `Collection<T>`) already relies on, confirmed by direct read before
/// writing this patch rather than assumed.
///
/// Because this template doesn't exist in real PHPStan's own stubs at
/// all, it must also be exempted from `missingType.generics`
/// (`checker/missing_typehint.rs`'s own `SYNTHETIC_GENERIC_CLASS_NAMES`),
/// since otherwise every ordinary bare `ReflectionMethod`/
/// `ReflectionProperty` type hint anywhere (an extremely common shape:
/// any reflection-based code, DI container, ORM metadata reader,
/// attribute reader) would start getting flagged at level 6+ for a
/// template this project invented, not one a user ever wrote or could
/// satisfy.
fn patch_reflection_method_and_property_templates(classes: &mut [(String, ClassInfo)]) {
    for class_name in ["ReflectionMethod", "ReflectionProperty"] {
        let Some((_, info)) = classes.iter_mut().find(|(name, _)| name == class_name) else {
            continue;
        };
        if info.templates.iter().any(|t| t == "TClass") {
            continue;
        }
        info.templates.push("TClass".to_string());
        info.covariant_templates.insert("TClass".to_string());
        info.template_bounds.insert(
            "TClass".to_string(),
            Type::Object("object".to_string(), Vec::new()),
        );
        let tp = TemplateParam {
            name: "TClass".to_string(),
            bound: Box::new(Type::Object("object".to_string(), Vec::new())),
            variance: crate::types::Variance::Covariant,
            scope: Some(class_name.to_string()),
            default: None,
        };
        if let Some(m) = info.methods.get_mut("getDeclaringClass") {
            m.return_type =
                Type::Object("ReflectionClass".to_string(), vec![Type::TemplateParam(tp)]);
        }
    }

    // `ReflectionClass`'s own `T` (real in phpstorm-stubs, already patched
    // above) is what the 4 receiver-propagating methods below bind into
    // the newly-added `TClass` slot - rebuild the same `TemplateParam`
    // `patch_reflection_class_constructor_template` already builds for
    // `T`, rather than reaching into that function's own local variable.
    let Some(t_ty) = classes
        .iter()
        .find(|(name, _)| name == "ReflectionClass")
        .and_then(|(_, info)| {
            if !info.templates.iter().any(|t| t == "T") {
                return None;
            }
            Some(Type::TemplateParam(TemplateParam {
                name: "T".to_string(),
                bound: Box::new(
                    info.template_bounds
                        .get("T")
                        .cloned()
                        .unwrap_or(Type::Mixed),
                ),
                variance: if info.covariant_templates.contains("T") {
                    crate::types::Variance::Covariant
                } else if info.contravariant_templates.contains("T") {
                    crate::types::Variance::Contravariant
                } else {
                    crate::types::Variance::Invariant
                },
                scope: Some("ReflectionClass".to_string()),
                default: info.template_defaults.get("T").cloned().map(Box::new),
            }))
        })
    else {
        return;
    };

    let Some((_, rc_info)) = classes
        .iter_mut()
        .find(|(name, _)| name == "ReflectionClass")
    else {
        return;
    };
    if let Some(m) = rc_info.methods.get_mut("getMethod") {
        m.return_type = Type::Object("ReflectionMethod".to_string(), vec![t_ty.clone()]);
    }
    if let Some(m) = rc_info.methods.get_mut("getMethods") {
        m.return_type = Type::Array(
            Box::new(Type::unknown_array_key()),
            Box::new(Type::Object(
                "ReflectionMethod".to_string(),
                vec![t_ty.clone()],
            )),
        );
    }
    if let Some(m) = rc_info.methods.get_mut("getProperty") {
        m.return_type = Type::Object("ReflectionProperty".to_string(), vec![t_ty.clone()]);
    }
    if let Some(m) = rc_info.methods.get_mut("getProperties") {
        m.return_type = Type::Array(
            Box::new(Type::unknown_array_key()),
            Box::new(Type::Object("ReflectionProperty".to_string(), vec![t_ty])),
        );
    }
}

/// The PHP 8 `debug_backtrace()`/`Throwable::getTrace()`-shaped "trace
/// frame" array: `list<array{function: string, line?: int, file?: string,
/// class?: string, type?: '->'|'::', args?: list<mixed>, object?:
/// object}>`, confirmed directly via `\PHPStan\dumpType()` against the
/// real `phpstan/phpstan` phar (real PHPStan hardcodes this via its own
/// dynamic-return-type extensions, not from either function's own bare
/// `array`-returning stub text).
fn backtrace_frame_list_type() -> Type {
    let frame_shape = Type::ArrayShape(vec![
        ("function".to_string(), Type::String, false),
        ("line".to_string(), Type::Int, true),
        ("file".to_string(), Type::String, true),
        // A bare (non-generic) `class-string` is already plain `Type::
        // String` in this project's own PHPDoc parser (`types/parse.rs`'s
        // own `"class-string" => ... [] => Type::String`) - matched here
        // for consistency, not a simplification specific to this patch.
        ("class".to_string(), Type::String, true),
        (
            "type".to_string(),
            Type::Union(vec![
                Type::StringLiteral("->".to_string()),
                Type::StringLiteral("::".to_string()),
            ]),
            true,
        ),
        (
            "args".to_string(),
            Type::Intersection(vec![
                Type::Array(Box::new(Type::unknown_array_key()), Box::new(Type::Mixed)),
                Type::Accessory(AccessoryKind::ArrayList),
            ]),
            true,
        ),
        (
            "object".to_string(),
            Type::Object("object".to_string(), Vec::new()),
            true,
        ),
    ]);
    Type::Intersection(vec![
        Type::Array(Box::new(Type::unknown_array_key()), Box::new(frame_shape)),
        Type::Accessory(AccessoryKind::ArrayList),
    ])
}

/// Two real, corpus-verified `list<T>` precision gaps found by diffing
/// phpstan-rs's own scoped Piwigo corpus run against a real `vendor/
/// bin/phpstan analyse` run on the same corpus and tracing the
/// discrepancy to source: neither phpstorm-stubs nor this project's own
/// `class_info.rs`/`php_stubs.rs` mark these two methods' return type
/// with the `list` accessory at all (both are a bare native `array`,
/// with no refining PHPDoc tag in phpstorm-stubs either), while real
/// PHPStan's own dynamic-return-type extensions do - confirmed directly
/// via `\PHPStan\dumpType()` against the real phar, not assumed from
/// stub text alone.
/// - `ReflectionFunctionAbstract::getParameters()` -> `list<
///   ReflectionParameter>`. Real corpus false positive this fixes:
///   `Piwigo\Tools\PhpStan\Latte\ShimClassGenerator::generate()`'s own
///   `array_map(..., $reflection->getParameters())` result, expected as
///   `list<string>` by `renderMethod()`'s own signature, inferred as
///   bare `array<string>` without this.
/// - `Exception`/`Error::getTrace()` -> the shared `backtrace_frame_
///   list_type()` shape above. Real corpus false positive this fixes:
///   `SensitiveParameterTest`'s own `sensitiveParameterTestTraceContains(
///   $exception->getTrace(), ...)` calls (3 real sites), expecting
///   exactly this shape.
fn patch_reflection_and_exception_list_types(classes: &mut [(String, ClassInfo)]) {
    if let Some((_, info)) = classes
        .iter_mut()
        .find(|(name, _)| name == "ReflectionFunctionAbstract")
        && let Some(m) = info.methods.get_mut("getParameters")
    {
        m.return_type = Type::Intersection(vec![
            Type::Array(
                Box::new(Type::unknown_array_key()),
                Box::new(Type::Object("ReflectionParameter".to_string(), Vec::new())),
            ),
            Type::Accessory(AccessoryKind::ArrayList),
        ]);
    }
    // Both `Exception`/`Error`'s own real phpstorm-stubs `getTrace()`
    // declarations are `final` (confirmed directly - neither is ever
    // overridden further down either hierarchy), so patching these two
    // concrete classes directly, rather than the `Throwable` interface
    // they both also implement, covers every real throwable via
    // ordinary inheritance.
    for class_name in ["Exception", "Error"] {
        let Some((_, info)) = classes.iter_mut().find(|(name, _)| name == class_name) else {
            continue;
        };
        let Some(m) = info.methods.get_mut("getTrace") else {
            continue;
        };
        m.return_type = backtrace_frame_list_type();
    }
}

/// All classes generated from the vendored phpstorm-stubs sources, merged
/// (first occurrence wins - see `generated_stubs`' equivalent note).
pub fn generated_stub_classes() -> Vec<(String, ClassInfo)> {
    // See `generated_stubs`' own doc comment - identical `LazyLock`
    // caching, same reason.
    static CACHE: std::sync::LazyLock<Vec<(String, ClassInfo)>> = std::sync::LazyLock::new(|| {
        let mut seen = std::collections::HashSet::new();
        let mut result = Vec::new();
        for source in PHPSTORM_STUBS_SOURCES
            .iter()
            .chain(PHPSTAN_SRC_SOURCES.iter())
        {
            for (name, info) in generate_classes_from_source(source) {
                if seen.insert(name.clone()) {
                    result.push((name, info));
                }
            }
        }
        patch_reflection_class_constructor_template(&mut result);
        patch_reflection_method_and_property_templates(&mut result);
        patch_reflection_and_exception_list_types(&mut result);
        result
    });
    CACHE.clone()
}

#[cfg(test)]
mod tests;
