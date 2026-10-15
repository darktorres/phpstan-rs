//! Generates built-in function signatures by parsing real stub sources
//! with this project's own lexer/parser/registry, rather than
//! hand-transcribing approximations one function at a time. This project
//! has no general hand-curated stub-overlay mechanism - see
//! `patch_array_multisort_by_ref` below for the one narrow,
//! explicitly-justified post-generation patch that exists instead.
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
//! `PHPSTAN_SRC_SOURCES`: a second vendored corpus,
//! `vendor/phpstan-src/src/` - real PHPStan's own MIT-licensed runtime
//! source, reused here for the identical reason `phpstorm-stubs` is: a
//! vendor package authoring a PHPStan rule/extension references
//! `PHPStan\Type\*`/`PHPStan\Rules\Rule`/etc., none of which exist as
//! scannable source in `phpstan/phpstan`'s own phar distribution.
//! `generated_stubs`/`generated_stub_classes` chain `PHPSTAN_SRC_SOURCES`
//! in after `PHPSTORM_STUBS_SOURCES`, so real PHP builtins keep their
//! existing first-occurrence-wins priority - phpstan-src can only add
//! names, never take priority over one phpstorm-stubs already claimed.
//! Not closed by this: `vendor/phpstan-src/src` itself references
//! classes from its own dependency, `nikic/php-doc-parser`
//! (`PHPStan\PhpDocParser\Ast\Type\*`), not vendored here.

use super::container::{Reader, Writer};
use super::packed::{Packed, Records, RecordsBuilder};
use syntax::name::Name;
use syntax::name::NameMap;

use rayon::prelude::*;

use crate::registry::{
    ClassReflection, FunctionReflection, Origin, Registry, register_global_constant,
};
use types::{self, AccessoryKind, Shape, TemplateParam, Type};

use stub_sources::{
    PHPSTAN_SRC_PATHS, PHPSTAN_SRC_SOURCES, PHPSTAN_STUB_SOURCES, PHPSTORM_STUBS_EXTENSIONS,
    PHPSTORM_STUBS_PATHS, PHPSTORM_STUBS_SOURCES,
};

