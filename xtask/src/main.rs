//! Regression gates for phpstan-rs (`cargo xtask <command>`).
//!
//! Snapshots live in the gate's store, `.corpus-runs/gate/store.git`
//! (gitignored, `store`), each a commit under `refs/snapshots/<commit>`,
//! keyed by the full hash of the commit the binary was built from, with an
//! entry for each artifact. `compare` checks HEAD's snapshot against its
//! parent's (or, when the parent has none, the nearest ancestor that has
//! them): every entry must be the same unless HEAD's own commit adds
//! `gates/approved/<slug>/<entry>.diff` holding exactly the store's
//! `git diff` of it.
//!
//! The corpus is the frozen Piwigo checkout at `$PHPSTAN_RS_CORPUS`
//! (default `/home/torres/piwigo17-frozen`).

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::io::BufRead as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::Instant;

use store::{Content, Snapshots, Store, snapshot_ref, text_content};

mod arch;
mod modgraph;
mod pass;
mod profile;
mod progress;
mod store;
mod stub_index;
#[cfg(test)]
mod tests;

const LEVELS: [&str; 12] = [
    "unset", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10",
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("gates") => gates(&args[1..]),
        Some("corpus") => corpus_command(&args[1..]),
        Some("progress") => progress::progress_command(&gate_dir(), &args[1..]),
        Some("profile") => profile::profile_command(&args[1..]),
        Some("compare") => compare(&args[1..]),
        Some("type-drift") => type_drift(&args[1..]),
        Some("perf") => perf(),
        Some("buildtime") => buildtime(),
        Some("tests") => test_manifest(),
        Some("modgraph") => modgraph::modgraph(&root(), &args[1..]),
        Some("arch") => arch::arch(&root()),
        Some("stub-index") => write_stub_index(),
        Some("baseline") => baseline_generate(),
        Some("approve") => approve(&args[1..]),
        Some("rekey") => rekey(&args[1..]),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("xtask: {e}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "usage: cargo xtask <command>
  gates [--full]         every gate for HEAD (quick tier by default)
  corpus [--full] [--profile <gate|oracle>]
                         one pass over the corpus on a worker per core:
                         the scoped and all-on reports, their types and
                         baselines; with --full every configuration in
                         each of its states with every artifact compared
                         (the reports, the front end, the registry, the
                         stub bases, the checker's records), the levels,
                         the server's sessions, the edit replays, the
                         vendor projects and the fixtures; records HEAD's
                         snapshot in the store once every run has passed,
                         or with --profile oracle runs the oracle build and
                         records nothing
  progress [--watch] [<command>]
                         show how far the running or last `corpus` (or
                         <command>) is: runs done by group, runs under
                         way, failures; --watch refreshes it until it ends
  profile <kind> [<subject>] [--threads <n>] [--file-cache]
                         the frame-pointer `dhat` build of HEAD, a warm
                         run's time and peak memory and callgrind of one
                         run the pass makes, with a README, in
                         ~/.cache/prs-profile; the kinds are vendor and
                         replay (a vendor project, default
                         vendor/amphp/amp), config (scoped, all-on,
                         unscoped, vendor-tree or arch), session (`.` or a
                         vendor project) and fixtures (a filter)
  compare [--full] [--against <hash>]
                         diff HEAD's snapshot against its parent's (or
                         the nearest ancestor that has one) in the store
  type-drift --against <hash> [--artifact <types-scoped|types-all-on|types-fixtures>]
                         relate every binding of HEAD's type snapshot to
                         <hash>'s and write the drift report
  perf                   record wall time and peak memory of the scoped run
  buildtime              record incremental rebuild times
  tests                  record and check the test-name manifest
  modgraph [--inventory] check the module graph against
                         gates/module-graph.txt (--inventory lists every
                         crossing edge)
  arch                   check the architecture rules
  stub-index             regenerate crates/stub-sources/index.tsv
  baseline               generate the reference baselines (Stage A only)
  approve <slug> [--full] [--against <commit>]
                         write HEAD's snapshot diffs against the same base
                         as compare (or <commit>) to gates/approved/<slug>/
                         (then amend them in)
  rekey <old-hash>       move <old-hash>'s snapshot to HEAD after an amend
                         that only added gates/ files

`gates` and `corpus` write each step's wall time to
.corpus-runs/gate/gatetime/<commit>.<command>.tsv, also when a step fails.";

/// The embedded stub corpora, in index priority order.
pub(crate) fn stub_corpora() -> Vec<stub_index::Corpus<'static>> {
    vec![
        stub_index::Corpus {
            tag: "c",
            files: stub_sources::PHPSTORM_STUBS_PATHS
                .iter()
                .zip(stub_sources::PHPSTORM_STUBS_SOURCES)
                .zip(stub_sources::PHPSTORM_STUBS_EXTENSIONS)
                .map(|((path, source), extension)| (*path, *source, Some(*extension)))
                .collect(),
        },
        stub_index::Corpus {
            tag: "s",
            files: stub_sources::PHPSTAN_SRC_PATHS
                .iter()
                .zip(stub_sources::PHPSTAN_SRC_SOURCES)
                .map(|(path, source)| (*path, *source, None))
                .collect(),
        },
    ]
}

fn write_stub_index() -> Result<(), String> {
    let index = stub_index::generate(&stub_corpora());
    write(
        &root().join("crates/stub-sources/index.tsv"),
        index.as_bytes(),
    )
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask lives one level below the repository root")
        .to_path_buf()
}

fn corpus_dir() -> PathBuf {
    std::env::var_os("PHPSTAN_RS_CORPUS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/home/torres/piwigo17-frozen"))
}

/// Where snapshots are recorded and compared, and where runs keep their
/// scratch: this checkout's `.corpus-runs/gate`, so runs in two worktrees
/// never share a scratch file, or the store `PHPSTAN_RS_GATE_DIR` names
/// for one that reads another's.
fn gate_dir() -> PathBuf {
    std::env::var_os("PHPSTAN_RS_GATE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join(".corpus-runs/gate"))
}

/// The `gate` profile's `phpstan-rs`: what the determinism check runs
/// against the runner, and what generates the reference baselines.
fn binary() -> PathBuf {
    root().join("target/gate/phpstan-rs")
}

/// The `gate` profile's `gate-runner` (`tools/gate-runner`): what the
/// corpus, type and baseline snapshots come from.
fn runner() -> PathBuf {
    root().join("target/gate/gate-runner")
}

/// The shipped `release` binary: what the performance gate measures.
fn release_binary() -> PathBuf {
    root().join("target/release/phpstan-rs")
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let shown = format!("{cmd:?}");
    let status = cmd
        .status()
        .map_err(|e| format!("failed to start {shown}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{shown} failed: {status}"))
    }
}

fn capture(cmd: &mut Command) -> Result<String, String> {
    let shown = format!("{cmd:?}");
    let out = cmd
        .stderr(Stdio::inherit())
        .output()
        .map_err(|e| format!("failed to start {shown}: {e}"))?;
    if !out.status.success() {
        return Err(format!("{shown} failed: {}", out.status));
    }
    String::from_utf8(out.stdout).map_err(|e| format!("{shown}: non-UTF-8 output: {e}"))
}

fn cargo() -> Command {
    let mut cmd = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()));
    cmd.current_dir(root());
    cmd
}

