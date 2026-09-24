//! `cargo xtask oracle`: every fixture's recorded diagnostics compared
//! against real PHPStan's on the same code.
//!
//! PHPStan is the release `xtask/oracle/composer.json` pins (the one
//! `vendor/phpstan-src` checks out), installed under `target/oracle`. Each
//! fixture runs alone, since fixtures reuse class names, under a neon
//! translated from its `phpstan-rs.toml`: `level` as given (else `max`,
//! the level an unconfigured fixture's always-strict acceptance matches),
//! every setting that mirrors a PHPStan parameter or a strict-rules /
//! deprecation-rules switch, and scan and stub paths. Settings with no
//! PHPStan counterpart are left out; the identifiers they enable are
//! phpstan-rs rules PHPStan has no counterpart for.
//!
//! Diagnostics pair up by file, line and identifier; paired entries whose
//! messages differ are a disagreement of their own (side `message`). A
//! phpstan-rs entry whose identifier PHPStan never emits (per its sources
//! and this run) belongs to a phpstan-rs-only rule, and one PHPStan
//! reports only with bleedingEdge on is confirmed by it; both are counted,
//! not listed.
//!
//! Every other entry only phpstan-rs reports (side `rs`) needs a verdict
//! in `gates/oracle/verdicts.tsv`, one per line:
//! `fixture<TAB>rs<TAB>line<TAB>identifier<TAB>verdict<TAB>note`. The
//! command fails while one lacks a verdict or a verdict matches nothing.
//! Entries only PHPStan reports (side `phpstan`) and `message` pairs are
//! the parity backlog, tracked in `gates/oracle/parity.tsv`: each run
//! reports what is new and what is resolved against it, and `--bless`
//! rewrites it. Details go to `target/oracle/report.txt`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use regex::Regex;
use sha2::{Digest, Sha256};

use super::root;

/// phpstan-rs identifiers spelled differently from the PHPStan rule they
/// port.
const ALIASES: &[(&str, &str)] = &[("phpstanRs.assertType", "phpstan.type")];

/// phpstan-rs settings that map onto a PHPStan parameter of the same
/// meaning.
const PARAMETERS: &[(&str, &str)] = &[
    (
        "check_uninitialized_properties",
        "checkUninitializedProperties",
    ),
    ("check_function_name_case", "checkFunctionNameCase"),
    ("check_dynamic_properties", "checkDynamicProperties"),
    ("check_benevolent_union_types", "checkBenevolentUnionTypes"),
    (
        "check_missing_callable_signature",
        "checkMissingCallableSignature",
    ),
    (
        "check_strict_printf_placeholder_types",
        "checkStrictPrintfPlaceholderTypes",
    ),
    (
        "check_too_wide_throw_type_in_protected_and_public_methods",
        "checkTooWideThrowTypesInProtectedAndPublicMethods",
    ),
    (
        "check_too_wide_return_type_in_protected_and_public_methods",
        "checkTooWideReturnTypesInProtectedAndPublicMethods",
    ),
    (
        "check_too_wide_by_ref_type_in_protected_and_public_methods",
        "checkTooWideParameterOutInProtectedAndPublicMethods",
    ),
];

/// phpstan-rs settings that switch on one phpstan-strict-rules rule.
const STRICT_RULES: &[(&str, &str)] = &[
    ("check_boolean_conditions", "booleansInConditions"),
    (
        "check_parent_constructor_call",
        "requireParentConstructorCall",
    ),
    (
        "check_implicit_array_creation",
        "disallowedImplicitArrayCreation",
    ),
    ("check_useless_casts", "uselessCast"),
    ("check_strict_array_filter", "strictArrayFilter"),
    (
        "check_arithmetic_operands",
        "numericOperandsInArithmeticOperators",
    ),
    ("check_strict_function_calls", "strictFunctionCalls"),
    ("check_switch_case_types", "switchConditionsMatchingType"),
    (
        "check_loop_variable_overwrite",
        "overwriteVariablesWithLoop",
    ),
    ("check_dynamic_static_calls", "dynamicCallOnStaticMethod"),
    (
        "check_illegal_constructor_calls",
        "illegalConstructorMethodCall",
    ),
];

/// The parameters including phpstan-strict-rules changes besides its
/// rules, at PHPStan's own defaults: phpstan-rs switches each strict rule
/// on alone.
const STRICT_RULES_GLOBALS: &[(&str, &str)] = &[
    ("polluteScopeWithLoopInitialAssignments", "true"),
    ("polluteScopeWithAlwaysIterableForeach", "true"),
    ("polluteScopeWithBlock", "true"),
    ("checkExplicitMixedMissingReturn", "false"),
    ("checkInternalClassCaseSensitivity", "false"),
    ("reportMaybesInMethodSignatures", "false"),
    ("reportStaticMethodSignatures", "false"),
    ("reportMaybesInPropertyPhpDocTypes", "false"),
    ("reportWrongPhpDocTypeInVarTag", "false"),
];