/// phpstorm-stubs' own real (and, per real PHP's own C-level `arg_info`,
/// genuinely accurate) declaration of `array_multisort`'s variadic tail
/// is `&...$rest` - every "rest" argument marked by-reference uniformly.
/// Real PHP's own actual runtime behavior for this specific function is
/// looser than that declaration suggests, though: a call like
/// `array_multisort($a, SORT_DESC, SORT_NUMERIC, $b)` legitimately
/// passes literal sort-order/sort-flag constants into by-ref-declared
/// positions, which every other by-ref parameter in the entire
/// language would reject outright ("Only variables can be passed by
/// reference"). Real PHPStan's own bundled `resources/functionMap.php`
/// declares this same function with a plain,
/// non-`&`-prefixed signature (`SignatureMapParser`'s own convention:
/// a `&`-prefixed parameter name is what marks by-ref, and this
/// project's entry has none) - i.e. real PHPStan deliberately treats
/// none of `array_multisort`'s arguments as reference-checked at all,
/// the exact trade-off (losing precision on the genuinely-by-ref array
/// slots to avoid a false positive on the interleaved sort-flag ones)
/// mirrored here. This project has no general hand-curated stub-overlay
/// mechanism - a single, narrow, explicitly justified post-generation
/// patch for this one function is more honest than reintroducing a
/// whole overlay system for it.
fn patch_array_multisort_by_ref(stubs: &mut [(Name, FunctionReflection)]) {
    for (name, sig) in stubs.iter_mut() {
        if name.as_ref() != "array_multisort" {
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
/// list_types`'s own doc comment).
fn patch_debug_backtrace(stubs: &mut [(Name, FunctionReflection)]) {
    for (name, sig) in stubs.iter_mut() {
        if name.as_ref() == "debug_backtrace" {
            sig.return_type = backtrace_frame_list_type();
        }
    }
}

/// `usleep()`'s own phpstorm-stubs PHPDoc refines its native `int`
/// parameter to `int<0,max>` - real PHP's own C-level `arg_info` has no
/// such refinement, so this is phpstorm-stubs' own text going further
/// than the runtime signature itself, the same situation `patch_array_
/// multisort_by_ref` above documents in general. Real PHPStan's own
/// `resources/functionMap.php` declares `usleep` with a plain `int`
/// parameter (no range), and neither an unconstrained `int` variable nor
/// a literal `-5` passed to `usleep()` is flagged at all. `NativeFunction
/// ReflectionProvider::findFunctionReflection` explains why: any
/// function `functionMap.php` already covers (the overwhelming majority
/// of internal functions, `usleep` included) never even reaches
/// phpstorm-stubs' own PHPDoc for its native parameter types - only
/// PHPStan's own separate, much smaller internal `stubs/` bundle (a
/// distinct resource from phpstorm-stubs) can refine a
/// `functionMap.php`-covered signature further, and it has no entry for
/// `usleep`.
///
/// This is likely one instance of a broader pattern (any
/// `functionMap.php`-covered function whose phpstorm-stubs PHPDoc adds a
/// refinement `functionMap.php` itself doesn't have could be equally
/// unenforced by real PHPStan) - not chased further, the same narrow,
/// one-function-at-a-time discipline `patch_array_multisort_by_ref`
/// above already established rather than reintroducing a whole
/// stub-overlay system for a still-hypothetical wider gap.
fn patch_usleep(stubs: &mut [(Name, FunctionReflection)]) {
    for (name, sig) in stubs.iter_mut() {
        if name.as_ref() == "usleep" {
            for param in &mut sig.params {
                param.ty = Type::int();
            }
        }
    }
}

/// `printf()`'s phpstorm-stubs PHPDoc narrows `mixed ...$values` to
/// `string|int|float`; `functionMap.php` covers `printf`, so PHPStan never
/// reads that tag. The values take `functionMap.php`'s type, which the
/// printf rule checks, as for `sprintf` and `fprintf`.
fn patch_printf_values(stubs: &mut [(Name, FunctionReflection)]) {
    for (name, sig) in stubs.iter_mut() {
        if name.as_ref() != "printf" {
            continue;
        }
        if let Some(values) = sig.params.iter_mut().find(|p| p.variadic()) {
            values.ty = values.ty_native.clone();
            values.doc_ty = None;
        }
    }
}

/// The `array_udiff`/`array_uintersect`/`array_diff_ukey` family takes its
/// comparison callback(s) as the last argument(s) of the variadic
/// `...$rest`, which phpstorm-stubs types `array` only; `$rest` holds
/// either. (`functionMap.php` spells out each arity as its own variant.)
// Declared types are joined while the class hierarchy is being built.
#[allow(clippy::disallowed_methods)]
fn patch_array_callback_family(stubs: &mut [(Name, FunctionReflection)]) {
    const FAMILY: &[&str] = &[
        "array_udiff",
        "array_udiff_assoc",
        "array_udiff_uassoc",
        "array_uintersect",
        "array_uintersect_assoc",
        "array_uintersect_uassoc",
        "array_diff_ukey",
        "array_diff_uassoc",
        "array_intersect_ukey",
        "array_intersect_uassoc",
    ];
    for (name, sig) in stubs.iter_mut() {
        if !FAMILY.contains(&name.as_ref()) {
            continue;
        }
        if let Some(rest) = sig.params.iter_mut().find(|p| p.variadic()) {
            let ty = Type::union_of(vec![
                Type::array(Type::unknown_array_key(), Type::mixed()),
                Type::callable(None),
            ]);
            rest.ty = ty.clone();
            rest.ty_native = ty.clone();
            if rest.doc_ty().is_some() {
                rest.doc_ty = Some(ty);
            }
        }
    }
}

/// Phpstorm-stubs' own `#[ArrayShape(["key" => "type", ...])]` attribute
/// (a JetBrains/PhpStorm IDE-inspection convention) is never read by
/// real PHPStan at all - zero occurrences of "ArrayShape" anywhere in
/// the vendored `vendor/phpstan-src/src/` tree, for any function or
/// method, stub-covered or not. Grafting that attribute's own shape onto
/// a bare `array`/`array|X` return type unconditionally (as an earlier
/// version of this project's stub-generation pipeline did) produces an
/// over-precise shape for every one of the ~85 real occurrences across
/// the vendored phpstorm-stubs corpus, regardless of what real PHPStan's
/// own bundled `resources/functionMap.php` (the actual source real
/// PHPStan uses for a stub function's native signature -
/// `NativeFunctionReflectionProvider` returns a `functionMap.php`-based
/// reflection before phpstorm-stubs' own attributes are ever reachable)
/// says for that same function.
///
/// `pathinfo()` itself needs a patch here too, for a reason distinct
/// from removing the attribute mechanism: unlike every other function
/// checked here, its own phpstorm-stubs docblock also carries a genuine,
/// machine-parseable `@return string|array{dirname: string, basename:
/// string, extension: string, filename: string}` type tag, independent
/// of its `#[ArrayShape(...)]` attribute - so the ordinary
/// docblock-parsing pipeline alone (nothing attribute-specific) would
/// still produce the same over-precise native signature even after the
/// attribute mechanism is removed. This doesn't actually close the
/// `pathinfo()` false positive, though -
/// `checker::expr::builtin_calls::pathinfo_array_shape`'s own dedicated
/// call-site special case (mirroring real PHPStan's own
/// `PathinfoFunctionDynamicReturnTypeExtension`, a real dynamic-return-
/// type extension, not a `functionMap.php`-vs-attribute precision
/// question at all) unconditionally overrides whatever this bare-signature
/// patch gives `pathinfo` for every ordinary call - see that function's
/// own doc comment for the actual fix.
///
/// The fix removed the general (wrong) mechanism entirely - see `registry::
/// function_sig::from_decl`'s own history - rather than special-casing
/// individual functions to suppress it, since the mechanism's own
/// premise never held for any function. This patch restores real
/// precision only where `functionMap.php` genuinely gives one, transcribed
/// directly from that file's own type text (not from phpstorm-stubs' own
/// attribute, which several of these disagree with in the details - e.g.
/// `imagecolorsforindex`'s own real shape uses `int<0, 255>`/`int<0, 127>`
/// ranges, not the attribute's plain `int`) - the same one-function-at-a-
/// time, `functionMap.php`-verified discipline `patch_debug_backtrace`/
/// `patch_usleep` already established. Every function `functionMap.php`
/// itself leaves bare (`gd_info`, `pathinfo`, and the many others
/// `#[ArrayShape]` would over-refine) intentionally gets no patch at
/// all here - bare is the correct, real-PHPStan-matching answer for
/// those, not an oversight. Not ported: `date_sun_info`
/// (`functionMap.php`'s own type wraps its shape in a
/// `__benevolent<...>` marker this project's type-string parser doesn't
/// model); `opcache_get_configuration` (`functionMap.php` does give it a
/// real, deeply-nested shape, but one large enough that hand-transcribing
/// it here risks a silent typo for little real payoff - left bare rather
/// than risk shipping a subtly-wrong shape unverified); and every
/// method-only occurrence of `#[ArrayShape]` (`Imagick`/`PDOStatement`/
/// `Phar`/`SQLite3`, ~22 sites) - `functionMap.php` only ever covers
/// top-level functions, never methods, and none of these classes have a
/// real PHPStan-bundled dynamic-return-type extension either, so bare
/// (whatever the native/PHPDoc return type already resolves to) is
/// already the correct real-PHPStan-matching answer for all of them too.
///
/// Where a later `functionMap_php*delta.php` revises one of these, the
/// newest revision is used - the same newest-PHP reading phpstorm-stubs'
/// own `LanguageLevelTypeAware` attributes get (`proc_get_status` lost
/// its `|false` in 8.0, `session_get_cookie_params` gained `partitioned`
/// in 8.5).
fn patch_functionmap_array_shapes(stubs: &mut [(Name, FunctionReflection)]) {
    let date_parse_shape = types::parse_type_string(
        "array{year: int|false, month: int|false, day: int|false, hour: int|false, \
         minute: int|false, second: int|false, fraction: float|false, warning_count: int, \
         warnings: string[], error_count: int, errors: string[], is_localtime: bool, \
         zone_type?: int|bool, zone?: int|bool, is_dst?: bool, tz_abbr?: string, tz_id?: string, \
         relative?: array{year: int, month: int, day: int, hour: int, minute: int, second: int, \
         weekday?: int, weekdays?: int, first_day_of_month?: bool, last_day_of_month?: bool}}",
    );
    let posix_pw_shape = types::parse_type_string(
        "array{name: string, passwd: string, uid: int, gid: int, gecos: string, dir: string, \
         shell: string}|false",
    );
    let getimagesize_shape = types::parse_type_string(
        "array{0: 0|positive-int, 1: 0|positive-int, 2: int, 3: string, mime: string, \
         channels?: int, bits?: int}|false",
    );
    let patches: &[(&str, &Type)] = &[
        (
            "imagecolorsforindex",
            &types::parse_type_string(
                "array{red: int<0, 255>, green: int<0, 255>, blue: int<0, 255>, \
                 alpha: int<0, 127>}",
            ),
        ),
        (
            "getdate",
            &types::parse_type_string(
                "array{seconds: int<0, 59>, minutes: int<0, 59>, hours: int<0, 23>, \
                 mday: int<1, 31>, wday: int<0, 6>, mon: int<1, 12>, year: int, \
                 yday: int<0, 365>, weekday: \"Monday\"|\"Tuesday\"|\"Wednesday\"|\"Thursday\"| \
                 \"Friday\"|\"Saturday\"|\"Sunday\", month: \"January\"|\"February\"|\"March\"| \
                 \"April\"|\"May\"|\"June\"|\"July\"|\"August\"|\"September\"|\"October\"| \
                 \"November\"|\"December\", 0: int}",
            ),
        ),
        ("date_parse", &date_parse_shape),
        ("date_parse_from_format", &date_parse_shape),
        (
            "date_get_last_errors",
            &types::parse_type_string(
                "array{warning_count: 0|positive-int, warnings: list<string>, \
                 error_count: 0|positive-int, errors: list<string>}|false",
            ),
        ),
        (
            "timezone_location_get",
            &types::parse_type_string(
                "array{country_code: string, latitude: float, longitude: float, \
                 comments: string}|false",
            ),
        ),
        (
            "session_get_cookie_params",
            &types::parse_type_string(
                "array{lifetime: 0|positive-int, path: non-falsy-string, domain: string, \
                 secure: bool, httponly: bool, \
                 samesite: 'None'|'Lax'|'Strict'|'none'|'lax'|'strict', partitioned: bool}",
            ),
        ),
        ("posix_getpwnam", &posix_pw_shape),
        ("posix_getpwuid", &posix_pw_shape),
        (
            "localeconv",
            &types::parse_type_string(
                "array{decimal_point: string, thousands_sep: string, int_curr_symbol: string, \
                 currency_symbol: string, mon_decimal_point: string, mon_thousands_sep: string, \
                 positive_sign: string, negative_sign: string, int_frac_digits: int, \
                 frac_digits: int, p_cs_precedes: int, p_sep_by_space: int, n_cs_precedes: int, \
                 n_sep_by_space: int, p_sign_posn: int, n_sign_posn: int, grouping: list<int>, \
                 mon_grouping: array<int, int>}",
            ),
        ),
        ("getimagesizefromstring", &getimagesize_shape),
        ("getimagesize", &getimagesize_shape),
        (
            "stream_context_get_params",
            &types::parse_type_string("array{notification: string, options: array<mixed>}"),
        ),
        (
            "stream_get_meta_data",
            &types::parse_type_string(
                "array{timed_out: bool, blocked: bool, eof: bool, unread_bytes: int, \
                 stream_type: string, wrapper_type: string, wrapper_data: mixed, mode: string, \
                 seekable: bool, uri?: string, mediatype?: string, base64?: bool, \
                 crypto?: array{protocol: string, cipher_name: string, cipher_bits: int, \
                 cipher_version: string, alpn_protocol?: string}}",
            ),
        ),
        (
            "error_get_last",
            &types::parse_type_string(
                "?array{type: int, message: string, file: string, line: int}",
            ),
        ),
        (
            "proc_get_status",
            &types::parse_type_string(
                "array{command: string, pid: int, running: bool, signaled: bool, stopped: bool, \
                 exitcode: int, termsig: int, stopsig: int}",
            ),
        ),
        (
            "time_nanosleep",
            &types::parse_type_string(
                "array{seconds: 0|positive-int, nanoseconds: 0|positive-int}|bool",
            ),
        ),
        ("pathinfo", &types::parse_type_string("array|string")),
        (
            "token_get_all",
            &types::parse_type_string("list<array{0: int, 1: string, 2: int}|string>"),
        ),
    ];
    for (name, sig) in stubs.iter_mut() {
        if let Some((_, shape)) = patches.iter().find(|(n, _)| *n == name.as_ref()) {
            sig.return_type = (*shape).clone();
        }
    }
}

impl Corpus {
    /// All signatures generated from the vendored phpstorm-stubs sources,
    /// merged (first occurrence wins). A few functions are declared more than
    /// once across the corpus (e.g. a version-conditional pair of signatures
    /// for the same function in different files); taking the first is a
    /// simplification, not a precision-critical choice.
    pub fn generated_stubs(&self) -> Vec<(Name, FunctionReflection)> {
        entries(&self.data().functions, |_| true)
    }
}

/// Real phpstorm-stubs' own `ReflectionClass::__construct`
/// (`vendor/phpstorm-stubs/Reflection/ReflectionClass.php`) declares its
/// single parameter's own `@param` tag as `class-string<T>|T
/// $objectOrClass`, referencing the class's own `@template T of object`,
/// but that doc type never reaches `ParameterReflection::ty` (`new
/// ReflectionClass($x)` binds nothing, and a deliberately-mistyped call
/// still only reports the native `object|string` hint in its error
/// message, not the union with `class-string<T>`) - a gap in this
/// project's own doc-merge pipeline for a parameter whose native hint
/// additionally carries phpstorm-stubs' own
/// `#[LanguageLevelTypeAware(...)]` attribute. Reuses the exact template
/// metadata `ReflectionClass`'s own already-correctly-parsed `ClassReflection`
/// carries for `T` (its `TemplateParam`) rather than hand-guessing a fresh
/// bound/variance/default - `TemplateParam` is a plain value type, not
/// identity-sensitive, so a copy is equivalent to "reusing" any other node
/// that would carry the same values.
///
/// `new ReflectionClass(Foo::class)`, `new ReflectionClass('Foo')` and
/// `new ReflectionClass($foo)` all bind `T` to `Foo`: the class-string
/// member binds from a class string (a literal naming a class included),
/// and the bare `T` member only from an object, `T` being bound by
/// `object`.
// Declared types are joined while the class hierarchy is being built.
#[allow(clippy::disallowed_methods)]
fn patch_reflection_class_constructor_template(classes: &mut [(Name, ClassReflection)]) {
    let Some((_, info)) = classes
        .iter_mut()
        .find(|(name, _)| name.as_ref() == "ReflectionClass")
    else {
        return;
    };
    let Some(tp) = info.template("T").cloned() else {
        return;
    };
    let tp = TemplateParam {
        scope: Some("ReflectionClass".to_string()),
        ..tp
    };
    let Some(ctor) = info.methods.get_mut("__construct") else {
        return;
    };
    let Some(param) = ctor.params.first_mut() else {
        return;
    };
    param.ty = Type::union_of(vec![
        Type::class_string(Type::template_param(tp.clone())),
        Type::template_param(tp.clone()),
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
    // this same function's own constructor-param patch just above, is
    // more proportionate than standing up a second stub-source
    // ingestion pipeline for one missing tag in one file.
    if let Some(new_instance_args) = info.methods.get_mut("newInstanceArgs") {
        new_instance_args.return_type = Type::template_param(tp);
    }
}

/// `ReflectionMethod`/`ReflectionProperty` are not generic in
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
/// already recurses through a returned object's own type-argument
/// list - the identical mechanism any ordinary fluent generic
/// collection API (a `Collection<T>`-shaped method returning another
/// `Collection<T>`) already relies on.
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
fn patch_reflection_method_and_property_templates(classes: &mut [(Name, ClassReflection)]) {
    for class_name in ["ReflectionMethod", "ReflectionProperty"] {
        let Some((_, info)) = classes
            .iter_mut()
            .find(|(name, _)| name.as_ref() == class_name)
        else {
            continue;
        };
        if info.template("TClass").is_some() {
            continue;
        }
        let tp = TemplateParam {
            name: "TClass".to_string(),
            bound: Box::new(Type::object_without_class()),
            variance: types::Variance::Covariant,
            scope: Some(class_name.to_string()),
            default: None,
        };
        info.templates.push(tp.clone());
        if let Some(m) = info.methods.get_mut("getDeclaringClass") {
            m.return_type = Type::object("ReflectionClass".into(), vec![Type::template_param(tp)]);
        }
    }

    // `ReflectionClass`'s own `T` (real in phpstorm-stubs, already patched
    // above) is what the 4 receiver-propagating methods below bind into
    // the newly-added `TClass` slot - rebuild the same `TemplateParam`
    // `patch_reflection_class_constructor_template` already builds for
    // `T`, rather than reaching into that function's own local variable.
    let Some(t_ty) = classes
        .iter()
        .find(|(name, _)| name.as_ref() == "ReflectionClass")
        .and_then(|(_, info)| {
            Some(Type::template_param(TemplateParam {
                scope: Some("ReflectionClass".to_string()),
                ..info.template("T")?.clone()
            }))
        })
    else {
        return;
    };

    let Some((_, rc_info)) = classes
        .iter_mut()
        .find(|(name, _)| name.as_ref() == "ReflectionClass")
    else {
        return;
    };
    if let Some(m) = rc_info.methods.get_mut("getMethod") {
        m.return_type = Type::object("ReflectionMethod".into(), vec![t_ty.clone()]);
    }
    if let Some(m) = rc_info.methods.get_mut("getMethods") {
        m.return_type = Type::array(
            Type::unknown_array_key(),
            Type::object("ReflectionMethod".into(), vec![t_ty.clone()]),
        );
    }
    if let Some(m) = rc_info.methods.get_mut("getProperty") {
        m.return_type = Type::object("ReflectionProperty".into(), vec![t_ty.clone()]);
    }
    if let Some(m) = rc_info.methods.get_mut("getProperties") {
        m.return_type = Type::array(
            Type::unknown_array_key(),
            Type::object("ReflectionProperty".into(), vec![t_ty]),
        );
    }
}

/// The PHP 8 `debug_backtrace()`/`Throwable::getTrace()`-shaped "trace
/// frame" array: `list<array{function: string, line?: int, file?: string,
/// class?: string, type?: '->'|'::', args?: list<mixed>, object?:
/// object}>` (real PHPStan hardcodes this via its own
/// dynamic-return-type extensions, not from either function's own bare
/// `array`-returning stub text).
// Declared types are joined while the class hierarchy is being built.
#[allow(clippy::disallowed_methods)]
fn backtrace_frame_list_type() -> Type {
    let frame_shape = Type::array_shape(Shape::from_named(vec![
        ("function".to_string(), Type::string(), false),
        ("line".to_string(), Type::int(), true),
        ("file".to_string(), Type::string(), true),
        // A bare (non-generic) `class-string` is already plain `Type::
        // String` in this project's own PHPDoc parser (`types/parse.rs`'s
        // own `"class-string"` with no argument is `string`) - matched here
        // for consistency, not a simplification specific to this patch.
        ("class".to_string(), Type::string(), true),
        (
            "type".to_string(),
            Type::union_of(vec![
                Type::string_literal("->".into()),
                Type::string_literal("::".into()),
            ]),
            true,
        ),
        (
            "args".to_string(),
            Type::intersection_of(vec![
                Type::array(Type::unknown_array_key(), Type::mixed()),
                Type::accessory(AccessoryKind::ArrayList),
            ]),
            true,
        ),
        ("object".to_string(), Type::object_without_class(), true),
    ]));
    Type::intersection_of(vec![
        Type::array(Type::unknown_array_key(), frame_shape),
        Type::accessory(AccessoryKind::ArrayList),
    ])
}

/// Two `list<T>` precision gaps: neither phpstorm-stubs nor this
/// project's own `class_info.rs`/`registry::stubs` mark these two methods'
/// return type with the `list` accessory at all (both are a bare native
/// `array`, with no refining PHPDoc tag in phpstorm-stubs either), while
/// real PHPStan's own dynamic-return-type extensions do.
/// - `ReflectionFunctionAbstract::getParameters()` -> `list<
///   ReflectionParameter>`.
/// - `Exception`/`Error::getTrace()` -> the shared `backtrace_frame_
///   list_type()` shape above.
fn patch_reflection_and_exception_list_types(classes: &mut [(Name, ClassReflection)]) {
    if let Some((_, info)) = classes
        .iter_mut()
        .find(|(name, _)| name.as_ref() == "ReflectionFunctionAbstract")
        && let Some(m) = info.methods.get_mut("getParameters")
    {
        m.return_type = Type::intersection_of(vec![
            Type::array(
                Type::unknown_array_key(),
                Type::object("ReflectionParameter".into(), Vec::new()),
            ),
            Type::accessory(AccessoryKind::ArrayList),
        ]);
    }
    // Both `Exception`/`Error`'s own real phpstorm-stubs `getTrace()`
    // declarations are `final` (neither is ever overridden further down
    // either hierarchy), so patching these two
    // concrete classes directly, rather than the `Throwable` interface
    // they both also implement, covers every real throwable via
    // ordinary inheritance.
    for class_name in ["Exception", "Error"] {
        let Some((_, info)) = classes
            .iter_mut()
            .find(|(name, _)| name.as_ref() == class_name)
        else {
            continue;
        };
        let Some(m) = info.methods.get_mut("getTrace") else {
            continue;
        };
        m.return_type = backtrace_frame_list_type();
    }
}

/// Six real SPL iterator classes real PHPStan resolves as generic that
/// this project's own vendored `jetbrains/phpstorm-stubs` copy doesn't
/// `@template`-tag at all - confirmed not a vendoring-freshness gap
/// specific to this project's own pinned commit: a separately-installed,
/// newer `jetbrains/phpstorm-stubs` release (a real target project's own
/// `vendor/`) lacks the same tags on the same classes. Yet a real,
/// installed `phpstan/phpstan` still resolves every one of them as
/// generic - confirmed directly, not assumed: a bare, untyped parameter
/// of each type triggers `missingType.generics` naming the exact
/// template list used below (`RecursiveDirectoryIterator` alone left
/// unflagged by that same probe - a real, observed leniency mirrored via
/// `checker::missing_typehint::MISSING_TYPEHINT_LENIENT_GENERIC_CLASS_
/// NAMES`, not this project's own invention). Real PHPStan must get these
/// from a separate, PHPStan-specific source this project doesn't
/// otherwise ingest - synthesized here the same way `patch_reflection_
/// method_and_property_templates` synthesizes `ReflectionMethod`/
/// `ReflectionProperty`'s own templates just above.
///
/// Real corpus impact (Piwigo's own full unscoped, `--no-composer`
/// verification run, which - unlike this project's real, production
/// scoped `check-project` usage - has no analyzed/context split at all
/// and so is the only mode that ever reaches a vendored dev-tooling
/// package's own incomplete declaration of these): every one of the
/// `generics.notGeneric` false positives these six classes caused is
/// gone.
fn patch_spl_iterator_templates(classes: &mut [(Name, ClassReflection)]) {
    let shapes: &[(&str, &[&str])] = &[
        ("FilterIterator", &["TKey", "TValue", "TIterator"]),
        ("RecursiveIterator", &["TKey", "TValue"]),
        ("RegexIterator", &["TKey", "TValue", "TIterator"]),
        ("RecursiveFilterIterator", &["TKey", "TValue", "TIterator"]),
        ("RecursiveIteratorIterator", &["T"]),
        ("RecursiveDirectoryIterator", &["TKey", "TValue"]),
    ];
    for (class_name, template_names) in shapes {
        let Some((_, info)) = classes
            .iter_mut()
            .find(|(name, _)| name.as_ref() == *class_name)
        else {
            continue;
        };
        if !info.templates.is_empty() {
            continue;
        }
        info.templates = template_names
            .iter()
            .map(|t| TemplateParam {
                scope: Some(class_name.to_string()),
                ..TemplateParam::unbounded((*t).to_string())
            })
            .collect();
    }
}

/// Class-level template declarations from PHPStan's own stubs
/// (`vendor/phpstan-src/stubs/*.stub`), which real PHPStan loads over its
/// reflection of every built-in class. A class phpstorm-stubs declares no
/// templates for takes PHPStan's (with their `@extends`/`@implements`
/// arguments); one declaring the same template names takes PHPStan's
/// variances, and any bound or default it lacks. Differently named
/// templates are left alone - phpstorm-stubs' own members refer to them.
///
/// Property types come from the same stubs: a property the stub gives a
/// PHPDoc type (`DOMDocument::$documentElement` as `DOMElement|null`,
/// where phpstorm-stubs' own `@var` claims a bare `DOMElement`) takes
/// that type, as long as the class ends up with the stub's own template
/// names, since the type may refer to them.
fn patch_phpstan_stub_classes(classes: &mut [(Name, ClassReflection)]) {
    fn same_template_names(a: &[TemplateParam], b: &[TemplateParam]) -> bool {
        a.len() == b.len() && a.iter().zip(b).all(|(a, b)| a.name == b.name)
    }

    for source in PHPSTAN_STUB_SOURCES {
        let Ok(parsed) = syntax::parse(source.as_bytes()) else {
            continue;
        };
        let mut program = parsed.stmts;
        crate::stub_attributes::apply_stub_attributes(&mut program);
        let registry = Registry::index_and_flatten(&program, &Origin::Unknown);
        for (name, stub) in registry.classes {
            let Some((class_name, info)) = classes
                .iter_mut()
                .find(|(n, _)| n.eq_ignore_ascii_case(&name))
            else {
                continue;
            };
            if stub.templates.is_empty() {
                // Nothing to merge.
            } else if info.templates.is_empty() {
                info.templates = stub
                    .templates
                    .iter()
                    .map(|template| TemplateParam {
                        scope: Some(class_name.to_string()),
                        ..template.clone()
                    })
                    .collect();
                if info.extends_args.is_empty() {
                    info.extends_args = stub.extends_args.clone();
                }
                for (iface, args) in &stub.implements_args {
                    info.implements_args
                        .entry(iface.clone())
                        .or_insert_with(|| args.clone());
                }
            } else if same_template_names(&info.templates, &stub.templates) {
                for (own, stub) in info.templates.iter_mut().zip(&stub.templates) {
                    own.variance = stub.variance;
                    if own.bound.is_mixed() {
                        own.bound = stub.bound.clone();
                    }
                    if own.default.is_none() {
                        own.default = stub.default.clone();
                    }
                }
            }
            if !same_template_names(&info.templates, &stub.templates) {
                continue;
            }
            for (prop, stub_sig) in &stub.properties {
                if stub_sig.ty.is_mixed() {
                    continue;
                }
                if let Some(sig) = info.properties.get_mut(prop) {
                    std::sync::Arc::make_mut(sig).ty = stub_sig.ty.clone();
                }
            }
        }
    }
}

impl Corpus {
    /// All classes generated from the vendored phpstorm-stubs sources, merged
    /// (first occurrence wins - see `generated_stubs`' equivalent note).
    pub fn generated_stub_classes(&self) -> Vec<(Name, ClassReflection)> {
        entries(&self.data().classes, |_| true)
    }
}

/// The entries `wanted` accepts, decoded.
fn entries<V: Clone>(
    all: &[StubEntry<V>],
    wanted: impl Fn(&StubEntry<V>) -> bool,
) -> Vec<(Name, V)> {
    all.iter()
        .filter(|entry| wanted(entry))
        .map(|entry| (entry.name.clone(), entry.value.get().clone()))
        .collect()
}

/// One stub function or class, and whether it is PHPStan's own.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(bound(
    serialize = "V: serde::Serialize",
    deserialize = "V: serde::de::DeserializeOwned"
))]
struct StubEntry<V> {
    name: Name,
    phpstan: bool,
    value: crate::registry::Lazy<V>,
}

impl Corpus {
    /// The stub files that reference `key` (a `deps::dependency_key`), by their
    /// number in the index.
    pub fn stub_dependent_files(&self, key: &str) -> impl Iterator<Item = u32> {
        self.index
            .dependents
            .get(key)
            .unwrap_or(&[])
            .as_chunks::<4>()
            .0
            .iter()
            .map(|file| u32::from_le_bytes(*file))
    }
}

impl Corpus {
    /// How many stub files there are.
    #[cfg(test)]
    pub fn stub_file_count(&self) -> usize {
        self.index.declared_by_file.len()
    }
}

impl Corpus {
    /// The stub files `file` leads to, itself included, as a bit set by file
    /// number: the files referencing a name it declares, those referencing a
    /// name one of them declares, and so on. It is a fact of the stubs, made
    /// once for a file.
    pub fn stub_file_closure(&self, file: u32) -> &[u64] {
        self.index.file_closure(file as usize)
    }
}

impl Corpus {
    /// The classes (`true`) and functions stub file `file` declares, each by
    /// its own dependency key.
    pub fn stub_file_names(&self, file: u32) -> impl Iterator<Item = (bool, &str)> {
        self.index
            .declared_keys_of(file as usize)
            .iter()
            .map(|(is_class, key)| (*is_class, &**key))
    }
}

impl Corpus {
    /// The classes (`true`) and functions declared in the stub files that
    /// reference `key` (a `deps::dependency_key`), each by its own dependency
    /// key.
    pub fn stub_dependents(&self, key: &str) -> impl Iterator<Item = (bool, &str)> {
        self.stub_dependent_files(key)
            .flat_map(|file| self.stub_file_names(file))
    }
}

/// The names one stub file declares, each as a dependency key, with whether
/// it is a class.
type DeclaredKeys = Box<[(bool, Box<str>)]>;

impl StubIndex {
    fn file_closure(&self, file: usize) -> &[u64] {
        self.file_closures[file].get_or_init(|| {
            self.closures
                .get(file)
                .as_chunks::<8>()
                .0
                .iter()
                .map(|word| u64::from_le_bytes(*word))
                .collect()
        })
    }

    /// Every stub file's closure over the stub files (`file_closure`), as
    /// the records of little-endian words the index stores: a file's
    /// closure is the files that reference a name it declares, and those
    /// that reference a name one of them declares, and so on. `keys` are
    /// the names each file declares, as `parse_declared_keys` reads them.
    fn build_closures(&self, keys: &[DeclaredKeys]) -> Records {
        let files = keys.len();
        let words = files.div_ceil(64);
        // The files each file leads to directly: those that reference a
        // name it declares. Each key is looked up once, here.
        let edges: Vec<Vec<u32>> = keys
            .iter()
            .map(|declared| {
                let mut out: Vec<u32> = Vec::new();
                for (_, key) in declared.iter() {
                    let dependents = self.dependents.get(key).unwrap_or(&[]);
                    out.extend(
                        dependents
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .map(|dependent| u32::from_le_bytes(*dependent)),
                    );
                }
                out.sort_unstable();
                out.dedup();
                out
            })
            .collect();
        // Tarjan's components, found without recursion. A component comes
        // out after every component it leads to, so its closure is its own
        // files and those of the closures of the components it leads to,
        // each of which is done: no search is repeated.
        const UNVISITED: u32 = u32::MAX;
        let mut order = vec![UNVISITED; files];
        let mut low = vec![0u32; files];
        let mut on_stack = vec![false; files];
        let mut component = vec![UNVISITED; files];
        let mut closures: Vec<Vec<u64>> = Vec::new();
        let mut stack: Vec<usize> = Vec::new();
        let mut calls: Vec<(usize, usize)> = Vec::new();
        let mut counter = 0u32;
        for root in 0..files {
            if order[root] != UNVISITED {
                continue;
            }
            order[root] = counter;
            low[root] = counter;
            counter += 1;
            stack.push(root);
            on_stack[root] = true;
            calls.push((root, 0));
            while let Some(&(node, next)) = calls.last() {
                if let Some(&edge) = edges[node].get(next) {
                    let to = edge as usize;
                    calls.last_mut().expect("a call is open").1 += 1;
                    if order[to] == UNVISITED {
                        order[to] = counter;
                        low[to] = counter;
                        counter += 1;
                        stack.push(to);
                        on_stack[to] = true;
                        calls.push((to, 0));
                    } else if on_stack[to] {
                        low[node] = low[node].min(order[to]);
                    }
                    continue;
                }
                calls.pop();
                if let Some(&(parent, _)) = calls.last() {
                    low[parent] = low[parent].min(low[node]);
                }
                if low[node] != order[node] {
                    continue;
                }
                let id = closures.len() as u32;
                let mut members = Vec::new();
                loop {
                    let member = stack.pop().expect("the component's files are on the stack");
                    on_stack[member] = false;
                    component[member] = id;
                    members.push(member);
                    if member == node {
                        break;
                    }
                }
                let mut bits = vec![0u64; words];
                for &member in &members {
                    bits[member / 64] |= 1 << (member % 64);
                }
                let mut merged = rustc_hash::FxHashSet::default();
                for &member in &members {
                    for &edge in &edges[member] {
                        let other = component[edge as usize];
                        if other != id && merged.insert(other) {
                            for (word, theirs) in bits.iter_mut().zip(&closures[other as usize]) {
                                *word |= theirs;
                            }
                        }
                    }
                }
                closures.push(bits);
            }
        }
        let mut records = RecordsBuilder::default();
        for file in 0..files {
            let bytes: Vec<u8> = closures[component[file] as usize]
                .iter()
                .flat_map(|word| word.to_le_bytes())
                .collect();
            records.push(&bytes);
        }
        records.finish()
    }

    /// The names the record of `declared_by_file` for `file` holds, as
    /// dependency keys.
    fn parse_declared_keys(&self, file: usize) -> DeclaredKeys {
        let mut names = self.declared_by_file.get(file);
        let mut keys = Vec::new();
        while let Some((&is_class, rest)) = names.split_first() {
            let length = u16::from_le_bytes([rest[0], rest[1]]) as usize;
            let key = std::str::from_utf8(&rest[2..2 + length]).expect("a name stored as text");
            keys.push((is_class == 1, Box::from(key)));
            names = &rest[2 + length..];
        }
        keys.into_boxed_slice()
    }

    /// The names stub file `file` declares, as dependency keys, read from
    /// the index the first time and kept.
    fn declared_keys_of(&self, file: usize) -> &DeclaredKeys {
        self.declared_keys[file].get_or_init(|| self.parse_declared_keys(file))
    }
}

impl Corpus {
    /// `generated_stubs` without PHPStan's own source: the PHP built-ins.
    pub fn builtin_stubs(&self) -> Vec<(Name, FunctionReflection)> {
        entries(&self.data().functions, |entry| !entry.phpstan)
    }
}

impl Corpus {
    /// `generated_stub_classes` without PHPStan's own source.
    pub fn builtin_stub_classes(&self) -> Vec<(Name, ClassReflection)> {
        entries(&self.data().classes, |entry| !entry.phpstan)
    }
}

impl Corpus {
    /// `generated_stub_constants` without PHPStan's own source.
    pub fn builtin_stub_constants(&self) -> Vec<(Name, Type)> {
        self.data().builtin_constants.clone()
    }
}

impl Corpus {
    /// The names `generated_stubs`, `generated_stub_classes` and
    /// `generated_stub_constants` hold only because PHPStan's own source
    /// declares them.
    pub fn phpstan_stub_names(&self) -> PhpstanStubNames {
        let index = &self.index;
        let names = |records: &Records| -> Vec<String> {
            (0..records.len())
                .map(|i| String::from_utf8_lossy(records.get(i)).into_owned())
                .collect()
        };
        PhpstanStubNames {
            functions: names(&index.phpstan_functions),
            classes: names(&index.phpstan_classes),
            constants: names(&index.phpstan_constants),
        }
    }
}

/// `phpstan_stub_names`'s result.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct PhpstanStubNames {
    pub functions: Vec<String>,
    pub classes: Vec<String>,
    pub constants: Vec<String>,
}

fn is_phpstan_origin(origin: &Origin) -> bool {
    matches!(origin, Origin::Phpstan { .. })
}

impl Corpus {
    /// The entries of `generated_stubs` named `name` in any case, in its
    /// order; PHPStan's own only `with_phpstan`.
    pub fn generated_stubs_named(
        &self,
        name: &str,
        with_phpstan: bool,
    ) -> Vec<(Name, FunctionReflection)> {
        if !self
            .index
            .function_names
            .contains(&name.trim_start_matches('\\').to_ascii_lowercase())
        {
            return Vec::new();
        }
        let data = self.data();
        named_positions(&data.function_positions, name)
            .iter()
            .map(|&i| &data.functions[i])
            .filter(|entry| with_phpstan || !entry.phpstan)
            .map(|entry| (entry.name.clone(), entry.value.get().clone()))
            .collect()
    }
}

impl Corpus {
    /// The entries of `generated_stub_classes` named `name` in any case, in
    /// its order; PHPStan's own only `with_phpstan`.
    pub fn generated_stub_classes_named(
        &self,
        name: &str,
        with_phpstan: bool,
    ) -> Vec<(Name, ClassReflection)> {
        if !self
            .index
            .class_names
            .contains(&name.trim_start_matches('\\').to_ascii_lowercase())
        {
            return Vec::new();
        }
        let data = self.data();
        named_positions(&data.class_positions, name)
            .iter()
            .map(|&i| &data.classes[i])
            .filter(|entry| with_phpstan || !entry.phpstan)
            .map(|entry| (entry.name.clone(), entry.value.get().clone()))
            .collect()
    }
}

impl Corpus {
    /// The entry of `generated_stub_constants` named exactly `name`, if any;
    /// as the built-ins alone define it unless `with_phpstan`.
    pub fn generated_stub_constants_named(
        &self,
        name: &str,
        with_phpstan: bool,
    ) -> Vec<(Name, Type)> {
        if !self.index.constant_names.contains(name) {
            return Vec::new();
        }
        let data = self.data();
        let ty = if with_phpstan {
            data.constant_positions
                .get(name)
                .map(|&i| &data.constants[i].1)
        } else {
            data.builtin_constant(name)
        };
        ty.map(|ty| (Name::from(name), ty.clone()))
            .into_iter()
            .collect()
    }
}

fn named_positions<'a>(
    positions: &'a rustc_hash::FxHashMap<String, Vec<usize>>,
    name: &str,
) -> &'a [usize] {
    positions
        .get(&name.trim_start_matches('\\').to_ascii_lowercase())
        .map_or(&[], Vec::as_slice)
}