fn git(args: &[&str]) -> Result<String, String> {
    Ok(capture(Command::new("git").current_dir(root()).args(args))?
        .trim()
        .to_string())
}

fn head_hash() -> Result<String, String> {
    git(&["rev-parse", "HEAD"])
}

fn require_clean_tree() -> Result<(), String> {
    let status = git(&["status", "--porcelain", "--untracked-files=no"])?;
    let dirty: Vec<&str> = status
        .lines()
        .filter(|l| !l.ends_with(" vendor/drupal"))
        .collect();
    if dirty.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "snapshots are keyed by commit; commit first (dirty: {})",
            dirty.join(", ")
        ))
    }
}

fn build_gate() -> Result<(), String> {
    run(cargo().args([
        "build",
        "--profile",
        "gate",
        "-p",
        "phpstan-rs",
        "--bin",
        "phpstan-rs",
        "-p",
        "phpstan-rs-gate-runner",
        "--bin",
        "gate-runner",
    ]))
}

fn build_release() -> Result<(), String> {
    run(cargo().args(["build", "--release", "--bin", "phpstan-rs"]))
}

// --- step timing -----------------------------------------------------------

/// The wall time of each step a command ran, in order.
struct Timings {
    steps: Vec<(String, f64)>,
}

impl Timings {
    fn new() -> Self {
        Timings { steps: Vec::new() }
    }

    /// Runs `step` and records its wall time under `name`, whether it
    /// passes or fails. Steps it times itself are recorded before it.
    fn time<T>(
        &mut self,
        name: &str,
        step: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<T, String> {
        let started = Instant::now();
        let result = step(self);
        self.record(name, started.elapsed().as_secs_f64(), result.is_ok());
        result
    }

    fn record(&mut self, name: &str, secs: f64, passed: bool) {
        let name = if passed {
            name.to_string()
        } else {
            format!("{name} (failed)")
        };
        eprintln!("[gatetime] {name}: {secs:.1}s");
        self.steps.push((name, secs));
    }

    /// Writes `gatetime/<hash>.<command>.tsv`: one `step \t seconds` line
    /// per step.
    fn write(&self, hash: &str, command: &str) -> Result<(), String> {
        let mut tsv = String::new();
        for (name, secs) in &self.steps {
            writeln!(tsv, "{name}\t{secs:.1}").unwrap();
        }
        write(
            &gate_dir()
                .join("gatetime")
                .join(format!("{hash}.{command}.tsv")),
            tsv.as_bytes(),
        )
    }
}

/// `cargo test <args>`, recording the build and then each test binary
/// separately, each name prefixed with `prefix`. Cargo runs the binaries one after another and prints a
/// `Running` (or `Doc-tests`) line before each, so a binary's time is
/// the span from its line to the next one; that also covers binaries
/// that print no libtest summary, such as criterion benches in test
/// mode. Output is forwarded as it arrives.
fn test_timed(timings: &mut Timings, prefix: &str, args: &[&str]) -> Result<(), String> {
    let (reader, writer) = std::io::pipe().map_err(|e| e.to_string())?;
    let mut cmd = cargo();
    cmd.arg("test")
        .args(args)
        .stdout(writer.try_clone().map_err(|e| e.to_string())?)
        .stderr(writer);
    let shown = format!("{cmd:?}");
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("failed to start {shown}: {e}"))?;
    // The pipe reaches end of file only once every writer is closed.
    drop(cmd);
    let started = Instant::now();
    let mut current: Option<(String, Instant)> = None;
    for line in std::io::BufReader::new(reader).lines() {
        let line = line.map_err(|e| format!("{shown}: {e}"))?;
        eprintln!("{line}");
        let Some(label) = test_binary_label(&line) else {
            continue;
        };
        let now = Instant::now();
        match current.take() {
            Some((name, since)) => timings.record(&name, (now - since).as_secs_f64(), true),
            None => timings.record(
                &format!("{prefix}test build"),
                (now - started).as_secs_f64(),
                true,
            ),
        }
        current = Some((format!("{prefix}{label}"), now));
    }
    let status = child.wait().map_err(|e| format!("{shown}: {e}"))?;
    match current {
        Some((name, since)) => {
            timings.record(&name, since.elapsed().as_secs_f64(), status.success())
        }
        None => timings.record(
            &format!("{prefix}test build"),
            started.elapsed().as_secs_f64(),
            status.success(),
        ),
    }
    if status.success() {
        Ok(())
    } else {
        Err(format!("{shown} failed: {status}"))
    }
}

/// `test <source> (<binary>)` for cargo's `Running <kind> <source>
/// (<path>)` line, `doc-tests <crate>` for its `Doc-tests <crate>` line.
fn test_binary_label(line: &str) -> Option<String> {
    let line = line.trim_start();
    if let Some(krate) = line.strip_prefix("Doc-tests ") {
        return Some(format!("doc-tests {}", krate.trim()));
    }
    let rest = line.strip_prefix("Running ")?;
    let (source, path) = rest.rsplit_once(" (")?;
    let source = source.strip_prefix("unittests ").unwrap_or(source);
    let stem = Path::new(path.trim_end_matches(')'))
        .file_name()?
        .to_str()?;
    let binary = stem.rsplit_once('-').map_or(stem, |(name, _)| name);
    Some(format!("test {source} ({binary})"))
}

// --- corpus configurations -------------------------------------------------

#[derive(Clone, Copy)]
struct Config {
    name: &'static str,
    args: fn() -> Vec<String>,
}

fn scoped_args() -> Vec<String> {
    vec!["--config".into(), "phpstan-rs.toml".into()]
}

fn unscoped_args() -> Vec<String> {
    vec!["--no-composer".into(), ".".into()]
}

/// Every opt-in check on, over the scoped configuration: the rules the
/// default configurations never exercise.
fn all_on_args() -> Vec<String> {
    let mut args = scoped_args();
    for set in ALL_ON_SETS {
        args.push("--set".into());
        args.push((*set).into());
    }
    args
}