fn oracle_dir() -> PathBuf {
    root().join("target/oracle")
}

fn vendor_dir() -> PathBuf {
    oracle_dir().join("vendor")
}

fn verdicts_file() -> PathBuf {
    root().join("gates/oracle/verdicts.tsv")
}

fn parity_file() -> PathBuf {
    root().join("gates/oracle/parity.tsv")
}

pub fn oracle(args: &[String]) -> Result<(), String> {
    let bless = args.iter().any(|a| a == "--bless");
    let filter = args.iter().find(|a| *a != "--bless").map(String::as_str);
    install()?;
    let lock = std::fs::read(root().join("xtask/oracle/composer.lock"))
        .map_err(|e| format!("xtask/oracle/composer.lock: {e}"))?;
    let fixtures_root = root().join("tests/fixtures");
    let mut stems = Vec::new();
    collect(&fixtures_root, &mut stems);
    stems.sort();
    let mut catalog = Catalog::from_sources()?;
    let mut outcomes = Vec::new();
    let selected: Vec<&PathBuf> = stems
        .iter()
        .filter(|stem| filter.is_none_or(|f| name(&fixtures_root, stem).contains(f)))
        .collect();
    for (i, stem) in selected.iter().enumerate() {
        if i % 250 == 0 {
            eprintln!("oracle: {i}/{} fixtures", selected.len());
        }
        let fixture = name(&fixtures_root, stem);
        let outcome =
            compare_fixture(stem, &fixture, &lock).map_err(|e| format!("{fixture}: {e}"))?;
        for d in outcome.phpstan.iter().chain(&outcome.bleeding_edge) {
            catalog.observed.insert(d.identifier.clone());
        }
        outcomes.push(outcome);
    }
    report(&outcomes, &catalog, filter.is_none(), bless)
}

/// Installs the pinned PHPStan unless `target/oracle` already holds the
/// locked versions.
fn install() -> Result<(), String> {
    let lock = root().join("xtask/oracle/composer.lock");
    let installed = vendor_dir().join("composer/installed.json");
    let current = std::fs::metadata(&installed)
        .and_then(|i| Ok(i.modified()? >= std::fs::metadata(&lock)?.modified()?))
        .unwrap_or(false);
    if current {
        return Ok(());
    }
    super::run(
        Command::new("composer")
            .args(["install", "--no-interaction", "--no-progress"])
            .current_dir(root().join("xtask/oracle")),
    )
}

/// The fixture stems under `dir`, as `tests/fixtures.rs` discovers them.
fn collect(dir: &Path, stems: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if !path.with_extension("expect").is_file() {
                collect(&path, stems);
            }
        } else if path.extension().is_some_and(|e| e == "expect") {
            stems.push(path.with_extension(""));
        }
    }
}