/// Each lowercased name's positions in `entries`.
fn positions_by_name<V>(entries: &[StubEntry<V>]) -> rustc_hash::FxHashMap<String, Vec<usize>> {
    let mut positions: rustc_hash::FxHashMap<String, Vec<usize>> = rustc_hash::FxHashMap::default();
    for (i, entry) in entries.iter().enumerate() {
        positions
            .entry(entry.name.to_ascii_lowercase())
            .or_default()
            .push(i);
    }
    positions
}

/// `entries` as stub entries, each marked PHPStan's own by its origin.
fn stub_entries<V>(entries: Vec<(Name, V)>, origin: impl Fn(&V) -> &Origin) -> Vec<StubEntry<V>> {
    entries
        .into_iter()
        .map(|(name, value)| StubEntry {
            phpstan: is_phpstan_origin(origin(&value)),
            name,
            value: crate::registry::Lazy::decoded(value),
        })
        .collect()
}

impl Corpus {
    /// The constant-side sibling of `generated_stub_classes` just above -
    /// undefined-`use const`/bare-constant detection needs built-in/extension
    /// constants (`JSON_PRETTY_PRINT`, `SORT_*`, `CURLOPT_*`, ...) to actually
    /// be known, the same way undefined-function detection needs
    /// `generated_stubs()`'s functions to be known. Real phpstorm-stubs
    /// declares these via ordinary top-level `define('NAME', value)` calls
    /// (6781 of them across the vendored corpus) - `Registry::index_and_
    /// flatten` already collects those into its own `global_constants` map
    /// via the identical `define_call_constant`/`register_global_constant`
    /// path project source goes through, so there's nothing new to parse,
    /// only a new place to read the result from. Merged via `register_
    /// global_constant`'s own "poison to Mixed on any real disagreement"
    /// policy - unlike `generated_stubs`/`generated_stub_classes`'s
    /// first-occurrence-wins dedup (a stub function/class name conflict is
    /// resolved by this project's own conditional-declaration tiebreaking
    /// instead), a bare global constant has no such tiebreaking mechanism, so
    /// this reuses the exact same conflict policy already used *within* one
    /// file's own `global_constants` map, across the whole corpus at once.
    pub fn generated_stub_constants(&self) -> Vec<(Name, Type)> {
        self.data().constants.clone()
    }
}