/// The config keys `all_on_args` sets over the scoped configuration.
const ALL_ON_SETS: &[&str] = &[
    "rules.strict_mixed_arrays=true",
    "rules.check_uninitialized_properties=true",
    "rules.check_internal_usage=true",
    "rules.check_function_name_case=true",
    "ext.deprecation-rules.enabled=true",
    "ext.extras.check_empty_method_bodies=true",
    "ext.extras.check_missing_checked_exceptions=true",
    "rules.check_strict_printf_placeholder_types=true",
    "rules.check_too_wide_throw_type_in_protected_and_public_methods=true",
    "rules.check_too_wide_return_type_in_protected_and_public_methods=true",
    "rules.check_too_wide_by_ref_type_in_protected_and_public_methods=true",
    "rules.check_method_throw_type_covariance=true",
    "rules.check_benevolent_union_types=true",
    "rules.check_missing_callable_signature=true",
    "rules.class_shape.check_closed_classes=true",
    "rules.class_shape.default_parent_calls=true",
    "ext.mockstan.check_mock_hygiene=true",
    "rules.check_duplicate_function_declarations=true",
    "rules.check_duplicate_class_declarations=true",
    "ext.extras.check_boolean_trap=true",
    "ext.extras.check_array_key_cast=true",
    "ext.strict-rules.check_parent_constructor_call=true",
    "ext.extras.check_promoted_property_suggestions=true",
    "ext.strict-rules.check_boolean_conditions=true",
    "rules.check_dynamic_properties=true",
    "ext.extras.check_swapped_arguments=true",
    "ext.extras.check_never_return_suggestions=true",
    "ext.extras.check_switch_missing_default=true",
    "ext.extras.check_throw_without_message=true",
    "ext.extras.check_throw_base_class=true",
    "ext.extras.check_exception_hygiene=true",
    "ext.slam.check_current_time_dependence=true",
    "ext.psr3.check_psr3_exception_context=true",
    "ext.psr3.check_psr3_logging=true",
    "ext.sidz.check_magic_numbers=true",
    "ext.extras.check_native_type_hints=true",
    "ext.strict-rules.check_implicit_array_creation=true",
    "ext.strict-rules.check_useless_casts=true",
    "ext.strict-rules.check_strict_array_filter=true",
    "ext.voku.check_mixed_type_operands=true",
    "ext.shipmonk.check_comparable_types=true",
    "ext.strict-rules.check_arithmetic_operands=true",
    "ext.extras.check_identity_vs_equals=true",
    "ext.extras.check_loose_haystack_comparison=true",
    "ext.strict-rules.check_strict_function_calls=true",
    "ext.strict-rules.check_switch_case_types=true",
    "ext.strict-rules.check_loop_variable_overwrite=true",
    "ext.strict-rules.check_dynamic_static_calls=true",
    "ext.strict-rules.check_illegal_constructor_calls=true",
    "ext.shipmonk.check_enum_exhaustiveness=true",
    "ext.shipmonk.check_null_coercion=true",
    "ext.extras.check_new_exception_discarded=true",
    "ext.phpunit.check_risky_tests=true",
    "rules.check_dead_enum_cases=true",
    "rules.check_dead_code=true",
    "rules.check_circular_dependencies=true",
    "rules.check_dead_abstract_classes=true",
    "rules.check_dead_members=true",
    "rules.check_renamed_named_arguments=true",
    "report_unmatched_ignored_errors=true",
    "rules.check_composer_hygiene=true",
    "ext.doctrine.enabled=true",
    "ext.pest.enabled=true",
    "ext.language-extensions.enabled=true",
    "ext.disallowed-calls.restricted_symbol_presets=[\"multibyte-strings\"]",
    "rules.banned.constructs=[\"eval\", \"goto\", \"global\", \"backticks\", \"variable-variables\"]",
];

/// The architecture rules of the `arch` configuration: one or more of
/// each assertion and each selector, over Piwigo's namespaces, so each
/// architecture pass has a corpus to be checked on.
const ARCH_RULES: &[&str] = &[
    r#"{name = "core below controllers", classes = [{namespace = 'Piwigo\Core'}], should_not_depend_on = [{namespace = 'Piwigo\Controller'}, {namespace = 'Piwigo\Admin'}], should_not_construct = [{namespace = 'Piwigo\Db'}]}"#,
    r#"{name = "value objects", classes = [{namespace = 'Piwigo\Common\ValueObject'}], excluding = [{is_interface = true}], can_only_depend_on = [{namespace = 'Piwigo\Common'}, {namespace = 'Piwigo\Core'}], should_be_final = true, should_be_readonly = true}"#,
    r#"{name = "projections", classes = [{classname = '/\\Projection\\/'}], should_be_final = true, should_have_only_one_public_method = true, should_not_use_trait = [{is_trait = true}]}"#,
    r#"{name = "controllers", classes = [{namespace = 'Piwigo\Controller', is_standard_class = true}], excluding = [{is_abstract = true}], should_be_named = '/Controller$/', should_depend_on = [{namespace = 'Piwigo\Core'}], should_apply_attribute = 'Piwigo\Controller\Route'}"#,
    r#"{name = "exceptions", classes = [{is_exception = true}], should_extend = [{classname = 'RuntimeException'}], should_not_extend = [{classname = 'Exception'}], should_be_named = '/Exception$/'}"#,
    r#"{name = "interfaces", classes = [{is_interface = true}], should_be_named = '/Interface$/', should_not_implement = [{classname = 'Countable'}]}"#,
    r#"{name = "abstract classes", classes = [{is_abstract = true, is_interface = false}], should_be_abstract = true, should_implement = [{classname = 'Stringable'}], should_use_trait = [{is_trait = true}]}"#,
    r#"{name = "attributes", classes = [{is_attribute = true}, {applies_attribute = 'Attribute'}], should_be_final = true}"#,
    r#"{name = "enums and traits", classes = [{is_enum = true}, {is_trait = true}, {is_final = true}, {is_readonly = true}], should_not_depend_on = [{extends = 'Exception'}, {implements = 'Traversable'}, {uses_trait = '/./'}]}"#,
];

/// The types the `arch` configuration bans: every keyword type banning
/// understands, and two classes.
const ARCH_BANNED_TYPES: &str = r#"rules.banned.types=["float", "int", "string", "bool", "array", "mixed", "object", "callable", "iterable", "DateTime", "stdClass"]"#;

/// The constructs the `arch` configuration bans: every one banning knows.
const ARCH_BANNED_CONSTRUCTS: &str = r#"rules.banned.constructs=["eval", "isset", "empty", "compact", "extract", "exit", "error-suppression", "switch", "goto", "global", "named-arguments", "assign-by-reference", "by-reference-parameters", "by-reference-returns", "short-ternary", "loose-comparison", "array-append", "backticks", "ignore-comments", "superglobals", "variable-variables", "dynamic-member-access"]"#;

/// The scoped configuration with architecture rules and banned types and
/// constructs, which no other configuration sets.
fn arch_args() -> Vec<String> {
    let mut args = scoped_args();
    args.push("--set".into());
    args.push(format!("rules.architecture=[{}]", ARCH_RULES.join(", ")));
    for set in [ARCH_BANNED_TYPES, ARCH_BANNED_CONSTRUCTS] {
        args.push("--set".into());
        args.push(set.into());
    }
    args
}

/// The corpus's vendor tree as one project, its own context.
fn vendor_tree_args() -> Vec<String> {
    vec!["--no-composer".into(), "vendor".into()]
}

const SCOPED: Config = Config {
    name: "scoped",
    args: scoped_args,
};
const ARCH: Config = Config {
    name: "arch",
    args: arch_args,
};
const VENDOR_TREE: Config = Config {
    name: "vendor-tree",
    args: vendor_tree_args,
};
const UNSCOPED: Config = Config {
    name: "unscoped",
    args: unscoped_args,
};
const ALL_ON: Config = Config {
    name: "all-on",
    args: all_on_args,
};