fn name(fixtures_root: &Path, stem: &Path) -> String {
    stem.strip_prefix(fixtures_root)
        .unwrap_or(stem)
        .display()
        .to_string()
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Entry {
    file: String,
    line: u32,
    identifier: String,
    message: String,
}

struct Outcome {
    fixture: String,
    rs: Vec<Entry>,
    phpstan: Vec<Entry>,
    /// PHPStan's diagnostics with bleedingEdge on.
    bleeding_edge: Vec<Entry>,
    /// Settings the neon could not express.
    unmapped: Vec<String>,
}

struct Translation {
    neon: String,
    paths: Vec<PathBuf>,
    /// Files whose contents the result depends on besides the neon.
    inputs: Vec<PathBuf>,
    level: Option<u8>,
    unmapped: Vec<String>,
}

fn compare_fixture(stem: &Path, fixture: &str, lock: &[u8]) -> Result<Outcome, String> {
    let single = stem.with_extension("php");
    let (base, subject) = if single.is_file() {
        (
            single.parent().unwrap_or(Path::new("")).to_path_buf(),
            single.clone(),
        )
    } else {
        (stem.to_path_buf(), stem.to_path_buf())
    };
    let config = match std::fs::read_to_string(stem.with_extension("toml")) {
        Ok(text) => text
            .parse::<toml::Table>()
            .map_err(|e| format!("{}.toml: {e}", stem.display()))?,
        Err(_) => toml::Table::new(),
    };
    let translation = translate(&config, &base, &subject)?;
    let expect = std::fs::read_to_string(stem.with_extension("expect"))
        .map_err(|e| format!("{}.expect: {e}", stem.display()))?;
    let mut rs = parse_expect(&expect, single.is_file(), &subject, translation.level)?;
    for entry in &mut rs {
        if let Some((_, theirs)) = ALIASES.iter().find(|(ours, _)| *ours == entry.identifier) {
            entry.identifier = (*theirs).to_string();
        }
    }
    let json = run_phpstan(&translation, &translation.neon, lock)?;
    let phpstan = parse_phpstan(&json, &base, single.is_file())?;
    let json = run_phpstan(&translation, &with_bleeding_edge(&translation.neon), lock)?;
    let bleeding_edge = parse_phpstan(&json, &base, single.is_file())?;
    Ok(Outcome {
        fixture: fixture.to_string(),
        rs,
        phpstan,
        bleeding_edge,
        unmapped: translation.unmapped,
    })
}

fn translate(config: &toml::Table, base: &Path, subject: &Path) -> Result<Translation, String> {
    let mut params: BTreeMap<&str, String> = BTreeMap::new();
    let mut includes = Vec::new();
    let mut unmapped = Vec::new();
    let mut strict = Vec::new();
    let mut paths = Vec::new();
    let mut inputs = vec![subject.to_path_buf()];
    let mut scan_dirs = Vec::new();
    let mut scan_files = Vec::new();
    let mut stub_files = Vec::new();
    let mut level = None;
    let mut php_version = None;
    let mut report_unmatched = false;
    let mut internal_tag = false;
    let mut exception_checks = Vec::new();
    let mut dynamic_constants = String::new();
    let mut no_composer = true;
    for (key, value) in config {
        let flag = value.as_bool();
        match key.as_str() {
            "paths" => paths = relative_paths(value, base)?,
            "scan_paths" | "stub_paths" => {
                for path in relative_paths(value, base)? {
                    inputs.push(path.clone());
                    if key == "stub_paths" {
                        stub_files.extend(php_files(&path));
                    } else if path.is_dir() {
                        scan_dirs.push(path);
                    } else {
                        scan_files.push(path);
                    }
                }
            }
            "level" => {
                level = Some(
                    value
                        .as_integer()
                        .and_then(|l| u8::try_from(l).ok())
                        .ok_or("level is not a number")?,
                );
            }
            "php_version" => {
                let text = value.as_str().ok_or("php_version is not a string")?;
                php_version = Some(php_version_id(text)?);
            }
            "no_composer" => no_composer = flag.unwrap_or(true),
            "report_unmatched_ignored_errors" => report_unmatched = flag.unwrap_or(false),
            "check_internal_usage" => internal_tag = flag.unwrap_or(false),
            "check_missing_checked_exceptions" => {
                if flag == Some(true) {
                    exception_checks.push("missingCheckedExceptionInThrows");
                }
            }
            "check_method_throw_type_covariance" => {
                if flag == Some(true) {
                    exception_checks.push("throwTypeCovariance");
                }
            }
            "check_deprecated_usage" => {
                if flag == Some(true) {
                    includes
                        .push(vendor_dir().join("phpstan/phpstan-deprecation-rules/rules.neon"));
                }
            }
            "dynamic_constant_names" => dynamic_constants = dynamic_constant_names(value)?,
            _ => {
                if let Some((_, parameter)) = PARAMETERS.iter().find(|(k, _)| k == key) {
                    let on = flag.ok_or_else(|| format!("{key} is not a boolean"))?;
                    params.insert(parameter, on.to_string());
                } else if let Some((_, rule)) = STRICT_RULES.iter().find(|(k, _)| k == key) {
                    if flag == Some(true) {
                        strict.push(*rule);
                    }
                } else if flag != Some(false) {
                    unmapped.push(key.clone());
                }
            }
        }
    }
    if paths.is_empty() {
        paths.push(subject.to_path_buf());
    }
    if !no_composer && base.join("vendor").is_dir() {
        scan_dirs.push(base.join("vendor"));
    }
    if !strict.is_empty() {
        includes.push(vendor_dir().join("phpstan/phpstan-strict-rules/rules.neon"));
        for (parameter, value) in STRICT_RULES_GLOBALS {
            params.entry(parameter).or_insert_with(|| value.to_string());
        }
        let mut rules = String::from("\n\t\tallRules: false\n\t\tbooleansInLoopConditions: false");
        for rule in &strict {
            write!(rules, "\n\t\t{rule}: true").unwrap();
        }
        params.insert("strictRules", rules);
    }
    let mut neon = String::new();
    if !includes.is_empty() {
        neon.push_str("includes:\n");
        for include in &includes {
            writeln!(neon, "\t- {}", quote(&include.display().to_string())).unwrap();
        }
    }
    neon.push_str("parameters:\n");
    match level {
        Some(level) => writeln!(neon, "\tlevel: {level}").unwrap(),
        None => neon.push_str("\tlevel: max\n"),
    }
    writeln!(
        neon,
        "\ttmpDir: {}",
        quote(&oracle_dir().join("tmp").display().to_string())
    )
    .unwrap();
    writeln!(neon, "\treportUnmatchedIgnoredErrors: {report_unmatched}").unwrap();
    if let Some(version) = php_version {
        writeln!(neon, "\tphpVersion: {version}").unwrap();
    }
    if internal_tag {
        neon.push_str("\tfeatureToggles:\n\t\tinternalTag: true\n");
    }
    if !exception_checks.is_empty() {
        neon.push_str("\texceptions:\n\t\tcheck:\n");
        for check in &exception_checks {
            writeln!(neon, "\t\t\t{check}: true").unwrap();
        }
    }
    for (parameter, list) in [
        ("scanDirectories", &scan_dirs),
        ("scanFiles", &scan_files),
        ("stubFiles", &stub_files),
    ] {
        if !list.is_empty() {
            writeln!(neon, "\t{parameter}:").unwrap();
            for path in list {
                writeln!(neon, "\t\t- {}", quote(&path.display().to_string())).unwrap();
            }
        }
    }
    neon.push_str(&dynamic_constants);
    for (parameter, value) in &params {
        writeln!(neon, "\t{parameter}: {value}").unwrap();
    }
    unmapped.sort();
    Ok(Translation {
        neon,
        paths,
        inputs,
        level,
        unmapped,
    })
}

fn relative_paths(value: &toml::Value, base: &Path) -> Result<Vec<PathBuf>, String> {
    value
        .as_array()
        .ok_or("a path list is not an array")?
        .iter()
        .map(|p| {
            p.as_str()
                .map(|p| base.join(p))
                .ok_or_else(|| "a path is not a string".to_string())
        })
        .collect()
}

fn php_files(path: &Path) -> Vec<PathBuf> {
    if !path.is_dir() {
        return vec![path.to_path_buf()];
    }
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(path) else {
        return out;
    };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            out.extend(php_files(&p));
        } else if p.extension().is_some_and(|e| e == "php") {
            out.push(p);
        }
    }
    out.sort();
    out
}