/// The three projections `generated_stubs`/`generated_stub_classes`/
/// `generated_stub_constants` each expose, computed together from one
/// shared pass over the vendored corpus instead of three independent
/// ones - `Registry::index_and_flatten` already builds `.functions`,
/// `.classes`, and `.global_constants` together on the same `Registry`
/// per file, so lexing/parsing/indexing each vendored file
/// three separate times (once per projection) just to read a different
/// field off what is otherwise the identical deterministic per-file
/// result was pure waste.
#[derive(serde::Serialize, serde::Deserialize)]
struct GeneratedStubData {
    functions: Vec<StubEntry<FunctionReflection>>,
    classes: Vec<StubEntry<ClassReflection>>,
    constants: Vec<(Name, Type)>,
    // Lowercased name -> its positions in `functions`/`classes`.
    function_positions: rustc_hash::FxHashMap<String, Vec<usize>>,
    class_positions: rustc_hash::FxHashMap<String, Vec<usize>>,
    // Name (constants are case-sensitive) -> its position in `constants`.
    constant_positions: rustc_hash::FxHashMap<String, usize>,
    // The constants as the built-ins alone define them, without PHPStan's
    // own source.
    builtin_constants: Vec<(Name, Type)>,
    builtin_constant_positions: rustc_hash::FxHashMap<String, usize>,
}