/// Artifact names recorded by `corpus`: the quick tier every commit, the
/// full tier at phase boundaries and for commits touching levels.
fn artifacts(full: bool) -> Vec<(Config, &'static str)> {
    let mut out = vec![(SCOPED, "unset"), (ALL_ON, "unset")];
    if full {
        out.push((UNSCOPED, "unset"));
        out.push((VENDOR_TREE, "unset"));
        out.push((ARCH, "unset"));
        for config in [SCOPED, ALL_ON] {
            for level in &LEVELS[1..] {
                out.push((config, level));
            }
        }
    }
    out
}

fn artifact_name(config: &Config, level: &str) -> String {
    if level == "unset" {
        config.name.to_string()
    } else {
        format!("{}-L{level}", config.name)
    }
}

/// One line per diagnostic, in output order: `path \t line \t col \t
/// level \t found from \t found below \t identifier \t message \t tip`.
fn normalize(json: &str) -> Result<String, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("corpus output is not JSON: {e}"))?;
    let mut out = String::new();
    let files_checked = &value["totals"]["files_checked"];
    writeln!(out, "# files_checked {files_checked}").unwrap();
    let files = value["files"]
        .as_object()
        .ok_or("corpus output has no `files` object")?;
    for (path, file) in files {
        for m in file["messages"].as_array().into_iter().flatten() {
            let field = |k: &str| {
                m[k].as_str()
                    .unwrap_or("")
                    .replace('\\', "\\\\")
                    .replace('\t', "\\t")
                    .replace('\n', "\\n")
            };
            writeln!(
                out,
                "{path}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                m["line"],
                m["column"],
                m["level"],
                m.get("foundFrom").unwrap_or(&serde_json::json!(0)),
                m.get("foundBelow")
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                field("identifier"),
                field("message"),
                field("tip"),
            )
            .unwrap();
        }
    }
    Ok(out)
}

fn write(path: &Path, contents: &[u8]) -> Result<(), String> {
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(path, contents).map_err(|e| format!("writing {}: {e}", path.display()))
}

fn corpus_command(args: &[String]) -> Result<(), String> {
    let full = args.iter().any(|a| a == "--full");
    let profile = match args.iter().position(|a| a == "--profile") {
        Some(i) => match args.get(i + 1).map(String::as_str) {
            Some("gate") => "gate",
            Some("oracle") => "oracle",
            other => return Err(format!("--profile takes gate or oracle, not {other:?}")),
        },
        None => "gate",
    };
    // Only the shipped profile's artifacts are recorded, from a committed
    // tree.
    let hash = if profile == "gate" {
        require_clean_tree()?;
        Some(head_hash()?)
    } else {
        None
    };
    let mut timings = Timings::new();
    let result = pass::run_pass(
        &pass::PassSetup {
            full,
            fixtures: true,
            profile,
            hash: hash.clone(),
        },
        &mut timings,
    );
    if let Some(hash) = &hash {
        timings.write(hash, "corpus")?;
    }
    result
}

/// `cargo xtask type-drift`: relates HEAD's recorded type snapshot of
/// `--artifact` (`types-scoped` by default) to `--against`'s through the
/// runner, within the corpus project's class hierarchy (each fixture's own
/// for `types-fixtures`), and writes the report as the
/// `type-drift-<artifact>` snapshot. Both snapshots must be recorded.
fn type_drift(args: &[String]) -> Result<(), String> {
    require_clean_tree()?;
    let hash = head_hash()?;
    let mut against = None;
    let mut artifact = "types-scoped".to_string();
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = || rest.next().cloned().ok_or(format!("{arg} needs a value"));
        match arg.as_str() {
            "--against" => against = Some(git(&["rev-parse", &value()?])?),
            "--artifact" => artifact = value()?,
            other => return Err(format!("unknown argument {other}")),
        }
    }
    let against = against.ok_or("--against <hash> is required")?;
    build_gate()?;
    let out = runner_out(&format!("type-drift-{artifact}"));
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let (old, new) = (out.join("old.txt"), out.join("new.txt"));
    let mut store = Store::open()?;
    for (commit, target) in [(&against, &old), (&hash, &new)] {
        let mut file = std::io::BufWriter::new(
            std::fs::File::create(target).map_err(|e| format!("{}: {e}", target.display()))?,
        );
        let rev = format!("{}:{artifact}.txt", snapshot_ref(commit));
        if !store.write_text(&rev, &mut file)? {
            return Err(format!(
                "no {artifact} snapshot for {commit}: record it with `cargo xtask corpus` there"
            ));
        }
        file.flush().map_err(|e| e.to_string())?;
    }
    let mut cmd = Command::new(runner());
    cmd.arg("--type-drift")
        .arg(&old)
        .arg(&new)
        .arg("--out")
        .arg(&out);
    match artifact.as_str() {
        "types-scoped" => cmd.current_dir(corpus_dir()).args((SCOPED.args)()),
        "types-all-on" => cmd.current_dir(corpus_dir()).args((ALL_ON.args)()),
        "types-fixtures" => cmd
            .current_dir(root())
            .arg("--fixture-root")
            .arg(root().join("crates/phpstan-rs/tests/fixtures")),
        other => return Err(format!("no type snapshot named {other}")),
    };
    run(&mut cmd)?;
    let report = out.join("type-drift.txt");
    let text = std::fs::read(&report).map_err(|e| e.to_string())?;
    let entry = format!("type-drift-{artifact}.txt");
    store.add(
        &snapshot_ref(&hash),
        &format!("record {entry}"),
        vec![(entry.clone(), text_content(&entry, text))],
    )?;
    println!("{}", report.display());
    Ok(())
}

/// Where the runner writes `artifact`'s views.
fn runner_out(artifact: &str) -> PathBuf {
    gate_dir().join(format!(".run-{artifact}"))
}

/// Runs the fixture suite with each fixture's type and diagnostic records
/// on, and records them in HEAD's snapshot (`fixture_entries`).
fn record_fixture_types(hash: &str, timings: &mut Timings) -> Result<(), String> {
    let dir = gate_dir().join(".fixture-types.tmp");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    timings.time("fixtures", |_| {
        run(cargo()
            .args(["test", "--profile", "gate", "--test", "fixtures", "--quiet"])
            .env("PHPSTAN_RS_DUMP_TYPES", &dir)
            .env("PHPSTAN_RS_DUMP_DIAGNOSTICS", &dir))
    })?;
    Store::open()?.add(
        &snapshot_ref(hash),
        "record the fixtures",
        fixture_entries(&dir)?,
    )?;
    std::fs::remove_dir_all(&dir).map_err(|e| e.to_string())
}