fn php_version_id(text: &str) -> Result<u32, String> {
    let mut parts = text.split('.').map(str::parse::<u32>);
    let mut next = || {
        parts
            .next()
            .transpose()
            .map_err(|e| format!("php_version {text}: {e}"))
    };
    let major = next()?.ok_or_else(|| format!("php_version {text}: empty"))?;
    let minor = next()?.unwrap_or(0);
    let patch = next()?.unwrap_or(0);
    Ok(major * 10000 + minor * 100 + patch)
}

/// `dynamicConstantNames` as explicit keys: an integer key names an
/// untyped constant, a name key gives its type.
fn dynamic_constant_names(value: &toml::Value) -> Result<String, String> {
    let mut out = String::from("\tdynamicConstantNames:\n");
    match value {
        toml::Value::Array(entries) => {
            for (i, entry) in entries.iter().enumerate() {
                match entry {
                    toml::Value::String(name) => writeln!(out, "\t\t{i}: {}", quote(name)).unwrap(),
                    toml::Value::Table(table) => {
                        let name = table
                            .get("name")
                            .and_then(toml::Value::as_str)
                            .ok_or("a dynamic_constant_names entry has no name")?;
                        match table.get("type").and_then(toml::Value::as_str) {
                            Some(ty) => {
                                writeln!(out, "\t\t{}: {}", quote(name), quote(ty)).unwrap()
                            }
                            None => writeln!(out, "\t\t{i}: {}", quote(name)).unwrap(),
                        }
                    }
                    _ => {
                        return Err(
                            "a dynamic_constant_names entry is neither a name nor a table".into(),
                        );
                    }
                }
            }
        }
        toml::Value::Table(table) => {
            for (name, ty) in table {
                let ty = ty
                    .as_str()
                    .ok_or("a dynamic_constant_names type is not a string")?;
                writeln!(out, "\t\t{}: {}", quote(name), quote(ty)).unwrap();
            }
        }
        _ => return Err("dynamic_constant_names is neither a list nor a table".into()),
    }
    Ok(out)
}

