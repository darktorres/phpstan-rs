use super::super::*;
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::registry::Registry;

/// `missingType.*` (this project's `MissingTypehintCheck` family -
/// untyped params/properties/returns, bare `array`/`iterable`,
/// non-generic use of a generic class) fires on almost any declaration
/// that isn't fully, precisely typed - which describes the overwhelming
/// majority of this whole suite's own minimal, deliberately-untyped
/// fixtures, none of which were written with this family in mind.
/// PHPStan's own test suite never hits this problem, since
/// `RuleTestCase` instantiates exactly one rule at a time; this
/// project's `Checker` runs every check unconditionally (see
/// `checker::Diagnostic::level`'s own doc comment - level is metadata
/// for the CLI's own post-hoc `--level` filtering, not something that
/// gates whether a check runs at all), so
/// `diagnostics_for`/`diagnostics_for_with` filtering this one family
/// out by default is what keeps every pre-existing test's own
/// "diagnostics_for returns exactly what this fixture is actually
/// testing" assumption true, without touching any of those tests' own
/// content. CLI usage is unaffected either way - `main.rs`'s own
/// default (`unwrap_or(MAX_LEVEL)`) already surfaces every level-6
/// diagnostic, `missingType.*` included, with no equivalent test-only
/// exclusion. A test that specifically wants to see this family
/// (`checker/tests/missing_typehint.rs`) uses `all_diagnostics_for`
/// instead.
fn is_missing_typehint_diagnostic(d: &Diagnostic) -> bool {
    d.identifier.starts_with("missingType.")
}

/// The transitive-purity payoff (`checker::noop::
/// check_pure_call_without_effect`) fires on a bare, discarded call to
/// any declaration this project's own registry can prove effect-free -
/// which, unlike the family it sits alongside
/// (`logicalXor.resultUnused`/`booleanAnd.resultUnused`/
/// `booleanOr.resultUnused`/`ternary.resultUnused`, deliberately left
/// unfiltered here - `checker/tests/noop.rs` relies on seeing those by
/// default), describes an enormous number of this whole suite's own
/// tiny helper functions/methods, called as a bare statement purely as
/// test setup incidental to whatever each fixture actually means to
/// exercise (the identical `missingType.*` problem above, for a
/// different diagnostic family). A test that specifically wants to see
/// this one (`checker/tests/purity.rs`) uses `all_diagnostics_for`
/// instead.
fn is_pure_call_result_unused_diagnostic(d: &Diagnostic) -> bool {
    matches!(
        d.identifier.as_str(),
        "function.resultUnused"
            | "method.resultUnused"
            | "staticMethod.resultUnused"
            | "new.resultUnused"
    )
}

/// NoopRule's generic `expr.resultUnused` fires on a bare `$a + $b;`,
/// which is how many fixtures probe an expression's own diagnostics.
/// `checker/tests/noop.rs` uses `all_diagnostics_for` to see it.
fn is_generic_noop_diagnostic(d: &Diagnostic) -> bool {
    d.identifier == "expr.resultUnused"
}

/// `TooWideFunctionReturnTypehintRule`'s own
/// `return.unusedType` fires unconditionally for a plain function (no
/// opt-in gate the way the method-side variant has) whenever a declared
/// `Union` return-type member is never actually produced by the analyzed
/// body - which describes a large share of this whole suite's own
/// minimal fixtures (a `array|false`-declared helper whose trivial body
/// only ever returns the array half, an `int|string`-declared one that
/// only exercises one branch, ...), none of which were written with this
/// check in mind. The identical `missingType.*`/`noop` problem above, for
/// a third family - see `is_missing_typehint_diagnostic`'s own doc
/// comment. A test that specifically wants to see this
/// (`checker/tests/return_type_too_wide.rs`) uses `all_diagnostics_for`
/// instead.
fn is_too_wide_return_type_diagnostic(d: &Diagnostic) -> bool {
    d.identifier == "return.unusedType"
}

fn is_too_wide_property_type_diagnostic(d: &Diagnostic) -> bool {
    d.identifier == "property.unusedType" || d.identifier == "staticProperty.unusedType"
}

pub(super) fn diagnostics_for(src: &str) -> Vec<Diagnostic> {
    diagnostics_for_with(src, |_| {})
}

pub(super) fn diagnostics_for_with(
    src: &str,
    configure: impl FnOnce(&mut Registry),
) -> Vec<Diagnostic> {
    let mut diagnostics = analyse(src, configure, true);
    diagnostics.retain(|d| {
        !is_missing_typehint_diagnostic(d)
            && !is_pure_call_result_unused_diagnostic(d)
            && !is_generic_noop_diagnostic(d)
            && !is_too_wide_return_type_diagnostic(d)
            && !is_too_wide_property_type_diagnostic(d)
    });
    diagnostics
}

/// The unfiltered twin of `diagnostics_for` - every diagnostic
/// `Checker::check_program` actually produced, `missingType.*` included.
/// Only `checker/tests/missing_typehint.rs` itself should need this.
pub(super) fn all_diagnostics_for(src: &str) -> Vec<Diagnostic> {
    all_diagnostics_for_with(src, |_| {})
}