/// The fixture suite's records in `dir`, in fixture order, as the
/// snapshot's `diagnostics-fixtures.txt` and `types-fixtures.txt`.
fn fixture_entries(dir: &Path) -> Result<Vec<(String, Content)>, String> {
    let joined = |extension: &str| -> Result<String, String> {
        let mut dumps: Vec<PathBuf> = std::fs::read_dir(dir)
            .map_err(|e| e.to_string())?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|e| e == extension))
            .collect();
        dumps.sort();
        let mut text = String::new();
        for dump in &dumps {
            text.push_str(&std::fs::read_to_string(dump).map_err(|e| e.to_string())?);
        }
        Ok(text)
    };
    let mut entries = Vec::new();
    for (extension, entry) in [
        ("diagnostics", "diagnostics-fixtures.txt"),
        ("types", "types-fixtures.txt"),
    ] {
        entries.push((
            entry.to_string(),
            text_content(entry, joined(extension)?.into_bytes()),
        ));
    }
    Ok(entries)
}

/// The corpus's vendor projects: each `vendor/<vendor>/<package>` with a
/// `composer.json`, in path order.
fn vendor_projects() -> Result<Vec<String>, String> {
    let vendor = corpus_dir().join("vendor");
    let mut projects = Vec::new();
    for org in std::fs::read_dir(&vendor).map_err(|e| format!("{}: {e}", vendor.display()))? {
        let org = org.map_err(|e| e.to_string())?.path();
        if !org.is_dir() {
            continue;
        }
        for package in std::fs::read_dir(&org).map_err(|e| e.to_string())? {
            let package = package.map_err(|e| e.to_string())?.path();
            if package.join("composer.json").is_file()
                && let Ok(relative) = package.strip_prefix(corpus_dir())
            {
                projects.push(relative.to_string_lossy().into_owned());
            }
        }
    }
    projects.sort();
    Ok(projects)
}

// --- item 0's dumps ----------------------------------------------------------

/// The vendor projects the server's sessions run over, beside the corpus.
const SESSION_PROJECTS: [&str; 2] = ["vendor/symfony/console", "vendor/doctrine/orm"];

// --- baseline compatibility ----------------------------------------------

fn baseline_file(config: &str) -> PathBuf {
    gate_dir().join("baseline").join(format!("{config}.neon"))
}

/// Records the reference baselines once, from the Stage A binary; every
/// later commit must keep them producing no diagnostics and no stale
/// entries.
fn baseline_generate() -> Result<(), String> {
    build_gate()?;
    for config in [SCOPED, ALL_ON] {
        let file = baseline_file(config.name);
        std::fs::create_dir_all(file.parent().unwrap()).map_err(|e| e.to_string())?;
        let status = Command::new(binary())
            .current_dir(corpus_dir())
            .arg("check-project")
            .args((config.args)())
            .arg("--generate-baseline")
            .arg(&file)
            .stdout(Stdio::null())
            .status()
            .map_err(|e| e.to_string())?;
        if !file.exists() {
            return Err(format!("{}: no baseline written ({status})", config.name));
        }
        eprintln!("generated {}", file.display());
    }
    Ok(())
}

// --- comparison ------------------------------------------------------------

/// The nearest proper ancestor of HEAD whose snapshot has every entry
/// `compare` checks (an entry no older snapshot has counts as present: it
/// starts at HEAD). Usually HEAD's parent; an ancestor whose gate run
/// failed before recording is skipped. A skipped commit that adds an
/// approved delta can't be skipped: `compare` only reads HEAD's own deltas.
fn comparison_base(full: bool, snapshots: &Snapshots) -> Result<String, String> {
    let hash = head_hash()?;
    let entries = compared_artifacts(full);
    let ancestors = git(&["rev-list", "HEAD~1"])?;
    let mut skipped: Vec<String> = Vec::new();
    for ancestor in ancestors.lines() {
        let complete = entries.iter().all(|entry| {
            has_entry(snapshots, ancestor, entry) || !recorded_before(snapshots, entry, &hash)
        });
        if complete {
            if let Some(commit) = skipped
                .iter()
                .find(|commit| adds_approved_delta(commit).unwrap_or(true))
            {
                return Err(format!(
                    "{commit} has no snapshot but adds an approved delta; record its snapshot, or compare with --against"
                ));
            }
            if !skipped.is_empty() {
                eprintln!(
                    "comparing against {ancestor}: {} newer commit(s) have no snapshot",
                    skipped.len()
                );
            }
            return Ok(ancestor.to_string());
        }
        skipped.push(ancestor.to_string());
    }
    Err("no ancestor has a complete snapshot".to_string())
}

fn has_entry(snapshots: &Snapshots, hash: &str, entry: &str) -> bool {
    snapshots
        .get(hash)
        .is_some_and(|entries| entries.contains(entry))
}

/// Whether a snapshot besides `hash`'s has `entry`: one none does is first
/// recorded at `hash`, with nothing older to compare against.
fn recorded_before(snapshots: &Snapshots, entry: &str, hash: &str) -> bool {
    snapshots
        .iter()
        .any(|(other, entries)| other != hash && entries.contains(entry))
}

/// Whether `commit` adds or changes a file under `gates/approved/`.
fn adds_approved_delta(commit: &str) -> Result<bool, String> {
    let changed = git(&["show", "--name-only", "--format=", commit])?;
    Ok(changed.lines().any(|p| p.starts_with("gates/approved/")))
}

/// The `gates/approved/*/<entry>.diff` HEAD's own commit adds or changes.
fn approved_delta(entry: &str) -> Result<Option<Vec<u8>>, String> {
    let changed = git(&["show", "--name-only", "--format=", "HEAD"])?;
    for path in changed.lines() {
        if path.starts_with("gates/approved/")
            && Path::new(path).file_name().and_then(|f| f.to_str())
                == Some(&format!("{entry}.diff"))
        {
            let text = std::fs::read(root().join(path)).map_err(|e| e.to_string())?;
            return Ok(Some(text));
        }
    }
    Ok(None)
}

fn unified_diff(old: &[u8], new: &[u8]) -> Result<String, String> {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let dir = gate_dir().join(".diff");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let at = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let name = |side: &str| dir.join(format!("{}-{at}.{side}", std::process::id()));
    let (a, b) = (name("old"), name("new"));
    std::fs::write(&a, old).map_err(|e| e.to_string())?;
    std::fs::write(&b, new).map_err(|e| e.to_string())?;
    let out = Command::new("diff")
        .args(["-u", "--label", "old", "--label", "new"])
        .arg(&a)
        .arg(&b)
        .output()
        .map_err(|e| e.to_string());
    let _ = std::fs::remove_file(&a);
    let _ = std::fs::remove_file(&b);
    Ok(String::from_utf8_lossy(&out?.stdout).into_owned())
}