fn with_bleeding_edge(neon: &str) -> String {
    let include = format!(
        "\t- {}\n",
        quote(
            &vendor_dir()
                .join("phpstan/phpstan/conf/bleedingEdge.neon")
                .display()
                .to_string()
        )
    );
    match neon.strip_prefix("includes:\n") {
        Some(rest) => format!("includes:\n{include}{rest}"),
        None => format!("includes:\n{include}{neon}"),
    }
}

fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "''"))
}

/// Runs PHPStan on the translation unless a result for the same neon,
/// inputs and PHPStan install is cached.
fn run_phpstan(translation: &Translation, neon: &str, lock: &[u8]) -> Result<String, String> {
    let mut hasher = Sha256::new();
    hasher.update(lock);
    hasher.update(neon.as_bytes());
    for path in translation.paths.iter().chain(&translation.inputs) {
        hasher.update(path.display().to_string().as_bytes());
        for file in all_files(path) {
            hasher.update(file.display().to_string().as_bytes());
            hasher.update(std::fs::read(&file).unwrap_or_default());
        }
    }
    let key = format!("{:x}", hasher.finalize());
    let cached = oracle_dir().join("cache").join(format!("{key}.json"));
    if let Ok(json) = std::fs::read_to_string(&cached) {
        return Ok(json);
    }
    let mut config_hasher = Sha256::new();
    config_hasher.update(neon.as_bytes());
    let neon_file = oracle_dir()
        .join("configs")
        .join(format!("{:x}.neon", config_hasher.finalize()));
    write(&neon_file, neon.as_bytes())?;
    let out = Command::new("php")
        .args(["-d", "memory_limit=2G"])
        .arg(vendor_dir().join("phpstan/phpstan/phpstan"))
        .args(["analyse", "--no-progress", "--error-format=json", "-c"])
        .arg(&neon_file)
        .args(&translation.paths)
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("failed to start PHPStan: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let json = match stdout.find("{\"totals\"") {
        Some(start) => stdout[start..].trim().to_string(),
        None => {
            return Err(format!(
                "PHPStan produced no result ({}):\n{}{}",
                out.status,
                stdout,
                String::from_utf8_lossy(&out.stderr)
            ));
        }
    };
    write(&cached, json.as_bytes())?;
    Ok(json)
}

fn all_files(path: &Path) -> Vec<PathBuf> {
    if !path.is_dir() {
        return vec![path.to_path_buf()];
    }
    let mut out = Vec::new();
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            out.extend(all_files(&entry.path()));
        }
    }
    out.sort();
    out
}

fn write(path: &Path, contents: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::write(path, contents).map_err(|e| format!("{}: {e}", path.display()))
}

/// The entries of a fixture's `.expect` up to `level`, if set: the
/// fixture suite records every level, PHPStan reports up to the one it
/// runs at.
fn parse_expect(
    expect: &str,
    single: bool,
    subject: &Path,
    level: Option<u8>,
) -> Result<Vec<Entry>, String> {
    let entry = Regex::new(r"^(?:(.+?):)?(\d+):\d+-\d+:\d+ (\S+) L(\d+) .*? \| (.*)$").unwrap();
    let single_name = subject
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let mut out = Vec::new();
    for line in expect.lines().filter(|l| !l.starts_with("    ")) {
        let c = entry
            .captures(line)
            .ok_or_else(|| format!("unparseable expectation line: {line}"))?;
        if level.is_some_and(|max| c[4].parse::<u8>().unwrap_or(0) > max) {
            continue;
        }
        let file = if single {
            single_name.clone()
        } else {
            c.get(1)
                .map(|m| m.as_str().to_string())
                .ok_or_else(|| format!("expectation line without a path: {line}"))?
        };
        out.push(Entry {
            file,
            line: c[2].parse().unwrap_or(0),
            identifier: c[3].to_string(),
            message: unescape(&c[5]),
        });
    }
    Ok(out)
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('n') => out.push('\n'),
                Some(other) => out.push(other),
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn parse_phpstan(json: &str, base: &Path, single: bool) -> Result<Vec<Entry>, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("PHPStan JSON: {e}"))?;
    let mut out = Vec::new();
    if let Some(files) = value["files"].as_object() {
        for (path, file) in files {
            let path = Path::new(path);
            let file_name = if single {
                path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
            } else {
                path.strip_prefix(base)
                    .unwrap_or(path)
                    .display()
                    .to_string()
            };
            for message in file["messages"].as_array().into_iter().flatten() {
                out.push(Entry {
                    file: file_name.clone(),
                    line: message["line"].as_u64().unwrap_or(0) as u32,
                    identifier: message["identifier"]
                        .as_str()
                        .unwrap_or("(none)")
                        .to_string(),
                    message: portable(message["message"].as_str().unwrap_or(""), base),
                });
            }
        }
    }
    for error in value["errors"].as_array().into_iter().flatten() {
        out.push(Entry {
            file: String::new(),
            line: 0,
            identifier: "(general)".to_string(),
            message: portable(error.as_str().unwrap_or(""), base),
        });
    }
    out.sort();
    Ok(out)
}