/// What is asked of the stubs without reading their declarations, kept
/// apart from them so that a process asking only this reads a small file
/// rather than all of `GeneratedStubData`.
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
struct StubIndex {
    // `deps::dependency_key` -> the stub files referencing it, as
    // positions in `declared_by_file` (four bytes each).
    #[serde(skip)]
    dependents: Packed,
    // The classes and functions each stub file declares: a flag (1 for a
    // class), the name's length and the name, repeated.
    #[serde(skip)]
    declared_by_file: Records,
    // Each record of `declared_by_file` read once as text, when first asked
    // for.
    #[serde(skip)]
    declared_keys: Vec<std::sync::OnceLock<DeclaredKeys>>,
    // Each stub file's closure over the stub files (`stub_file_closure`) as
    // little-endian words, made when the index is built.
    #[serde(skip)]
    closures: Records,
    // Each record of `closures` read once as words, when first asked for.
    #[serde(skip)]
    file_closures: Vec<std::sync::OnceLock<Box<[u64]>>>,
    // Lowercased name -> the phpstorm-stubs directory (extension) that
    // declares it, as a position in `extension_names`; PHPStan's own
    // classes aren't built-ins and aren't here.
    #[serde(skip)]
    function_extensions: Packed,
    #[serde(skip)]
    class_extensions: Packed,
    extension_names: Vec<String>,
    #[serde(skip)]
    extensions: Vec<&'static str>,
    // The names PHPStan's own source declares and the built-ins don't, as
    // records of text: a function, a class and a constant list.
    #[serde(skip)]
    phpstan_functions: Records,
    #[serde(skip)]
    phpstan_classes: Records,
    #[serde(skip)]
    phpstan_constants: Records,
    // The names the stubs declare - classes and functions lowercased,
    // constants as written - so a name no stub declares is known without
    // reading the declarations.
    #[serde(skip)]
    class_names: Packed,
    #[serde(skip)]
    function_names: Packed,
    #[serde(skip)]
    constant_names: Packed,
}

