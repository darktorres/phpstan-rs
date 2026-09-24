//! Regression gates for phpstan-rs (`cargo xtask <command>`).
//!
//! Snapshots live under `.corpus-runs/gate/<artifact>/<commit>.<ext>`
//! (gitignored), keyed by the full hash of the commit the binary was
//! built from. `compare` checks HEAD's snapshots against its parent's:
//! every artifact must be byte-identical unless HEAD's own commit adds
//! `gates/approved/<slug>/<artifact>.diff` holding exactly the expected
//! `diff -u` output.
//!
//! The corpus is the frozen Piwigo checkout at `$PHPSTAN_RS_CORPUS`
//! (default `/home/torres/piwigo17-frozen`).

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};
use std::time::Instant;


const LEVELS: [&str; 12] = [
    "unset", "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10",
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("gates") => gates(&args[1..]),
        Some("corpus") => corpus(&args[1..]),
        Some("compare") => compare(&args[1..]),
        Some("perf") => perf(),
        Some("buildtime") => buildtime(),
        Some("tests") => test_manifest(),
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
  corpus [--full]        record HEAD's corpus, type and baseline snapshots
  compare [--full] [--against <hash>]
                         diff HEAD's snapshots against its parent's
  perf                   record wall time and peak memory of the scoped run
  buildtime              record incremental rebuild times
  tests                  record and check the test-name manifest
  baseline               generate the reference baselines (Stage A only)
  approve <slug> [--full]
                         write HEAD's snapshot diffs against its parent to
                         gates/approved/<slug>/ (then amend them in)
  rekey <old-hash>       move <old-hash>'s snapshots to HEAD after an amend
                         that only added gates/ files";

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

/// The main checkout's `.corpus-runs/gate`, also from a linked worktree,
/// so every worktree records into and compares against one store.
fn gate_dir() -> PathBuf {
    let common = git(&["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .map(PathBuf::from)
        .unwrap_or_else(|_| root().join(".git"));
    common
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(root)
        .join(".corpus-runs/gate")
}

/// The `gate` profile's binary: what the corpus, type and baseline
/// snapshots run.
fn binary() -> PathBuf {
    root().join("target/gate/phpstan-rs")
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
    run(cargo().args(["build", "--profile", "gate", "--bin", "phpstan-rs"]))
}

fn build_release() -> Result<(), String> {
    run(cargo().args(["build", "--release", "--bin", "phpstan-rs"]))
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

fn all_on_args() -> Vec<String> {
    vec!["--config".into(), ALL_ON_FILE.into()]
}

const ALL_ON_FILE: &str = ".phpstan-rs-gate-all-on.toml";

/// Every opt-in check on, over the scoped path set: the rules the default
/// configurations never exercise.
fn write_all_on_config() -> Result<(), String> {
    let scoped = std::fs::read_to_string(corpus_dir().join("phpstan-rs.toml"))
        .map_err(|e| format!("reading the scoped config: {e}"))?;
    let (head, tables) = match scoped.find("\n[") {
        Some(i) => scoped.split_at(i + 1),
        None => (scoped.as_str(), ""),
    };
    let mut toml = String::from(head);
    for key in ALL_ON_TOGGLES {
        writeln!(toml, "{key} = true").unwrap();
    }
    toml.push_str("restricted_symbol_presets = [\"multibyte-strings\"]\n");
    toml.push_str(
        "banned_constructs = [\"eval\", \"goto\", \"global\", \"backticks\", \"variable-variables\"]\n",
    );
    toml.push_str(tables);
    std::fs::write(corpus_dir().join(ALL_ON_FILE), toml)
        .map_err(|e| format!("writing {ALL_ON_FILE}: {e}"))
}

const ALL_ON_TOGGLES: &[&str] = &[
    "strict_mixed_arrays",
    "check_uninitialized_properties",
    "check_internal_usage",
    "check_function_name_case",
    "check_deprecated_usage",
    "check_empty_method_bodies",
    "check_missing_checked_exceptions",
    "check_strict_printf_placeholder_types",
    "check_too_wide_throw_type_in_protected_and_public_methods",
    "check_too_wide_return_type_in_protected_and_public_methods",
    "check_too_wide_by_ref_type_in_protected_and_public_methods",
    "check_method_throw_type_covariance",
    "check_benevolent_union_types",
    "check_missing_callable_signature",
    "check_closed_classes",
    "default_parent_calls",
    "check_mock_hygiene",
    "check_duplicate_function_declarations",
    "check_duplicate_class_declarations",
    "check_boolean_trap",
    "check_array_key_cast",
    "check_parent_constructor_call",
    "check_promoted_property_suggestions",
    "check_boolean_conditions",
    "check_dynamic_properties",
    "check_swapped_arguments",
    "check_never_return_suggestions",
    "check_switch_missing_default",
    "check_throw_without_message",
    "check_throw_base_class",
    "check_exception_hygiene",
    "check_current_time_dependence",
    "check_psr3_exception_context",
    "check_psr3_logging",
    "check_magic_numbers",
    "check_native_type_hints",
    "check_implicit_array_creation",
    "check_useless_casts",
    "check_strict_array_filter",
    "check_mixed_type_operands",
    "check_comparable_types",
    "check_arithmetic_operands",
    "check_identity_vs_equals",
    "check_loose_haystack_comparison",
    "check_strict_function_calls",
    "check_switch_case_types",
    "check_loop_variable_overwrite",
    "check_dynamic_static_calls",
    "check_illegal_constructor_calls",
    "check_enum_exhaustiveness",
    "check_null_coercion",
    "check_new_exception_discarded",
    "check_risky_tests",
    "check_dead_enum_cases",
    "check_dead_code",
    "check_circular_dependencies",
    "check_dead_abstract_classes",
    "check_dead_members",
    "check_renamed_named_arguments",
    "report_unmatched_ignored_errors",
    "check_composer_hygiene",
];

const SCOPED: Config = Config {
    name: "scoped",
    args: scoped_args,
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

/// One line per diagnostic, in output order:
/// `path \t line \t col \t level \t identifier \t location \t message \t tip`.
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
                "{path}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
                m["line"],
                m["column"],
                m["level"],
                field("identifier"),
                field("location"),
                field("message"),
                field("tip"),
            )
            .unwrap();
        }
    }
    Ok(out)
}

fn snapshot_path(artifact: &str, hash: &str, ext: &str) -> PathBuf {
    gate_dir().join(artifact).join(format!("{hash}.{ext}"))
}

fn write(path: &Path, contents: &[u8]) -> Result<(), String> {
    std::fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(path, contents).map_err(|e| format!("writing {}: {e}", path.display()))
}

fn run_corpus(config: &Config, level: &str, extra: &[String]) -> Result<(String, String), String> {
    let mut cmd = Command::new(binary());
    cmd.current_dir(corpus_dir())
        .arg("check-project")
        .args((config.args)())
        .args(["--format", "json"])
        .args(extra);
    if level != "unset" {
        cmd.args(["--level", level]);
    }
    let out = cmd
        .output()
        .map_err(|e| format!("running phpstan-rs on {}: {e}", config.name))?;
    Ok((
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    ))
}

fn corpus(args: &[String]) -> Result<(), String> {
    let full = args.iter().any(|a| a == "--full");
    require_clean_tree()?;
    let hash = head_hash()?;
    build_gate()?;
    write_all_on_config()?;
    for (config, level) in artifacts(full) {
        let artifact = artifact_name(&config, level);
        let started = Instant::now();
        let mut extra = Vec::new();
        let dump = level == "unset" && config.name != "unscoped";
        let dump_file = gate_dir().join(format!(".{artifact}-types.tmp"));
        if dump {
            extra.push("--dump-types".to_string());
            extra.push(dump_file.display().to_string());
        }
        let (stdout, stderr) = run_corpus(&config, level, &extra)?;
        write(
            &snapshot_path(&artifact, &hash, "txt"),
            normalize(&stdout)?.as_bytes(),
        )?;
        write(&snapshot_path(&artifact, &hash, "err"), stderr.as_bytes())?;
        if dump {
            let target = snapshot_path(&format!("types-{artifact}"), &hash, "txt.zst");
            std::fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
            run(Command::new("zstd")
                .args(["-q", "-f", "-19", "--rm", "-T0"])
                .arg(&dump_file)
                .arg("-o")
                .arg(&target))?;
        }
        eprintln!(
            "recorded {artifact} in {:.1}s",
            started.elapsed().as_secs_f64()
        );
    }
    baseline_check(&hash)?;
    determinism_check(full)?;
    Ok(())
}

/// Runs a configuration twice with `--emission-order` (each file's
/// diagnostics in the order the checks produced them, no final sort) and
/// requires identical output: the engine itself must be reproducible,
/// not just the sorted report.
fn determinism_check(full: bool) -> Result<(), String> {
    let mut configs = vec![ALL_ON];
    if full {
        configs.push(UNSCOPED);
    }
    for config in configs {
        let flag = ["--emission-order".to_string()];
        let (first, first_err) = run_corpus(&config, "unset", &flag)?;
        let (second, second_err) = run_corpus(&config, "unset", &flag)?;
        if first != second || first_err != second_err {
            let diff = unified_diff(first.as_bytes(), second.as_bytes())?;
            let shown: String = diff.lines().take(40).collect::<Vec<_>>().join("\n");
            return Err(format!(
                "{}: two runs emitted different output\n{shown}",
                config.name
            ));
        }
        eprintln!("{}: emission order reproducible", config.name);
    }
    Ok(())
}

// --- baseline compatibility ----------------------------------------------

fn baseline_file(config: &str) -> PathBuf {
    gate_dir().join("baseline").join(format!("{config}.neon"))
}

/// Records the reference baselines once, from the Stage A binary; every
/// later commit must keep them producing no diagnostics and no stale
/// entries.
fn baseline_generate() -> Result<(), String> {
    build_gate()?;
    write_all_on_config()?;
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

fn baseline_check(hash: &str) -> Result<(), String> {
    for config in [SCOPED, ALL_ON] {
        let file = baseline_file(config.name);
        if !file.exists() {
            eprintln!(
                "no reference baseline for {} yet; run `cargo xtask baseline`",
                config.name
            );
            continue;
        }
        let (stdout, stderr) = run_corpus(
            &config,
            "unset",
            &["--baseline".to_string(), file.display().to_string()],
        )?;
        let normalized = normalize(&stdout)?;
        let artifact = format!("baseline-{}", config.name);
        write(
            &snapshot_path(&artifact, hash, "txt"),
            normalized.as_bytes(),
        )?;
        write(&snapshot_path(&artifact, hash, "err"), stderr.as_bytes())?;
        let remaining = normalized.lines().filter(|l| !l.starts_with('#')).count();
        let stale = stderr
            .lines()
            .filter(|l| l.contains("baseline entry"))
            .count();
        // The Stage A snapshot is empty; `compare` holds every later one
        // to its parent's plus the commit's approved delta.
        if remaining != 0 || stale != 0 {
            eprintln!(
                "baseline compatibility ({}): {remaining} diagnostic(s) and {stale} stale entr(ies) with the reference baseline",
                config.name
            );
        }
    }
    Ok(())
}

// --- comparison ------------------------------------------------------------

fn parent_hash() -> Result<String, String> {
    git(&["rev-parse", "HEAD~1"])
}

/// `gates/approved/*/<artifact>.diff` files HEAD's own commit adds or
/// changes.
fn approved_delta(artifact: &str) -> Result<Option<String>, String> {
    let changed = git(&["show", "--name-only", "--format=", "HEAD"])?;
    for path in changed.lines() {
        if path.starts_with("gates/approved/")
            && Path::new(path).file_name().and_then(|f| f.to_str())
                == Some(&format!("{artifact}.diff"))
        {
            let text = std::fs::read_to_string(root().join(path)).map_err(|e| e.to_string())?;
            return Ok(Some(text));
        }
    }
    Ok(None)
}

fn read_snapshot(artifact: &str, hash: &str, ext: &str) -> Result<Option<Vec<u8>>, String> {
    let path = snapshot_path(artifact, hash, ext);
    if !path.exists() {
        return Ok(None);
    }
    if ext.ends_with(".zst") {
        let text = capture(Command::new("zstd").args(["-dc", "-q"]).arg(&path))?;
        return Ok(Some(text.into_bytes()));
    }
    std::fs::read(&path).map(Some).map_err(|e| e.to_string())
}

fn unified_diff(old: &[u8], new: &[u8]) -> Result<String, String> {
    let dir = gate_dir().join(".diff");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let (a, b) = (dir.join("old"), dir.join("new"));
    std::fs::write(&a, old).map_err(|e| e.to_string())?;
    std::fs::write(&b, new).map_err(|e| e.to_string())?;
    let out = Command::new("diff")
        .args(["-u", "--label", "old", "--label", "new"])
        .arg(&a)
        .arg(&b)
        .output()
        .map_err(|e| e.to_string())?;
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Every artifact `compare` checks, with its file extension.
fn compared_artifacts(full: bool) -> Vec<(String, &'static str)> {
    let mut names = Vec::new();
    for (config, level) in artifacts(full) {
        let artifact = artifact_name(&config, level);
        if level == "unset" && config.name != "unscoped" {
            names.push((format!("types-{artifact}"), "txt.zst"));
            names.push((format!("baseline-{artifact}"), "txt"));
        }
        names.push((artifact, "txt"));
    }
    names
}

fn compare(args: &[String]) -> Result<(), String> {
    let full = args.iter().any(|a| a == "--full");
    let against = match args.iter().position(|a| a == "--against") {
        Some(i) => git(&[
            "rev-parse",
            args.get(i + 1).ok_or("--against needs a hash")?,
        ])?,
        None => parent_hash()?,
    };
    let hash = head_hash()?;
    let mut failures = Vec::new();
    for (artifact, ext) in &compared_artifacts(full) {
        let (Some(old), Some(new)) = (
            read_snapshot(artifact, &against, ext)?,
            read_snapshot(artifact, &hash, ext)?,
        ) else {
            failures.push(format!(
                "{artifact}: missing snapshot for {against} or {hash}"
            ));
            continue;
        };
        if old == new {
            continue;
        }
        let diff = unified_diff(&old, &new)?;
        match approved_delta(artifact)? {
            Some(approved) if approved == diff => {
                eprintln!("{artifact}: approved delta matches");
            }
            Some(_) => failures.push(format!("{artifact}: differs from its approved delta")),
            None => {
                let shown: String = diff.lines().take(40).collect::<Vec<_>>().join("\n");
                failures.push(format!("{artifact}: unapproved change\n{shown}"));
            }
        }
    }
    if failures.is_empty() {
        eprintln!("snapshots of {hash} match {against}");
        Ok(())
    } else {
        Err(failures.join("\n\n"))
    }
}

/// Writes every artifact that differs between HEAD's snapshots and its
/// parent's to `gates/approved/<slug>/<artifact>.diff`, for amending
/// into the commit that makes the change.
fn approve(args: &[String]) -> Result<(), String> {
    let slug = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .ok_or("approve needs a slug")?;
    let full = args.iter().any(|a| a == "--full");
    let (parent, hash) = (parent_hash()?, head_hash()?);
    let dir = root().join("gates/approved").join(slug);
    let mut written = 0usize;
    for (artifact, ext) in compared_artifacts(full) {
        let (Some(old), Some(new)) = (
            read_snapshot(&artifact, &parent, ext)?,
            read_snapshot(&artifact, &hash, ext)?,
        ) else {
            return Err(format!(
                "{artifact}: missing snapshot for {parent} or {hash}"
            ));
        };
        if old != new {
            write(
                &dir.join(format!("{artifact}.diff")),
                unified_diff(&old, &new)?.as_bytes(),
            )?;
            written += 1;
        }
    }
    eprintln!("{written} approved delta(s) written to {}", dir.display());
    Ok(())
}

/// After `git commit --amend` added only `gates/` files, HEAD's
/// snapshots are still keyed by the pre-amend hash: move them.
fn rekey(args: &[String]) -> Result<(), String> {
    let old = args.first().ok_or("rekey needs the pre-amend hash")?;
    let old = &git(&["rev-parse", old])?;
    let hash = head_hash()?;
    let changed = git(&["diff", "--name-only", old, "HEAD"])?;
    if let Some(path) = changed.lines().find(|p| !p.starts_with("gates/")) {
        return Err(format!("{old}..HEAD changes {path}, not only gates/"));
    }
    let mut moved = 0usize;
    for entry in std::fs::read_dir(gate_dir())
        .map_err(|e| e.to_string())?
        .flatten()
    {
        let dir = entry.path();
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
    eprintln!("moved {moved} snapshot file(s) from {old} to {hash}");
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
    let ancestors = git(&["rev-list", "--max-count=200", "HEAD~1"])?;
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
const BUILD_SCENARIOS: &[(&str, &str)] = &[
    ("engine", "crates/phpstan-rs/src/checker/expr.rs"),
    ("rule", "crates/phpstan-rs/src/checker/magic_numbers.rs"),
    ("lexer", "crates/phpstan-rs/src/lexer.rs"),
    ("stub", "vendor/phpstorm-stubs/standard/standard_0.php"),
];

fn buildtime() -> Result<(), String> {
    require_clean_tree()?;
    let hash = head_hash()?;
    run(cargo().args(["test", "--workspace", "--no-run", "--quiet"]))?;
    run(cargo().args(["build", "--release", "--bin", "phpstan-rs", "--quiet"]))?;
    let mut report = String::new();
    for (scenario, file) in BUILD_SCENARIOS {
        for (label, args) in [
            (
                "test-no-run",
                vec!["test", "--workspace", "--no-run", "--quiet"],
            ),
            (
                "build-release",
                vec!["build", "--release", "--bin", "phpstan-rs", "--quiet"],
            ),
        ] {
            run(Command::new("touch").arg(root().join(file)))?;
            let started = Instant::now();
            run(cargo().args(&args))?;
            let secs = started.elapsed().as_secs_f64();
            writeln!(report, "{scenario}\t{label}\t{secs:.1}").unwrap();
            eprintln!("{scenario} {label}: {secs:.1}s");
        }
    }
    write(
        &gate_dir().join("buildtime").join(format!("{hash}.tsv")),
        report.as_bytes(),
    )
}

// --- test-name manifest ----------------------------------------------------

fn list_tests() -> Result<BTreeSet<String>, String> {
    let out = capture(cargo().args([
        "test",
        "--workspace",
        "--all-targets",
        "--quiet",
        "--",
        "--list",
        "--format",
        "terse",
    ]))?;
    // The fixture target is `test = false`, so `--all-targets` leaves it
    // out; list it on its own.
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
        .chain(fixtures.lines())
        .filter_map(|l| {
            l.strip_suffix(": test")
                .or_else(|| l.strip_suffix(": benchmark"))
        })
        .map(str::to_string)
        .collect())
}

/// Every test of the parent commit must still exist, or be listed in a
/// `gates/test-renames/*.txt` file HEAD's commit adds, as
/// `old -> new` or `old -> REMOVED: reason`.
fn test_manifest() -> Result<(), String> {
    require_clean_tree()?;
    let hash = head_hash()?;
    let tests = list_tests()?;
    let listing: String = tests.iter().map(|t| format!("{t}\n")).collect();
    let dir = gate_dir().join("tests");
    write(&dir.join(format!("{hash}.txt")), listing.as_bytes())?;
    eprintln!("{hash}: {} tests", tests.len());
    let parent = parent_hash()?;
    let Ok(old) = std::fs::read_to_string(dir.join(format!("{parent}.txt"))) else {
        eprintln!("no manifest recorded for {parent}; nothing to compare");
        return Ok(());
    };
    let mut renames = std::collections::BTreeMap::new();
    let changed = git(&["show", "--name-only", "--format=", "HEAD"])?;
    for path in changed
        .lines()
        .filter(|p| p.starts_with("gates/test-renames/"))
    {
        let text = std::fs::read_to_string(root().join(path)).map_err(|e| e.to_string())?;
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
        match renames.get(name) {
            Some(to) if to.starts_with("REMOVED:") || tests.contains(to) => {}
            Some(to) => missing.push(format!("{name} -> {to} (target missing)")),
            None => missing.push(name.to_string()),
        }
    }
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} test(s) of {parent} disappeared without a rename/removal entry:\n{}",
            missing.len(),
            missing.join("\n")
        ))
    }
}