/// The fixture suite's spelling of absolute paths in messages.
fn portable(text: &str, base: &Path) -> String {
    let mut out = text.to_string();
    let mut placeholder = String::from("<fixture>");
    let mut dir = Some(base);
    while let Some(d) = dir {
        let spelled = d.display().to_string();
        if spelled.len() > 1 {
            out = out.replace(&spelled, &placeholder);
        }
        placeholder.push_str("/..");
        dir = d.parent();
    }
    out
}

/// Identifiers PHPStan can emit: identifier-shaped literals and
/// `sprintf` identifier templates in the pinned sources, plus whatever
/// this run saw.
struct Catalog {
    literals: BTreeSet<String>,
    templates: Vec<Regex>,
    observed: BTreeSet<String>,
}

impl Catalog {
    fn from_sources() -> Result<Catalog, String> {
        let literal = Regex::new(r"'([a-z][A-Za-z0-9]*(?:\.[A-Za-z0-9]+)+)'").unwrap();
        let template = Regex::new(r"identifier\(\s*sprintf\(\s*'([^']*%[^']*)'").unwrap();
        let mut catalog = Catalog {
            literals: BTreeSet::new(),
            templates: Vec::new(),
            observed: BTreeSet::new(),
        };
        let sources = [
            root().join("vendor/phpstan-src/src"),
            vendor_dir().join("phpstan/phpstan-strict-rules/src"),
            vendor_dir().join("phpstan/phpstan-deprecation-rules/src"),
        ];
        let mut seen_templates = BTreeSet::new();
        for dir in &sources {
            if !dir.is_dir() {
                return Err(format!("{} is missing", dir.display()));
            }
            for file in php_files(dir) {
                let text = std::fs::read_to_string(&file).unwrap_or_default();
                for c in literal.captures_iter(&text) {
                    catalog.literals.insert(c[1].to_string());
                }
                for c in template.captures_iter(&text) {
                    seen_templates.insert(c[1].to_string());
                }
            }
        }
        for t in seen_templates {
            let pattern = regex::escape(&t)
                .replace("%s", "[A-Za-z0-9]+")
                .replace("%d", "[0-9]+");
            catalog
                .templates
                .push(Regex::new(&format!("^{pattern}$")).unwrap());
        }
        Ok(catalog)
    }

    fn knows(&self, identifier: &str) -> bool {
        self.literals.contains(identifier)
            || self.observed.contains(identifier)
            || self.templates.iter().any(|t| t.is_match(identifier))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Side {
    Rs,
    Phpstan,
    Message,
}

impl Side {
    fn label(self) -> &'static str {
        match self {
            Side::Rs => "rs",
            Side::Phpstan => "phpstan",
            Side::Message => "message",
        }
    }
}

struct Disagreement {
    fixture: String,
    side: Side,
    rs: Option<Entry>,
    phpstan: Option<Entry>,
}

impl Disagreement {
    fn entry(&self) -> &Entry {
        self.rs
            .as_ref()
            .or(self.phpstan.as_ref())
            .expect("a disagreement has a side")
    }

    /// `fixture<TAB>side<TAB>line<TAB>identifier`, as the verdict and
    /// parity files spell it.
    fn key(&self) -> String {
        let entry = self.entry();
        format!(
            "{}\t{}\t{}\t{}",
            self.fixture,
            self.side.label(),
            entry.line,
            entry.identifier
        )
    }