/// The sections a stored index is in, besides `rest`.
const INDEX_PACKED: [&str; 6] = [
    "index:dependents",
    "index:function_extensions",
    "index:class_extensions",
    "index:class_names",
    "index:function_names",
    "index:constant_names",
];
const INDEX_RECORDS: [&str; 5] = [
    "index:declared_by_file",
    "index:file_closures",
    "index:phpstan_functions",
    "index:phpstan_classes",
    "index:phpstan_constants",
];

impl StubIndex {
    fn store(&self, writer: &mut Writer) -> Result<(), String> {
        let packed = [
            &self.dependents,
            &self.function_extensions,
            &self.class_extensions,
            &self.class_names,
            &self.function_names,
            &self.constant_names,
        ];
        for (map, section) in packed.into_iter().zip(INDEX_PACKED) {
            map.store(writer, section)?;
        }
        let records = [
            &self.declared_by_file,
            &self.closures,
            &self.phpstan_functions,
            &self.phpstan_classes,
            &self.phpstan_constants,
        ];
        for (records, section) in records.into_iter().zip(INDEX_RECORDS) {
            records.store(writer, section)?;
        }
        writer.section("index:rest", &crate::registry::stub_cache::encode(self)?)
    }

    fn load(reader: &Reader) -> Result<StubIndex, String> {
        let rest = reader.section("index:rest")?;
        let mut index: StubIndex = crate::registry::stub_cache::decode(&reader.buffer()[rest])?;
        let [a, b, c, d, e, f] = INDEX_PACKED.map(|section| Packed::load(reader, section));
        index.dependents = a?;
        index.function_extensions = b?;
        index.class_extensions = c?;
        index.class_names = d?;
        index.function_names = e?;
        index.constant_names = f?;
        let [a, b, c, d, e] = INDEX_RECORDS.map(|section| Records::load(reader, section));
        index.declared_by_file = a?;
        index.closures = b?;
        index.phpstan_functions = c?;
        index.phpstan_classes = d?;
        index.phpstan_constants = e?;
        if index.closures.len() != index.declared_by_file.len() {
            return Err("the stored closures are not one per stub file".to_string());
        }
        Ok(index.resolved())
    }