// --- the gate --------------------------------------------------------------

fn gates(args: &[String]) -> Result<(), String> {
    let full = args.iter().any(|a| a == "--full");
    require_clean_tree()?;
    run(cargo().args(["fmt", "--all", "--check"]))?;
    run(cargo().args([
        "clippy",
        "--workspace",
        "--all-targets",
        "--quiet",
        "--",
        "-D",
        "warnings",
    ]))?;
    run(cargo().args(["test", "--workspace", "--all-targets", "--quiet"]))?;
    run(cargo().args(["test", "--profile", "gate", "--test", "fixtures", "--quiet"]))?;
    test_manifest()?;
    run(cargo().args(["doc", "--workspace", "--no-deps", "--quiet"]))?;
    run(cargo().args(["bench", "--no-run", "--quiet"]))?;
    run(cargo().args([
        "check",
        "--quiet",
        "--manifest-path",
        "fuzz/Cargo.toml",
        "--all-targets",
    ]))?;
    run(cargo().args([
        "build",
        "--quiet",
        "--no-default-features",
        "--bin",
        "phpstan-rs",
    ]))?;
    if full {
        run(cargo().args(["test", "--workspace", "--quiet", "--no-default-features"]))?;
        run(cargo().args([
            "hack",
            "check",
            "--workspace",
            "--each-feature",
            "--no-dev-deps",
            "--mutually-exclusive-features",
            "dhat-heap,hotpath-alloc",
            "--quiet",
        ]))?;
        run(cargo().args(["machete"]))?;
        run(cargo().args(["deny", "check"]))?;
    }
    let mut corpus_args = Vec::new();
    if full {
        corpus_args.push("--full".to_string());
    }
    corpus(&corpus_args)?;
    compare(&corpus_args)?;
    if full {
        perf()?;
        buildtime()?;
    }
    eprintln!("all gates passed");
    Ok(())
}