    fn describe(&self, out: &mut String) {
        let entry = self.entry();
        writeln!(
            out,
            "  {} {} {}",
            self.side.label(),
            entry.line,
            entry.identifier
        )
        .unwrap();
        for (who, e) in [("rs", &self.rs), ("phpstan", &self.phpstan)] {
            if let Some(e) = e {
                writeln!(
                    out,
                    "    {who:8} {}:{} {}",
                    e.file,
                    e.line,
                    e.message.replace('\n', "\\n")
                )
                .unwrap();
            }
        }
    }
}

#[derive(Default)]
struct Tallies {
    /// phpstan-rs entries of rules PHPStan has no counterpart for.
    rs_rules: BTreeMap<String, usize>,
    /// phpstan-rs entries only PHPStan's bleedingEdge reports.
    bleeding_edge: BTreeMap<String, usize>,
}

/// Pairs one fixture's entries: exact matches first, then same file,
/// line and identifier with different messages.
fn pair(outcome: &Outcome, catalog: &Catalog, tallies: &mut Tallies) -> Vec<Disagreement> {
    let same_place =
        |a: &Entry, b: &Entry| a.file == b.file && a.line == b.line && a.identifier == b.identifier;
    let mut rs: Vec<Option<&Entry>> = outcome.rs.iter().map(Some).collect();
    let mut phpstan: Vec<Option<&Entry>> = outcome.phpstan.iter().map(Some).collect();
    for r in rs.iter_mut() {
        if let Some(entry) = *r
            && let Some(p) = phpstan.iter_mut().find(|p| p.is_some_and(|p| p == entry))
        {
            *r = None;
            *p = None;
        }
    }
    let mut out = Vec::new();
    for r in rs.iter_mut() {
        let Some(entry) = *r else { continue };
        if let Some(p) = phpstan
            .iter_mut()
            .find(|p| p.is_some_and(|p| same_place(p, entry)))
        {
            out.push(Disagreement {
                fixture: outcome.fixture.clone(),
                side: Side::Message,
                rs: Some(entry.clone()),
                phpstan: p.cloned(),
            });
            *r = None;
            *p = None;
        }
    }
    for entry in rs.into_iter().flatten() {
        if !catalog.knows(&entry.identifier) {
            *tallies
                .rs_rules
                .entry(entry.identifier.clone())
                .or_default() += 1;
        } else if outcome.bleeding_edge.iter().any(|b| same_place(b, entry)) {
            *tallies
                .bleeding_edge
                .entry(entry.identifier.clone())
                .or_default() += 1;
        } else {
            out.push(Disagreement {
                fixture: outcome.fixture.clone(),
                side: Side::Rs,
                rs: Some(entry.clone()),
                phpstan: None,
            });
        }
    }
    for entry in phpstan.into_iter().flatten() {
        out.push(Disagreement {
            fixture: outcome.fixture.clone(),
            side: Side::Phpstan,
            rs: None,
            phpstan: Some(entry.clone()),
        });
    }
    out
}

/// `gates/oracle/verdicts.tsv`: `key<TAB>verdict<TAB>note` per line.
fn load_verdicts() -> Result<BTreeMap<String, String>, String> {
    let text = std::fs::read_to_string(verdicts_file()).unwrap_or_default();
    let mut out = BTreeMap::new();
    for line in text
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        let fields: Vec<&str> = line.splitn(6, '\t').collect();
        let [fixture, side, at, identifier, verdict, _note] = fields[..] else {
            return Err(format!("verdicts.tsv: malformed line: {line}"));
        };
        if side != "rs" {
            return Err(format!(
                "verdicts.tsv: side {side} needs no verdict: {line}"
            ));
        }
        out.insert(
            format!("{fixture}\t{side}\t{at}\t{identifier}"),
            verdict.to_string(),
        );
    }
    Ok(out)
}

/// `gates/oracle/parity.tsv`: one key per line, as a multiset.
fn load_parity() -> BTreeMap<String, usize> {
    let mut out = BTreeMap::new();
    let text = std::fs::read_to_string(parity_file()).unwrap_or_default();
    for line in text
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        *out.entry(line.to_string()).or_default() += 1;
    }
    out
}