pub(super) fn all_diagnostics_for_with(
    src: &str,
    configure: impl FnOnce(&mut Registry),
) -> Vec<Diagnostic> {
    analyse(src, configure, false)
}

fn stubbed_registry(program: &[crate::ast::Stmt]) -> Registry {
    let mut registry = Registry::build(program);
    registry.merge_stubs(crate::php_stubs::generated_stubs());
    registry.merge_stub_classes(crate::php_stubs::generated_stub_classes());
    registry.merge_stub_constants(crate::php_stubs::generated_stub_constants());
    registry
}

fn analyse(src: &str, configure: impl FnOnce(&mut Registry), filtered: bool) -> Vec<Diagnostic> {
    let tokens = Lexer::new(src).tokenize_with_spans();
    let program = Parser::new(tokens).parse_program().unwrap();
    let mut registry = stubbed_registry(&program);
    configure(&mut registry);
    if std::env::var_os("PHPSTAN_RS_CAPTURE").is_some() {
        capture(src, None, &stubbed_registry(&program), &registry, filtered);
    }
    if std::env::var_os("PHPSTAN_RS_REPLAY").is_some()
        && let Some(diagnostics) = replay()
    {
        return diagnostics;
    }
    // `merge_stub_classes`/`configure` can both add classes/extends edges
    // after `Registry::build`'s own internal precompute already ran -
    // rebuild `ancestor_names`/member-owner maps once more here, now that
    // the registry is actually settled, so `is_subclass_of`/`lookup_
    // method`/etc. see every merged-in stub class too (see `precompute_
    // ancestor_names`/`precompute_member_owners`'s own doc comments).
    registry.precompute_ancestor_names();
    registry.precompute_member_owners();
    let mut checker = Checker::new(&registry);
    checker.check_program(&program);
    checker.into_diagnostics()
}

/// `diagnostics_for`, checking `src` as if it were the file at `path`.
pub(super) fn diagnostics_for_file(src: &str, path: &std::path::Path) -> Vec<Diagnostic> {
    let tokens = Lexer::new(src).tokenize_with_spans();
    let program = Parser::new(tokens).parse_program().unwrap();
    let mut registry = stubbed_registry(&program);
    if std::env::var_os("PHPSTAN_RS_CAPTURE").is_some() {
        capture(src, Some(path), &registry, &registry, false);
    }
    registry.precompute_ancestor_names();
    registry.precompute_member_owners();
    let mut checker = Checker::new(&registry);
    checker.set_current_file(path);
    checker.check_program(&program);
    checker.into_diagnostics()
}

/// With `PHPSTAN_RS_REPLAY` set, the fixture recorded for this call (per
/// `tests/fixtures/MAPPING.tsv`) is analysed through the `check-project`
/// pipeline instead, so the test's own assertions check that pipeline's
/// output - the proof that the fixture preserves what the test checked.
/// `None` for a call with no fixture.
fn replay() -> Option<Vec<Diagnostic>> {
    thread_local! {
        static CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }
    static MAPPING: std::sync::OnceLock<rustc_hash::FxHashMap<(String, usize), String>> =
        std::sync::OnceLock::new();
    let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mapping = MAPPING.get_or_init(|| {
        let text = std::fs::read_to_string(fixtures.join("MAPPING.tsv")).unwrap_or_default();
        text.lines()
            .skip(1)
            .filter_map(|line| {
                let mut fields = line.split('\t');
                let test = fields.next()?.to_string();
                let index = fields.next()?.parse().ok()?;
                let fixture = fields.next()?.to_string();
                Some(((test, index), fixture))
            })
            .collect()
    });
    let index = CALLS.with(|c| {
        let n = c.get();
        c.set(n + 1);
        n
    });
    let test = std::thread::current().name()?.to_string();
    let fixture = mapping.get(&(test, index))?;
    let (session, _) = crate::project::session::Session::for_fixture(&fixtures.join(fixture))
        .expect("replayed fixture resolves");
    let analysis = session.run(None).expect("replayed fixture runs");
    Some(analysis.results.into_values().flatten().collect())
}