    /// The index stored for this build, if there is a readable file of it.
    fn load_cached(dir: Option<&crate::registry::CacheDir>) -> Option<StubIndex> {
        let reader = crate::registry::load_container(dir, "stub-index")?;
        match StubIndex::load(&reader) {
            Ok(index) => Some(index),
            Err(error) => {
                debug_assert!(false, "the stored stub-index is inconsistent: {error}");
                eprintln!(
                    "phpstan-rs: the stored stub-index is inconsistent ({error}); building it again"
                );
                None
            }
        }
    }

    fn store_cached(&self, dir: Option<&crate::registry::CacheDir>) {
        if !dir.is_some_and(crate::registry::CacheDir::can_write) {
            return;
        }
        let mut writer = Writer::new();
        if self.store(&mut writer).is_ok() {
            crate::registry::store_container(dir, "stub-index", writer);
        }
    }

    /// The index with its extension names as the embedded strings they are.
    fn resolved(mut self) -> StubIndex {
        self.declared_keys = (0..self.declared_by_file.len())
            .map(|_| std::sync::OnceLock::new())
            .collect();
        self.file_closures = (0..self.declared_by_file.len())
            .map(|_| std::sync::OnceLock::new())
            .collect();
        self.extensions = self
            .extension_names
            .iter()
            .map(|name| {
                crate::registry::provenance::embedded_str(name).expect("an embedded extension")
            })
            .collect();
        self
    }

    fn extension(&self, map: &Packed, lowercased: &str) -> Option<&'static str> {
        let position = *map.get(lowercased)?.first()?;
        self.extensions.get(position as usize).copied()
    }
}

impl GeneratedStubData {
    fn builtin_constant(&self, name: &str) -> Option<&Type> {
        self.builtin_constant_positions
            .get(name)
            .map(|&i| &self.builtin_constants[i].1)
    }
}

impl Corpus {
    /// The phpstorm-stubs directory (the extension) declaring built-in
    /// function `name`, when it's a PHP built-in.
    pub fn builtin_function_extension(&self, name: &str) -> Option<&'static str> {
        let index = &self.index;
        index.extension(
            &index.function_extensions,
            &name.trim_start_matches('\\').to_ascii_lowercase(),
        )
    }
}

impl Corpus {
    /// The class-like counterpart of `builtin_function_extension`.
    pub fn builtin_class_extension(&self, name: &str) -> Option<&'static str> {
        let index = &self.index;
        index.extension(
            &index.class_extensions,
            &name.trim_start_matches('\\').to_ascii_lowercase(),
        )
    }
}

/// Computes `GeneratedStubData` exactly once per process
/// (`std::sync::LazyLock`) and returns a `'static` reference to it -
/// re-parsing the combined vendored corpus from scratch on every call
/// is fine for the CLI binary, which calls each of the three public
/// accessors above exactly once per process, but makes the test suite
/// (hundreds of `diagnostics_for`/`Project::new` calls, each
/// independently needing the same data) measurably slower. Each public
/// accessor still returns a fresh, independently-owned `Vec` (cloning
/// already-built values, not re-parsing anything), so none of their
/// public signatures - or any of their call sites, which all consume the
/// returned `Vec` by value - change.
/// `DateTime`/`DateTimeImmutable`'s fluent methods return an instance of
/// the receiver's own class (the object itself, or a clone of it), so a
/// subclass calling `parent::modify()` gets itself back. phpstorm-stubs
/// types some of these by the base class name instead (the PHP 8.4
/// tentative hint); the class member becomes `static`, keeping any
/// `false` failure alternative.
fn patch_datetime_fluent_returns(classes: &mut [(Name, ClassReflection)]) {
    const FLUENT: [&str; 9] = [
        "modify",
        "add",
        "sub",
        "setDate",
        "setISODate",
        "setTime",
        "setTimestamp",
        "setTimezone",
        "setMicrosecond",
    ];
    for (name, info) in classes.iter_mut() {
        if !matches!(name.as_ref(), "DateTime" | "DateTimeImmutable") {
            continue;
        }
        let class = name.to_string();
        for method in FLUENT {
            let Some(sig) = info.methods.get_mut(method) else {
                continue;
            };
            let to_static = |ty: &Type| {
                if ty.as_object().is_some_and(|(n, _)| *n == class) {
                    Type::static_ref(Vec::new())
                } else {
                    ty.clone()
                }
            };
            let members: Vec<Type> = sig
                .return_type
                .union_members()
                .iter()
                .map(to_static)
                .collect();
            sig.return_type = match <[Type; 1]>::try_from(members) {
                Ok([single]) => single,
                Err(members) => sig.return_type.with_members(members),
            };
        }
    }
}

/// One stub file's declarations, in the order its registry yields them.
struct StubFileDeclarations {
    functions: Vec<(Name, FunctionReflection)>,
    classes: Vec<(Name, ClassReflection)>,
    constants: Vec<(Name, Type)>,
    // The names the file references (`deps::referenced_names`).
    references: rustc_hash::FxHashSet<String>,
}

/// Parses and indexes one embedded stub file, from `origin`.
fn index_stub_file(source: &str, origin: &Origin) -> StubFileDeclarations {
    let mut program = syntax::parse(source.as_bytes())
        .expect("vendored phpstorm-stubs source failed to parse")
        .stmts;
    crate::stub_attributes::apply_stub_attributes(&mut program);
    let registry = Registry::index_and_flatten(&program, origin);
    StubFileDeclarations {
        references: crate::deps::referenced_names(&program)
            .iter()
            .filter_map(crate::deps::RefKey::stub_key)
            .map(str::to_string)
            .collect(),
        functions: registry
            .functions
            .into_iter()
            .map(|(name, mut sig)| {
                // See `FunctionReflection::is_stub`'s own doc comment: a stub
                // file's own body is never real - `Registry::
                // index_and_flatten` has no way to know that on its own,
                // so this is set here instead, the one place that does.
                sig.is_stub = true;
                (name, sig)
            })
            .collect(),
        classes: registry
            .classes
            .into_iter()
            .map(|(name, mut info)| {
                // Same reasoning one level down: every method a stub
                // class declares has an equally empty, unreal body.
                for sig in info.methods.values_mut() {
                    sig.is_stub = true;
                }
                (name, info)
            })
            .collect(),
        constants: registry.global_constants.into_iter().collect(),
    }
}

/// The built-in stubs as a run reads them: the index of what the stub
/// files declare and reference, which is small, and the declarations
/// themselves, read from the stub cache when first asked for. Made once
/// per `Stubs` (`Stubs::corpus`), on a thread of its own, and shared by
/// every registry built on it.
pub struct Corpus {
    index: StubIndex,
    data: std::sync::OnceLock<GeneratedStubData>,
    // The stub cache the declarations are read from, if the index was.
    dir: Option<std::sync::Arc<crate::registry::CacheDir>>,
}

impl std::fmt::Debug for Corpus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Corpus").finish_non_exhaustive()
    }
}

impl Corpus {
    /// The corpus stored in the stub cache `dir` for this build, else
    /// built from the embedded sources on `pool` and stored there.
    pub(crate) fn obtain(
        dir: Option<&std::sync::Arc<crate::registry::CacheDir>>,
        pool: &rayon::ThreadPool,
    ) -> Corpus {
        if let Some(index) = StubIndex::load_cached(dir.map(|d| &**d)) {
            return Corpus {
                index,
                data: std::sync::OnceLock::new(),
                dir: dir.map(std::sync::Arc::clone),
            };
        }
        let (index, data) = build_generated_stub_data(Some(pool));
        index.store_cached(dir.map(|d| &**d));
        crate::registry::store_cached(dir.map(|d| &**d), "stub-data", &data);
        Corpus {
            index,
            data: std::sync::OnceLock::from(data),
            dir: dir.map(std::sync::Arc::clone),
        }
    }