fn report(
    outcomes: &[Outcome],
    catalog: &Catalog,
    complete: bool,
    bless: bool,
) -> Result<(), String> {
    let verdicts = load_verdicts()?;
    let recorded_parity = load_parity();
    let fixtures: BTreeSet<&str> = outcomes.iter().map(|o| o.fixture.as_str()).collect();
    let mut tallies = Tallies::default();
    let mut unmapped: BTreeMap<&str, usize> = BTreeMap::new();
    let mut by_identifier: BTreeMap<(Side, String), usize> = BTreeMap::new();
    let mut by_verdict: BTreeMap<&str, usize> = BTreeMap::new();
    let mut used_verdicts = BTreeSet::new();
    let mut unclassified = String::new();
    let mut unclassified_count = 0usize;
    let mut parity: BTreeMap<String, usize> = BTreeMap::new();
    let mut parity_details: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for outcome in outcomes {
        for setting in &outcome.unmapped {
            *unmapped.entry(setting).or_default() += 1;
        }
        let mut section = String::new();
        for d in pair(outcome, catalog, &mut tallies) {
            let key = d.key();
            *by_identifier
                .entry((d.side, d.entry().identifier.clone()))
                .or_default() += 1;
            if d.side != Side::Rs {
                *parity.entry(key.clone()).or_default() += 1;
                let mut text = String::new();
                d.describe(&mut text);
                parity_details.entry(key).or_default().push(text);
            } else if let Some(verdict) = verdicts.get(&key) {
                used_verdicts.insert(key);
                *by_verdict.entry(verdict).or_default() += 1;
            } else {
                unclassified_count += 1;
                d.describe(&mut section);
            }
        }
        if !section.is_empty() {
            writeln!(unclassified, "{}", outcome.fixture).unwrap();
            if !outcome.unmapped.is_empty() {
                writeln!(
                    unclassified,
                    "  (unmapped: {})",
                    outcome.unmapped.join(", ")
                )
                .unwrap();
            }
            unclassified.push_str(&section);
        }
    }
    let recorded: BTreeMap<&String, usize> = recorded_parity
        .iter()
        .filter(|(key, _)| {
            key.split('\t')
                .next()
                .is_some_and(|fixture| fixtures.contains(fixture))
        })
        .map(|(key, count)| (key, *count))
        .collect();
    let mut added = String::new();
    let mut added_count = 0usize;
    for (key, count) in &parity {
        let before = recorded.get(key).copied().unwrap_or(0);
        if *count > before {
            added_count += count - before;
            writeln!(added, "{}", key.split('\t').next().unwrap_or("")).unwrap();
            for text in &parity_details[key] {
                added.push_str(text);
            }
        }
    }
    let mut resolved = String::new();
    let mut resolved_count = 0usize;
    for (key, before) in &recorded {
        let now = parity.get(*key).copied().unwrap_or(0);
        if *before > now {
            resolved_count += before - now;
            writeln!(resolved, "{key} (x{})", before - now).unwrap();
        }
    }

    let mut summary = String::new();
    writeln!(summary, "fixtures: {}", outcomes.len()).unwrap();
    for (title, tally) in [
        ("phpstan-rs-only rules", &tallies.rs_rules),
        ("confirmed by bleedingEdge only", &tallies.bleeding_edge),
    ] {
        writeln!(
            summary,
            "{title}: {} entries",
            tally.values().sum::<usize>()
        )
        .unwrap();
        for (identifier, count) in tally {
            writeln!(summary, "  {count:6} {identifier}").unwrap();
        }
    }
    writeln!(summary, "disagreements by side and identifier:").unwrap();
    let mut ranked: Vec<_> = by_identifier.iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for ((side, identifier), count) in ranked {
        writeln!(summary, "  {count:6} {:7} {identifier}", side.label()).unwrap();
    }
    writeln!(summary, "settings without a PHPStan counterpart:").unwrap();
    for (setting, count) in &unmapped {
        writeln!(summary, "  {count:6} {setting}").unwrap();
    }
    writeln!(summary, "phpstan-rs-only, classified: {by_verdict:?}").unwrap();
    writeln!(
        summary,
        "phpstan-rs-only, unclassified: {unclassified_count}"
    )
    .unwrap();
    writeln!(
        summary,
        "parity: {} disagreements, {added_count} new and {resolved_count} resolved since parity.tsv",
        parity.values().sum::<usize>()
    )
    .unwrap();
    write(
        &oracle_dir().join("report.txt"),
        format!(
            "{summary}\n== phpstan-rs-only, unclassified\n{unclassified}\n== parity, new\n{added}\n== parity, resolved\n{resolved}"
        )
        .as_bytes(),
    )?;
    eprint!("{summary}");
    eprintln!("details: {}", oracle_dir().join("report.txt").display());

    if bless {
        if !complete {
            return Err("--bless needs the whole suite (no filter)".into());
        }
        let mut text = String::from(
            "# Fixture disagreements with PHPStan outside phpstan-rs-only reports:\n# fixture, side (phpstan: only PHPStan reports it; message: both do,\n# worded differently), line, identifier. Written by\n# `cargo xtask oracle --bless`.\n",
        );
        for (key, count) in &parity {
            for _ in 0..*count {
                writeln!(text, "{key}").unwrap();
            }
        }
        write(&parity_file(), text.as_bytes())?;
        eprintln!("wrote {}", parity_file().display());
    }
    let stale: Vec<&String> = verdicts
        .keys()
        .filter(|k| !used_verdicts.contains(*k))
        .filter(|k| k.split('\t').next().is_some_and(|f| fixtures.contains(f)))
        .collect();
    for key in &stale {
        eprintln!("stale verdict: {key}");
    }
    if !stale.is_empty() {
        return Err(format!(
            "{} verdicts no longer match a disagreement",
            stale.len()
        ));
    }
    if unclassified_count > 0 {
        return Err(format!(
            "{unclassified_count} unclassified phpstan-rs-only disagreements"
        ));
    }
    Ok(())
}