/// Every entry of a snapshot `compare` checks.
fn compared_artifacts(full: bool) -> Vec<String> {
    let mut names = vec![
        "types-fixtures.txt".to_string(),
        "diagnostics-fixtures.txt".to_string(),
    ];
    for (config, level) in artifacts(full) {
        let artifact = artifact_name(&config, level);
        if level == "unset" && matches!(config.name, "scoped" | "all-on") {
            names.push(format!("types-{artifact}.txt"));
            names.push(format!("baseline-{artifact}.txt"));
        }
        names.push(format!("diagnostics-{artifact}.txt"));
        names.push(format!("{artifact}.txt"));
    }
    if full {
        for name in [
            "vendor-projects.txt",
            "diagnostics-vendor-projects.txt",
            "equalities.txt",
        ] {
            names.push(name.to_string());
        }
        for config in [SCOPED, ALL_ON, UNSCOPED, VENDOR_TREE, ARCH] {
            for dump in ["registry", "expr-types", "doc-resolutions"] {
                names.push(format!("{dump}-{}.txt", config.name));
            }
            names.push(format!("front-end-{}", config.name));
        }
        names.push("stub-bases.txt".to_string());
        names.push("session-piwigo.txt".to_string());
        for project in SESSION_PROJECTS {
            names.push(format!("session-{}.txt", project.replace('/', "-")));
        }
        names.push("replay-piwigo".to_string());
        for project in vendor_projects().unwrap_or_default() {
            names.push(format!("replay-{}", project.replace('/', "-")));
        }
    }
    names
}

/// The first lines of a diff.
fn shown(diff: &[u8]) -> String {
    String::from_utf8_lossy(diff)
        .lines()
        .take(40)
        .collect::<Vec<_>>()
        .join("\n")
}

fn compare(args: &[String]) -> Result<(), String> {
    let full = args.iter().any(|a| a == "--full");
    let store = Store::open()?;
    let snapshots = store.snapshots()?;
    let against = match args.iter().position(|a| a == "--against") {
        Some(i) => git(&[
            "rev-parse",
            args.get(i + 1).ok_or("--against needs a hash")?,
        ])?,
        None => comparison_base(full, &snapshots)?,
    };
    let hash = head_hash()?;
    let mut failures = Vec::new();
    for entry in &compared_artifacts(full) {
        match (
            has_entry(&snapshots, &against, entry),
            has_entry(&snapshots, &hash, entry),
        ) {
            (true, true) => {}
            // An entry the older snapshot predates starts here.
            (false, true) if !recorded_before(&snapshots, entry, &hash) => {
                eprintln!("{entry}: first recorded at {hash}");
                continue;
            }
            _ => {
                failures.push(format!(
                    "{entry}: not in the snapshot of {against} or {hash}"
                ));
                continue;
            }
        }
        let diff = store.diff(&against, &hash, entry)?;
        if diff.is_empty() {
            continue;
        }
        match approved_delta(entry)? {
            Some(approved) if approved == diff => {
                eprintln!("{entry}: approved delta matches");
            }
            Some(_) => failures.push(format!("{entry}: differs from its approved delta")),
            None => failures.push(format!("{entry}: unapproved change\n{}", shown(&diff))),
        }
    }
    if failures.is_empty() {
        eprintln!("the snapshot of {hash} matches {against}'s");
        Ok(())
    } else {
        Err(failures.join("\n\n"))
    }
}

/// Writes the store's diff of every entry that differs between HEAD's
/// snapshot and its parent's to `gates/approved/<slug>/<entry>.diff`, for
/// amending into the commit that makes the change.
fn approve(args: &[String]) -> Result<(), String> {
    let mut against = None;
    let mut slug = None;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--against" => against = Some(rest.next().ok_or("--against needs a commit")?),
            "--full" => {}
            _ => slug = Some(arg),
        }
    }
    let slug = slug.ok_or("approve needs a slug")?;
    let full = args.iter().any(|a| a == "--full");
    let store = Store::open()?;
    let snapshots = store.snapshots()?;
    let parent = match against {
        Some(commit) => git(&["rev-parse", commit])?,
        None => comparison_base(full, &snapshots)?,
    };
    let hash = head_hash()?;
    let dir = root().join("gates/approved").join(slug);
    let mut written = 0usize;
    for entry in compared_artifacts(full) {
        if !has_entry(&snapshots, &parent, &entry) || !has_entry(&snapshots, &hash, &entry) {
            return Err(format!(
                "{entry}: not in the snapshot of {parent} or {hash}"
            ));
        }
        let diff = store.diff(&parent, &hash, &entry)?;
        if !diff.is_empty() {
            write(&dir.join(format!("{entry}.diff")), &diff)?;
            written += 1;
        }
    }
    eprintln!("{written} approved delta(s) written to {}", dir.display());
    Ok(())
}

/// After `git commit --amend` added only `gates/` files, HEAD's snapshot
/// and the files kept by commit are still keyed by the pre-amend hash: move
/// them.
fn rekey(args: &[String]) -> Result<(), String> {
    let old = args.first().ok_or("rekey needs the pre-amend hash")?;
    let old = &git(&["rev-parse", old])?;
    let hash = head_hash()?;
    let changed = git(&["diff", "--name-only", old, "HEAD"])?;
    if let Some(path) = changed.lines().find(|p| !p.starts_with("gates/")) {
        return Err(format!("{old}..HEAD changes {path}, not only gates/"));
    }
    let moved_snapshot = Store::open()?.rename_snapshot(old, &hash)?;
    let mut moved = 0usize;
    for entry in std::fs::read_dir(gate_dir())
        .map_err(|e| e.to_string())?
        .flatten()
    {
        let dir = entry.path();
        if dir.file_name().is_some_and(|name| name == "store.git") {
            continue;
        }
        let Ok(files) = std::fs::read_dir(&dir) else {
            continue;
        };
        for file in files.flatten() {
            let name = file.file_name().to_string_lossy().to_string();
            if let Some(rest) = name.strip_prefix(&format!("{old}.")) {
                std::fs::rename(file.path(), dir.join(format!("{hash}.{rest}")))
                    .map_err(|e| e.to_string())?;
                moved += 1;
            }
        }
    }
    eprintln!(
        "moved {} and {moved} file(s) from {old} to {hash}",
        if moved_snapshot {
            "the snapshot"
        } else {
            "no snapshot"
        }
    );
    Ok(())
}

// --- performance -----------------------------------------------------------