    /// The corpus built from the embedded sources, stored nowhere.
    pub(crate) fn built(pool: Option<&rayon::ThreadPool>) -> Corpus {
        let (index, data) = build_generated_stub_data(pool);
        Corpus {
            index,
            data: std::sync::OnceLock::from(data),
            dir: None,
        }
    }

    /// The declarations. When the stub cache that held the index no longer
    /// holds them, they are built again here, serially: this can be asked
    /// on a thread of any pool, which must not wait for a parallel build.
    fn data(&self) -> &GeneratedStubData {
        self.data.get_or_init(|| {
            match crate::registry::load_cached(self.dir.as_deref(), "stub-data") {
                Some(data) => data,
                None => build_generated_stub_data(None).1,
            }
        })
    }
}

/// Indexes every embedded stub file and merges the results.
fn build_generated_stub_data(pool: Option<&rayon::ThreadPool>) -> (StubIndex, GeneratedStubData) {
    let mut seen_functions = rustc_hash::FxHashSet::default();
    let mut functions = Vec::new();
    let mut seen_classes = rustc_hash::FxHashSet::default();
    let mut classes = Vec::new();
    let mut merged_constants: syntax::name::NameMap<Type> = syntax::name::NameMap::default();
    let mut builtin_constants: syntax::name::NameMap<Type> = NameMap::default();
    let mut function_extensions = rustc_hash::FxHashMap::default();
    let mut class_extensions = rustc_hash::FxHashMap::default();
    let builtins = PHPSTORM_STUBS_SOURCES
        .iter()
        .zip(PHPSTORM_STUBS_PATHS)
        .zip(PHPSTORM_STUBS_EXTENSIONS)
        .map(|((source, file), extension)| {
            (
                *source,
                Some(*extension),
                Origin::Builtin { file, extension },
            )
        });
    let phpstan = PHPSTAN_SRC_SOURCES
        .iter()
        .zip(PHPSTAN_SRC_PATHS)
        .map(|(source, file)| (*source, None, Origin::Phpstan { file }));
    let files: Vec<(&'static str, Option<&'static str>, Origin)> =
        builtins.chain(phpstan).collect();
    // Each file indexes on its own; merging stays sequential, in file
    // order, so which declaration wins doesn't depend on scheduling.
    let index_file = |(source, _, origin): &(&'static str, Option<&'static str>, Origin)| {
        index_stub_file(source, origin)
    };
    let indexed: Vec<StubFileDeclarations> = match pool {
        Some(pool) => pool.install(|| files.par_iter().map(index_file).collect()),
        None => files.iter().map(index_file).collect(),
    };
    let mut dependents: rustc_hash::FxHashMap<String, Vec<u32>> = rustc_hash::FxHashMap::default();
    let mut declared_by_file: Vec<Vec<(bool, String)>> = Vec::new();
    for ((_, extension, _), file) in files.into_iter().zip(indexed) {
        let declared: Vec<(bool, String)> = file
            .classes
            .iter()
            .map(|(name, _)| (true, name.to_string()))
            .chain(
                file.functions
                    .iter()
                    .map(|(name, _)| (false, name.to_string())),
            )
            .collect();
        if !declared.is_empty() {
            let id = declared_by_file.len() as u32;
            for key in &file.references {
                dependents.entry(key.clone()).or_default().push(id);
            }
            declared_by_file.push(declared);
        }
        for (name, sig) in file.functions {
            if seen_functions.insert(name.clone()) {
                if let Some(extension) = extension {
                    function_extensions.insert(name.to_ascii_lowercase(), extension);
                }
                functions.push((name, sig));
            }
        }
        for (name, info) in file.classes {
            if seen_classes.insert(name.clone()) {
                if let Some(extension) = extension {
                    class_extensions.insert(name.to_ascii_lowercase(), extension);
                }
                classes.push((name, info));
            }
        }
        for (name, ty) in file.constants {
            if extension.is_some() {
                register_global_constant(&mut builtin_constants, name.clone(), ty.clone());
            }
            register_global_constant(&mut merged_constants, name, ty);
        }
    }
    patch_array_multisort_by_ref(&mut functions);
    patch_debug_backtrace(&mut functions);
    patch_usleep(&mut functions);
    patch_printf_values(&mut functions);
    patch_array_callback_family(&mut functions);
    patch_functionmap_array_shapes(&mut functions);
    patch_reflection_class_constructor_template(&mut classes);
    patch_reflection_method_and_property_templates(&mut classes);
    patch_reflection_and_exception_list_types(&mut classes);
    patch_phpstan_stub_classes(&mut classes);
    patch_spl_iterator_templates(&mut classes);
    patch_datetime_fluent_returns(&mut classes);
    let constants: Vec<(Name, Type)> = merged_constants.into_iter().collect();
    let builtin_constants: Vec<(Name, Type)> = builtin_constants.into_iter().collect();
    let functions = stub_entries(functions, |sig| sig.provenance().origin());
    let classes = stub_entries(classes, |info| info.provenance.origin());
    let data = GeneratedStubData {
        builtin_constant_positions: builtin_constants
            .iter()
            .enumerate()
            .map(|(i, (name, _))| (name.to_string(), i))
            .collect(),
        builtin_constants,
        function_positions: positions_by_name(&functions),
        class_positions: positions_by_name(&classes),
        constant_positions: constants
            .iter()
            .enumerate()
            .map(|(i, (name, _))| (name.to_string(), i))
            .collect(),
        functions,
        classes,
        constants,
    };
    let to_packed =
        |keys: Vec<String>| Packed::build(keys.into_iter().map(|key| (key, Vec::new())).collect());
    let mut extension_names: Vec<String> = Vec::new();
    let mut extensions_packed = |map: rustc_hash::FxHashMap<String, &'static str>| {
        Packed::build(
            map.into_iter()
                .map(|(name, extension)| {
                    let position = extension_names
                        .iter()
                        .position(|known| known == extension)
                        .unwrap_or_else(|| {
                            extension_names.push(extension.to_string());
                            extension_names.len() - 1
                        });
                    (
                        name,
                        vec![u8::try_from(position).expect("under 256 extensions")],
                    )
                })
                .collect(),
        )
    };
    let function_extensions = extensions_packed(function_extensions);
    let class_extensions = extensions_packed(class_extensions);
    let mut declared_records = RecordsBuilder::default();
    for declared in &declared_by_file {
        let mut record = Vec::new();
        for (is_class, name) in declared {
            // What `stub_dependents` hands out: each name as the key a
            // dependency on it is tracked under.
            let key = name.trim_start_matches('\\').to_ascii_lowercase();
            record.push(u8::from(*is_class));
            record.extend_from_slice(&(key.len() as u16).to_le_bytes());
            record.extend_from_slice(key.as_bytes());
        }
        declared_records.push(&record);
    }
    let index = StubIndex {
        class_names: to_packed(data.class_positions.keys().cloned().collect()),
        function_names: to_packed(data.function_positions.keys().cloned().collect()),
        constant_names: to_packed(data.constant_positions.keys().cloned().collect()),
        dependents: Packed::build(
            dependents
                .into_iter()
                .map(|(key, files)| (key, files.iter().flat_map(|f| f.to_le_bytes()).collect()))
                .collect(),
        ),
        declared_by_file: declared_records.finish(),
        declared_keys: Vec::new(),
        closures: Records::default(),
        file_closures: Vec::new(),
        function_extensions,
        class_extensions,
        extension_names,
        extensions: Vec::new(),
        phpstan_functions: name_records(
            data.functions
                .iter()
                .filter(|entry| entry.phpstan)
                .map(|entry| entry.name.as_str()),
        ),
        phpstan_classes: name_records(
            data.classes
                .iter()
                .filter(|entry| entry.phpstan)
                .map(|entry| entry.name.as_str()),
        ),
        phpstan_constants: name_records(
            data.constants
                .iter()
                .filter(|(name, ty)| data.builtin_constant(name) != Some(ty))
                .map(|(name, _)| name.as_str()),
        ),
    };
    let keys: Vec<DeclaredKeys> = (0..index.declared_by_file.len())
        .map(|file| index.parse_declared_keys(file))
        .collect();
    let index = StubIndex {
        closures: index.build_closures(&keys),
        ..index
    }
    .resolved();
    (index, data)
}

/// One record for each of `names`.
fn name_records<'a>(names: impl Iterator<Item = &'a str>) -> Records {
    let mut records = RecordsBuilder::default();
    for name in names {
        records.push(name.as_bytes());
    }
    records.finish()
}

#[cfg(test)]
mod tests;