/// When `PHPSTAN_RS_CAPTURE` names a directory, every analysis call writes
/// a record there (`<test>__<n>.json`): its source, its settings as
/// `phpstan-rs.toml` keys, and whatever the settings can't express. The
/// fixture converter builds the fixture suite from these records.
fn capture(
    src: &str,
    file: Option<&std::path::Path>,
    defaults: &Registry,
    configured: &Registry,
    filtered: bool,
) {
    thread_local! {
        static CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }
    let Some(dir) = std::env::var_os("PHPSTAN_RS_CAPTURE") else {
        return;
    };
    let index = CALLS.with(|c| {
        let n = c.get();
        c.set(n + 1);
        n
    });
    let test = std::thread::current()
        .name()
        .unwrap_or("unnamed")
        .to_string();
    let mut toml = String::new();
    let mut unsupported: Vec<&str> = Vec::new();
    macro_rules! bools {
        ($($field:ident,)*) => {
            $(
                if configured.$field != defaults.$field {
                    toml.push_str(&format!("{} = {}\n", stringify!($field), configured.$field));
                }
            )*
        };
    }
    bools! {
        strict_mixed_arrays,
        check_uninitialized_properties,
        check_internal_usage,
        check_function_name_case,
        check_deprecated_usage,
        check_missing_checked_exceptions,
        check_empty_method_bodies,
        check_strict_printf_placeholder_types,
        check_too_wide_throw_type_in_protected_and_public_methods,
        check_too_wide_return_type_in_protected_and_public_methods,
        check_too_wide_by_ref_type_in_protected_and_public_methods,
        check_method_throw_type_covariance,
        check_duplicate_function_declarations,
        check_duplicate_class_declarations,
        check_boolean_trap,
        check_array_key_cast,
        check_parent_constructor_call,
        check_promoted_property_suggestions,
        check_boolean_conditions,
        check_dynamic_properties,
        check_swapped_arguments,
        check_never_return_suggestions,
        check_switch_missing_default,
        check_throw_without_message,
        check_throw_base_class,
        check_exception_hygiene,
        check_current_time_dependence,
        check_psr3_exception_context,
        check_psr3_logging,
        check_magic_numbers,
        magic_numbers_ignore_numeric_strings,
        check_native_type_hints,
        check_implicit_array_creation,
        check_useless_casts,
        check_strict_array_filter,
        check_mixed_type_operands,
        check_comparable_types,
        check_arithmetic_operands,
        check_identity_vs_equals,
        check_loose_haystack_comparison,
        check_strict_function_calls,
        check_switch_case_types,
        check_loop_variable_overwrite,
        check_dynamic_static_calls,
        check_illegal_constructor_calls,
        check_enum_exhaustiveness,
        check_null_coercion,
        check_new_exception_discarded,
        check_risky_tests,
        check_dead_enum_cases,
        check_dead_code,
        check_circular_dependencies,
        check_dead_abstract_classes,
        check_dead_members,
        check_renamed_named_arguments,
        check_benevolent_union_types,
        check_missing_callable_signature,
    }
    if configured.configured_level != defaults.configured_level
        && let Some(level) = configured.configured_level
    {
        toml.push_str(&format!("level = {level}\n"));
    }
    if configured.php_version != defaults.php_version
        && let Some((major, minor)) = configured.php_version
    {
        toml.push_str(&format!("php_version = \"{major}.{minor}\"\n"));
    }
    if configured
        .psr3_context_key_pattern
        .as_ref()
        .map(regex::Regex::as_str)
        != defaults
            .psr3_context_key_pattern
            .as_ref()
            .map(regex::Regex::as_str)
        && let Some(pattern) = &configured.psr3_context_key_pattern
    {
        toml.push_str(&format!(
            "psr3_context_key_pattern = {:?}\n",
            pattern.as_str()
        ));
    }
    if configured.magic_number_exemptions != defaults.magic_number_exemptions
        && let Some(numbers) = &configured.magic_number_exemptions
    {
        let list: Vec<String> = numbers.iter().map(|n| format!("{n:?}")).collect();
        toml.push_str(&format!(
            "magic_number_exemptions = [{}]\n",
            list.join(", ")
        ));
    }
    if configured.phpstan_api_check_enabled != defaults.phpstan_api_check_enabled {
        unsupported.push("phpstan_api_check_enabled");
    }
    if format!("{:?}", configured.restricted_symbols)
        != format!("{:?}", defaults.restricted_symbols)
    {
        unsupported.push("restricted_symbols");
    }
    if format!("{:?}", configured.dynamic_constant_names)
        != format!("{:?}", defaults.dynamic_constant_names)
    {
        unsupported.push("dynamic_constant_names");
    }
    if format!("{:?}", configured.doctrine_custom_functions)
        != format!("{:?}", defaults.doctrine_custom_functions)
    {
        unsupported.push("doctrine_custom_functions");
    }
    if configured.sql_reflection_schema.is_some() || configured.sql_reflection_config.is_some() {
        unsupported.push("sql_reflection");
    }
    if configured.configured_assertions.len() != defaults.configured_assertions.len()
        || configured.configured_class_argument_returns.len()
            != defaults.configured_class_argument_returns.len()
    {
        unsupported.push("assertion_tables");
    }
    if configured.classes.len() != defaults.classes.len()
        || configured.functions.len() != defaults.functions.len()
        || configured.global_constants.len() != defaults.global_constants.len()
    {
        unsupported.push("declarations");
    }
    let record = serde_json::json!({
        "test": test,
        "index": index,
        "source": src,
        "file": file.map(|f| f.display().to_string()),
        "toml": toml,
        "unsupported": unsupported,
        "filtered": filtered,
    });
    let name = format!("{}__{index}.json", test.replace("::", "__"));
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(std::path::Path::new(&dir).join(name), record.to_string());
}