fn perf() -> Result<(), String> {
    require_clean_tree()?;
    let hash = head_hash()?;
    build_release()?;
    let dir = gate_dir().join("perf");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let json = dir.join(format!("{hash}.hyperfine.json"));
    let command = format!(
        "{} check-project --config phpstan-rs.toml --format json",
        release_binary().display()
    );
    run(Command::new("hyperfine")
        .current_dir(corpus_dir())
        .args([
            "--warmup", "1", "--runs", "10", "--style", "basic", "--output", "null",
        ])
        .arg("--export-json")
        .arg(&json)
        .arg(&command))?;
    let rss = Command::new("/usr/bin/time")
        .current_dir(corpus_dir())
        .args(["-f", "%M"])
        .arg(release_binary())
        .args([
            "check-project",
            "--config",
            "phpstan-rs.toml",
            "--format",
            "json",
        ])
        .stdout(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    let rss_kb = String::from_utf8_lossy(&rss.stderr)
        .lines()
        .last()
        .unwrap_or("")
        .trim()
        .to_string();
    write(&dir.join(format!("{hash}.rss")), rss_kb.as_bytes())?;
    let (mean, stddev) = hyperfine_stats(&json)?;
    eprintln!("{hash}: mean {mean:.2}s ± {stddev:.2}s, peak RSS {rss_kb} KB");
    // Timing needs a quiet machine, so not every commit has a
    // measurement: compare against the nearest ancestor that does.
    let Some(parent) = nearest_ancestor_with(&dir, "hyperfine.json")? else {
        eprintln!("no earlier performance measurement to compare against");
        return Ok(());
    };
    let parent_json = dir.join(format!("{parent}.hyperfine.json"));
    {
        let (old_mean, old_stddev) = hyperfine_stats(&parent_json)?;
        let limit = old_mean + (old_mean * 0.02).max(3.0 * old_stddev);
        if mean > limit {
            return Err(format!(
                "wall time regressed: {mean:.2}s > {limit:.2}s ({parent}: {old_mean:.2}s ± {old_stddev:.2}s)"
            ));
        }
        if let Ok(old_rss) = std::fs::read_to_string(dir.join(format!("{parent}.rss"))) {
            let (old, new) = (
                old_rss.trim().parse::<f64>().unwrap_or(f64::MAX),
                rss_kb.parse::<f64>().unwrap_or(0.0),
            );
            if new > old * 1.02 {
                return Err(format!("peak RSS regressed: {new} KB > {old} KB + 2%"));
            }
        }
    }
    Ok(())
}

/// The nearest proper ancestor of HEAD with `<hash>.<ext>` in `dir`.
fn nearest_ancestor_with(dir: &Path, ext: &str) -> Result<Option<String>, String> {
    let ancestors = git(&["rev-list", "HEAD~1"])?;
    Ok(ancestors
        .lines()
        .map(str::to_string)
        .find(|hash| dir.join(format!("{hash}.{ext}")).exists()))
}

fn hyperfine_stats(path: &Path) -> Result<(f64, f64), String> {
    let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let value: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let r = &value["results"][0];
    Ok((
        r["mean"].as_f64().ok_or("hyperfine: no mean")?,
        r["stddev"].as_f64().unwrap_or(0.0),
    ))
}

// --- build time ------------------------------------------------------------

/// Files whose edit represents each rebuild scenario.
/// Each build-time scenario: the file it touches, and the workspace
/// library crates `cargo build -p phpstan-rs` must (`Some(true)`) or must
/// not (`Some(false)`) rebuild after it; unlisted crates are unconstrained.
/// An edit inside the package rebuilds no extracted crate; a lexer edit
/// rebuilds `syntax` and so everything above it; a stub edit rebuilds
/// `stub_sources` and only its dependents.
/// A library crate and whether a scenario must rebuild it.
type RebuildTarget = (&'static str, bool);

const BUILD_SCENARIOS: &[(&str, &str, &[RebuildTarget])] = &[
    (
        "engine",
        "crates/phpstan-rs/src/checker/expr.rs",
        &[
            ("syntax", false),
            ("types", false),
            ("phpdoc", false),
            ("sql", false),
            ("diagnostics", false),
            ("composer", false),
            ("stub_sources", false),
            ("config", false),
        ],
    ),
    (
        "rule",
        "crates/phpstan-rs/src/checker/magic_numbers.rs",
        &[
            ("syntax", false),
            ("types", false),
            ("phpdoc", false),
            ("sql", false),
            ("diagnostics", false),
            ("composer", false),
            ("stub_sources", false),
            ("config", false),
        ],
    ),
    (
        "lexer",
        "crates/syntax/src/lexer.rs",
        &[
            ("syntax", true),
            ("sql", false),
            ("stub_sources", false),
            ("config", false),
        ],
    ),
    (
        "stub",
        "vendor/phpstorm-stubs/standard/standard_0.php",
        &[
            ("syntax", false),
            ("types", false),
            ("phpdoc", false),
            ("sql", false),
            ("diagnostics", false),
            ("composer", false),
            ("stub_sources", true),
            ("config", false),
        ],
    ),
];

fn buildtime() -> Result<(), String> {
    require_clean_tree()?;
    let hash = head_hash()?;
    run(cargo().args(["test", "--workspace", "--no-run", "--quiet"]))?;
    run(cargo().args(["build", "-p", "phpstan-rs", "--quiet"]))?;
    let mut report = String::new();
    let mut violations = Vec::new();
    for (scenario, file, expected) in BUILD_SCENARIOS {
        run(Command::new("touch").arg(root().join(file)))?;
        let started = Instant::now();
        let rebuilt = rebuilt_workspace_crates(cargo().args([
            "build",
            "-p",
            "phpstan-rs",
            "--message-format=json",
        ]))?;
        let secs = started.elapsed().as_secs_f64();
        writeln!(
            report,
            "{scenario}\tbuild\t{secs:.1}\t{}",
            rebuilt.iter().cloned().collect::<Vec<_>>().join(",")
        )
        .unwrap();
        eprintln!("{scenario} build: {secs:.1}s, rebuilt {rebuilt:?}");
        for (krate, must) in *expected {
            if rebuilt.contains(*krate) != *must {
                violations.push(format!(
                    "{scenario}: {krate} {}",
                    if *must {
                        "was not rebuilt"
                    } else {
                        "was rebuilt"
                    }
                ));
            }
        }
        run(Command::new("touch").arg(root().join(file)))?;
        let started = Instant::now();
        run(cargo().args(["test", "--workspace", "--no-run", "--quiet"]))?;
        let secs = started.elapsed().as_secs_f64();
        writeln!(report, "{scenario}\ttest-no-run\t{secs:.1}").unwrap();
        eprintln!("{scenario} test-no-run: {secs:.1}s");
        // Test builds unify dev-dependency features into their own
        // library artifacts; the plain build's go stale with the second
        // touch and would count against the next scenario.
        run(cargo().args(["build", "-p", "phpstan-rs", "--quiet"]))?;
    }
    write(
        &gate_dir().join("buildtime").join(format!("{hash}.tsv")),
        report.as_bytes(),
    )?;
    if violations.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "build-time targets missed:\n{}",
            violations.join("\n")
        ))
    }
}

/// The workspace library crates (by lib name) a cargo invocation with
/// `--message-format=json` compiled rather than found fresh.
fn rebuilt_workspace_crates(cmd: &mut Command) -> Result<BTreeSet<String>, String> {
    let out = capture(cmd)?;
    let workspace = root().join("crates");
    let mut rebuilt = BTreeSet::new();
    for line in out.lines() {
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if msg["reason"] != "compiler-artifact" || msg["fresh"] != false {
            continue;
        }
        let in_workspace = msg["manifest_path"]
            .as_str()
            .is_some_and(|m| Path::new(m).starts_with(&workspace));
        let is_lib = msg["target"]["kind"]
            .as_array()
            .is_some_and(|kinds| kinds.iter().any(|k| k == "lib"));
        if in_workspace
            && is_lib
            && let Some(name) = msg["target"]["name"].as_str()
        {
            rebuilt.insert(name.to_string());
        }
    }
    Ok(rebuilt)
}

// --- test-name manifest ----------------------------------------------------

fn list_tests() -> Result<BTreeSet<String>, String> {
    let out = capture(cargo().args([
        "test",
        "--workspace",
        "--lib",
        "--bins",
        "--tests",
        "--quiet",
        "--",
        "--list",
        "--format",
        "terse",
    ]))?;
    // The benches run in the gate profile, and the fixture target is
    // `test = false`; list both from there.
    let benches = capture(cargo().args([
        "test",
        "--profile",
        "gate",
        "--workspace",
        "--bench",
        "*",
        "--quiet",
        "--",
        "--list",
    ]))?;
    let fixtures = capture(cargo().args([
        "test",
        "--profile",
        "gate",
        "--test",
        "fixtures",
        "--quiet",
        "--",
        "--list",
    ]))?;
    Ok(out
        .lines()
        .chain(benches.lines())
        .chain(fixtures.lines())
        .filter_map(|l| {
            l.strip_suffix(": test")
                .or_else(|| l.strip_suffix(": benchmark"))
        })
        .map(str::to_string)
        .collect())
}

/// Every test of the nearest ancestor with a recorded manifest must still
/// exist, or be listed in a `gates/test-renames/*.txt` file a commit since
/// then added or changed, as `old -> new` or `old -> REMOVED: reason`.
fn test_manifest() -> Result<(), String> {
    require_clean_tree()?;
    let hash = head_hash()?;
    let tests = list_tests()?;
    let listing: String = tests.iter().map(|t| format!("{t}\n")).collect();
    let dir = gate_dir().join("tests");
    write(&dir.join(format!("{hash}.txt")), listing.as_bytes())?;
    eprintln!("{hash}: {} tests", tests.len());
    let ancestors = git(&["rev-list", "HEAD~1"])?;
    let Some((base, old)) = ancestors.lines().find_map(|hash| {
        std::fs::read_to_string(dir.join(format!("{hash}.txt")))
            .ok()
            .map(|text| (hash.to_string(), text))
    }) else {
        eprintln!("no manifest recorded for any ancestor; nothing to compare");
        return Ok(());
    };
    eprintln!("comparing against {base}");
    let mut renames = std::collections::BTreeMap::new();
    let changed = git(&[
        "log",
        "--name-only",
        "--format=",
        &format!("{base}..HEAD"),
        "--",
        "gates/test-renames/",
    ])?;
    let files: BTreeSet<&str> = changed.lines().filter(|p| !p.is_empty()).collect();
    for path in files {
        let Ok(text) = std::fs::read_to_string(root().join(path)) else {
            continue;
        };
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            if let Some((from, to)) = line.split_once(" -> ") {
                renames.insert(from.trim().to_string(), to.trim().to_string());
            }
        }
    }
    let mut missing = Vec::new();
    for name in old.lines() {
        if tests.contains(name) {
            continue;
        }
        let mut target = renames.get(name);
        let mut hops = 0;
        while let Some(to) = target
            && !tests.contains(to)
            && hops < 16
        {
            match renames.get(to) {
                Some(next) => {
                    target = Some(next);
                    hops += 1;
                }
                None => break,
            }
        }
        match target {
            Some(to) if to.starts_with("REMOVED:") || tests.contains(to) => {}
            Some(to) => missing.push(format!("{name} -> {to} (target missing)")),
            None => missing.push(name.to_string()),
        }
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} test(s) of {base} disappeared without a rename/removal entry:\n{}",
            missing.len(),
            missing.join("\n")
        ))
    }
}

// --- the gate --------------------------------------------------------------

fn gates(args: &[String]) -> Result<(), String> {
    let full = args.iter().any(|a| a == "--full");
    require_clean_tree()?;
    let hash = head_hash()?;
    let mut timings = Timings::new();
    let started = Instant::now();
    let result = gate_steps(full, &hash, &mut timings);
    timings.record("total", started.elapsed().as_secs_f64(), result.is_ok());
    timings.write(&hash, if full { "gates-full" } else { "gates" })?;
    result?;
    eprintln!("all gates passed");
    Ok(())
}

fn gate_steps(full: bool, hash: &str, timings: &mut Timings) -> Result<(), String> {
    timings.time("fmt", |_| run(cargo().args(["fmt", "--all", "--check"])))?;
    timings.time("clippy", |_| {
        run(cargo().args([
            "clippy",
            "--workspace",
            "--all-targets",
            "--quiet",
            "--",
            "-D",
            "warnings",
        ]))
    })?;
    timings.time("tests", |t| {
        test_timed(
            t,
            "",
            &["--workspace", "--lib", "--bins", "--tests", "--", "--quiet"],
        )
    })?;
    timings.time("benches in test mode", |t| {
        test_timed(
            t,
            "gate ",
            &[
                "--profile",
                "gate",
                "--workspace",
                "--bench",
                "*",
                "--",
                "--quiet",
            ],
        )
    })?;
    record_fixture_types(hash, timings)?;
    timings.time("test manifest", |_| test_manifest())?;
    timings.time("module graph", |_| modgraph::modgraph(&root(), &[]))?;
    timings.time("arch", |_| arch::arch(&root()))?;
    timings.time("doc", |_| {
        run(cargo()
            .args(["doc", "--workspace", "--no-deps", "--quiet"])
            .env("RUSTDOCFLAGS", "-D warnings"))
    })?;
    timings.time("fuzz check", |_| {
        run(cargo().args([
            "check",
            "--quiet",
            "--manifest-path",
            "fuzz/Cargo.toml",
            "--all-targets",
        ]))
    })?;
    timings.time("no-default-features check", |_| {
        run(cargo().args([
            "check",
            "--quiet",
            "--no-default-features",
            "--bin",
            "phpstan-rs",
        ]))
    })?;
    if full {
        timings.time("bench compile", |_| {
            run(cargo().args(["bench", "--workspace", "--no-run", "--quiet"]))
        })?;
        timings.time("no-default-features tests", |t| {
            test_timed(
                t,
                "no-default-features ",
                &["--workspace", "--no-default-features", "--", "--quiet"],
            )
        })?;
        timings.time("no-default-features fixtures", |_| {
            run(cargo().args([
                "test",
                "--profile",
                "gate",
                "--no-default-features",
                "--test",
                "fixtures",
                "--quiet",
            ]))
        })?;
        timings.time("feature matrix", |_| {
            run(cargo().args([
                "hack",
                "check",
                "--workspace",
                "--each-feature",
                "--exclude-all-features",
                "--quiet",
            ]))
        })?;
        // cargo-machete takes itself for a `cargo run` binary, and its own
        // name for a path, while `CARGO_PKG_NAME` is set.
        timings.time("machete", |_| {
            run(cargo()
                .args(["machete", "crates", "tools", "xtask", "fuzz"])
                .env_remove("CARGO_PKG_NAME"))
        })?;
        timings.time("deny", |_| run(cargo().args(["deny", "check"])))?;
    }
    let mut corpus_args = Vec::new();
    if full {
        corpus_args.push("--full".to_string());
    }
    pass::run_pass(
        &pass::PassSetup {
            full,
            fixtures: false,
            profile: "gate",
            hash: Some(hash.to_string()),
        },
        timings,
    )?;
    timings.time("compare", |_| compare(&corpus_args))?;
    if full {
        timings.time("perf", |_| perf())?;
        timings.time("buildtime", |_| buildtime())?;
    }
    Ok(())
}
