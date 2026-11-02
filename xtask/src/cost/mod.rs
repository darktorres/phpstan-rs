//! `cargo xtask cost`: the cost meter (`docs/design/cost-meter.md`).
//!
//! It builds the `cost` profile of `phpstan-rs`, runs each workload of
//! `perf/workloads.toml` under callgrind and under valgrind's DHAT, and
//! either compares the counts with a build of a base commit (the gate) or
//! records them in `perf/history.tsv`.

mod callgrind;
mod dhat;

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::files;

const USAGE: &str = "usage: cargo xtask cost [--workload <glob>] [--base <commit>] [--record]";

/// Rows of the top-20 tables.
const TOP: usize = 20;

/// The gate's bounds: a rise fails when it is over both the relative and
/// the absolute bound. The absolute bounds keep a change of a few bytes or
/// instructions in a tiny workload from failing it.
const IR_RISE: f64 = 0.01;
const IR_FLOOR: u64 = 10_000;
const HEAP_RISE: f64 = 0.02;
const HEAP_FLOOR: u64 = 1_024;

struct Workload {
    name: String,
    args: Vec<String>,
    /// Globs under phpstan-src whose files go to stdin, one path per line.
    stdin_files: Vec<String>,
}

struct Measurement {
    ir: u64,
    blocks: u64,
    bytes: u64,
    peak: u64,
    live: u64,
    functions: Vec<(String, u64)>,
    sites: Vec<(String, u64)>,
}

pub(crate) fn run(root: &Path, args: &[String]) -> Result<(), String> {
    let mut filter = None;
    let mut base = None;
    let mut record = false;
    let mut rest = args.iter();
    while let Some(arg) = rest.next() {
        let mut value = || {
            rest.next()
                .cloned()
                .ok_or_else(|| format!("{arg} needs a value\n{USAGE}"))
        };
        match arg.as_str() {
            "--workload" => filter = Some(value()?),
            "--base" => base = Some(value()?),
            "--record" => record = true,
            _ => return Err(format!("unknown argument {arg}\n{USAGE}")),
        }
    }
    if record && base.is_some() {
        return Err("--record measures the checked-out commit; it takes no --base".to_owned());
    }
    let workloads: Vec<Workload> = read_workloads(&root.join("perf/workloads.toml"))?
        .into_iter()
        .filter(|w| {
            filter
                .as_deref()
                .is_none_or(|pattern| files::glob(pattern, &w.name))
        })
        .collect();
    if workloads.is_empty() {
        return Err("no workload matches".to_owned());
    }
    let scratch = files::cache_dir()?.join("phpstan-rs/cost");
    let head_commit = if record {
        Some(clean_head(root)?)
    } else {
        None
    };
    let head = measure_all(
        root,
        &build(root, &scratch.join("target"))?,
        &workloads,
        &scratch,
    )?;
    match (base, head_commit) {
        (Some(base), _) => {
            let base_binary = build_commit(root, &base, &scratch)?;
            let before = measure_all(root, &base_binary, &workloads, &scratch)?;
            gate(root, &base, &workloads, &before, &head)
        }
        (None, Some(commit)) => {
            write_history(root, &commit, &workloads, &head)?;
            print_counts(&workloads, &head);
            println!("recorded {} workloads for {commit}", workloads.len());
            Ok(())
        }
        (None, None) => {
            print_counts(&workloads, &head);
            Ok(())
        }
    }
}

fn read_workloads(path: &Path) -> Result<Vec<Workload>, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let table: toml::Table = text
        .parse()
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let strings = |value: Option<&toml::Value>| -> Vec<String> {
        value
            .and_then(toml::Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(toml::Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default()
    };
    table
        .iter()
        .map(|(name, spec)| {
            let cache = spec
                .get("cache")
                .and_then(toml::Value::as_str)
                .unwrap_or("off");
            if cache != "off" {
                return Err(format!(
                    "workload {name}: cache mode {cache} is not supported yet"
                ));
            }
            Ok(Workload {
                name: name.clone(),
                args: strings(spec.get("args")),
                stdin_files: strings(spec.get("stdin").and_then(|stdin| stdin.get("files"))),
            })
        })
        .collect()
}

/// Builds the `cost` profile of the workspace at `dir` into `target` and
/// returns the binary. Frame pointers are on, so a sampling profile of the
/// same binary unwinds.
fn build(dir: &Path, target: &Path) -> Result<PathBuf, String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .current_dir(dir)
        .args([
            "build",
            "--quiet",
            "--locked",
            "--profile",
            "cost",
            "--package",
            "phpstan-rs",
        ])
        .env("CARGO_TARGET_DIR", target)
        .env("RUSTFLAGS", "-C force-frame-pointers=yes")
        .env_remove("CARGO_ENCODED_RUSTFLAGS")
        .status()
        .map_err(|error| format!("cargo build: {error}"))?;
    if !status.success() {
        return Err(format!("cargo build of {} failed: {status}", dir.display()));
    }
    Ok(target
        .join("cost")
        .join(format!("phpstan-rs{}", std::env::consts::EXE_SUFFIX)))
}

/// Builds `commit` in a throwaway worktree, which is removed again.
fn build_commit(root: &Path, commit: &str, scratch: &Path) -> Result<PathBuf, String> {
    let tree = scratch.join("base");
    let tree_arg = tree.to_string_lossy().into_owned();
    if tree.exists() {
        unlink_submodule(&tree)?;
        git(root, &["worktree", "remove", "--force", &tree_arg])?;
    }
    git(root, &["worktree", "add", "--detach", &tree_arg, commit])?;
    let result =
        submodule(root, &tree, commit).and_then(|()| build(&tree, &scratch.join("target-base")));
    let removed = unlink_submodule(&tree)
        .and_then(|()| git(root, &["worktree", "remove", "--force", &tree_arg]));
    let binary = result?;
    removed?;
    Ok(binary)
}

/// Gives the base worktree its phpstan-src. At the head's commit, which is
/// the usual case while the submodule is frozen, it is a link to the head's
/// checkout; otherwise a clone of the base's commit.
fn submodule(root: &Path, tree: &Path, commit: &str) -> Result<(), String> {
    let gitlink = |rev: &str| {
        command_text(
            Command::new("git")
                .arg("-C")
                .arg(root)
                .args(["rev-parse", &format!("{rev}:vendor/phpstan-src")]),
        )
    };
    if gitlink("HEAD")? != gitlink(commit)? {
        return git(
            tree,
            &[
                "submodule",
                "update",
                "--init",
                "--depth",
                "1",
                "vendor/phpstan-src",
            ],
        );
    }
    let path = tree.join("vendor/phpstan-src");
    if path.exists() {
        fs::remove_dir(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    }
    link(&root.join("vendor/phpstan-src"), &path)
}

#[cfg(unix)]
fn link(target: &Path, path: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(target, path).map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(not(unix))]
fn link(_target: &Path, path: &Path) -> Result<(), String> {
    Err(format!(
        "{}: the cost meter runs on Linux only",
        path.display()
    ))
}

/// Removes the link `submodule` made, so removing the worktree can never
/// reach the head's checkout through it.
fn unlink_submodule(tree: &Path) -> Result<(), String> {
    let path = tree.join("vendor/phpstan-src");
    match fs::symlink_metadata(&path) {
        Ok(meta) if meta.file_type().is_symlink() => {
            fs::remove_file(&path).map_err(|error| format!("{}: {error}", path.display()))
        }
        _ => Ok(()),
    }
}

fn git(dir: &Path, args: &[&str]) -> Result<(), String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|error| format!("git: {error}"))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "git {}: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        ))
    }
}

fn measure_all(
    root: &Path,
    binary: &Path,
    workloads: &[Workload],
    scratch: &Path,
) -> Result<Vec<Measurement>, String> {
    // Both sides run from the same path: the program's own path is part of
    // what it counts (argv[0]).
    let bin = scratch.join("bin");
    fs::create_dir_all(&bin).map_err(|error| format!("{}: {error}", bin.display()))?;
    let measured = bin.join(binary.file_name().unwrap_or_default());
    fs::copy(binary, &measured).map_err(|error| format!("{}: {error}", measured.display()))?;
    let binary = measured.as_path();
    let src = root.join("vendor/phpstan-src");
    let all_files = files::php_files(&src, &src)?;
    workloads
        .iter()
        .map(|workload| {
            let mut stdin = Vec::new();
            for path in all_files
                .iter()
                .filter(|p| workload.stdin_files.iter().any(|g| files::glob(g, p)))
            {
                stdin.extend_from_slice(path.as_bytes());
                stdin.push(b'\n');
            }
            let args: Vec<String> = workload
                .args
                .iter()
                .map(|arg| arg.replace("{src}", &src.to_string_lossy()))
                .collect();
            measure(
                binary,
                &args,
                &stdin,
                &scratch.join("run").join(&workload.name),
            )
        })
        .collect()
}

/// One workload under callgrind, then under DHAT, each with an empty
/// environment apart from `PATH` and a private `HOME`.
fn measure(
    binary: &Path,
    args: &[String],
    stdin: &[u8],
    dir: &Path,
) -> Result<Measurement, String> {
    if dir.exists() {
        fs::remove_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    }
    let home = dir.join("home");
    fs::create_dir_all(&home).map_err(|error| format!("{}: {error}", home.display()))?;
    let input = dir.join("stdin");
    fs::write(&input, stdin).map_err(|error| format!("{}: {error}", input.display()))?;
    let callgrind_out = dir.join("callgrind.out");
    let dhat_out = dir.join("dhat.out");
    valgrind(
        &[
            "--tool=callgrind".to_owned(),
            format!("--callgrind-out-file={}", callgrind_out.display()),
        ],
        binary,
        args,
        &input,
        &home,
    )?;
    valgrind(
        &[
            "--tool=dhat".to_owned(),
            format!("--dhat-out-file={}", dhat_out.display()),
        ],
        binary,
        args,
        &input,
        &home,
    )?;
    let read = |path: &Path| {
        fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))
    };
    let profile = callgrind::parse(&read(&callgrind_out)?)?;
    let heap = dhat::parse(&read(&dhat_out)?)?;
    Ok(Measurement {
        ir: profile.total,
        blocks: heap.blocks,
        bytes: heap.bytes,
        peak: heap.peak,
        live: heap.live,
        functions: profile.functions.into_iter().take(TOP).collect(),
        sites: heap.sites.into_iter().take(TOP).collect(),
    })
}

fn valgrind(
    tool: &[String],
    binary: &Path,
    args: &[String],
    stdin: &Path,
    home: &Path,
) -> Result<(), String> {
    let input = fs::File::open(stdin).map_err(|error| format!("{}: {error}", stdin.display()))?;
    // The count must not depend on the run. The guest reads /proc/self/maps
    // (Rust's thread setup does), so setarch -R turns address randomization
    // off, and --vgdb=no drops the PID-named pipe valgrind would otherwise
    // map (and create under /tmp). The command runs on its own thread, and
    // whether the main thread's join has to wait was a race; --fair-sched
    // hands valgrind's lock out in FIFO order, which fixes that order.
    let output = Command::new("setarch")
        .args(["-R", "valgrind", "--vgdb=no", "--fair-sched=yes"])
        .args(tool)
        .arg(binary)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", home)
        .stdin(input)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| {
            format!("setarch: {error}; the cost meter needs setarch (util-linux) and valgrind")
        })?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "valgrind {} failed ({}):\n{}",
            tool[0],
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

/// The checked-out commit, provided no tracked file differs from it.
fn clean_head(root: &Path) -> Result<String, String> {
    let status = command_text(Command::new("git").arg("-C").arg(root).args([
        "status",
        "--porcelain",
        "--untracked-files=no",
    ]))?;
    if !status.is_empty() {
        return Err("--record measures a commit; commit or stash the changes first".to_owned());
    }
    command_text(
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "HEAD"]),
    )
}

fn command_text(command: &mut Command) -> Result<String, String> {
    let output = command
        .output()
        .map_err(|error| format!("{command:?}: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "{command:?}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// The counts the gate compares, by name.
fn gated(m: &Measurement) -> [(&'static str, u64, f64, u64); 3] {
    [
        ("ir", m.ir, IR_RISE, IR_FLOOR),
        ("peak", m.peak, HEAP_RISE, HEAP_FLOOR),
        ("live", m.live, HEAP_RISE, HEAP_FLOOR),
    ]
}

/// Whether `after` rose past both bounds over `before`.
fn rose(before: u64, after: u64, relative: f64, floor: u64) -> bool {
    let rise = after.saturating_sub(before);
    let over_relative = rise as f64 > before as f64 * relative;
    rise > floor && over_relative
}

fn gate(
    root: &Path,
    base: &str,
    workloads: &[Workload],
    before: &[Measurement],
    after: &[Measurement],
) -> Result<(), String> {
    let mut failures = Vec::new();
    for ((workload, b), a) in workloads.iter().zip(before).zip(after) {
        println!("{}:", workload.name);
        for ((name, was, relative, floor), (_, now, _, _)) in gated(b).into_iter().zip(gated(a)) {
            let flag = if rose(was, now, relative, floor) {
                "  RISE"
            } else {
                ""
            };
            println!(
                "  {name:<5} {was:>14} -> {now:>14} ({}){flag}",
                percent(was, now)
            );
            if !flag.is_empty() {
                failures.push(format!("{} {name}", workload.name));
            }
        }
        if failures
            .iter()
            .any(|f| f.starts_with(&format!("{} ", workload.name)))
        {
            print_diff("self Ir by function", &b.functions, &a.functions);
            print_diff("bytes by allocation site", &b.sites, &a.sites);
        }
    }
    if failures.is_empty() {
        println!("cost: no rise against {base}");
        return Ok(());
    }
    let messages = command_text(Command::new("git").arg("-C").arg(root).args([
        "log",
        "--format=%B",
        &format!("{base}..HEAD"),
    ]))?;
    if messages.lines().any(|line| line.starts_with("cost:")) {
        println!(
            "cost: rises accepted by a cost: line in {base}..HEAD: {}",
            failures.join(", ")
        );
        return Ok(());
    }
    Err(format!(
        "cost rose against {base}: {}; explain it with a cost: line in the commit message",
        failures.join(", ")
    ))
}

fn percent(before: u64, after: u64) -> String {
    if before == 0 {
        return if after == 0 {
            "+0.00%".to_owned()
        } else {
            "new".to_owned()
        };
    }
    let change = (after as f64 - before as f64) / before as f64 * 100.0;
    format!("{change:+.2}%")
}

/// The top-20 rows that changed. Function names move with inlining, so a
/// row that only moved is not a cost by itself; inclusive costs are never
/// shown or summed.
fn print_diff(title: &str, before: &[(String, u64)], after: &[(String, u64)]) {
    println!("    {title} (top {TOP}; names move with inlining):");
    let was: BTreeMap<&str, u64> = before.iter().map(|(n, c)| (n.as_str(), *c)).collect();
    let now: BTreeMap<&str, u64> = after.iter().map(|(n, c)| (n.as_str(), *c)).collect();
    let mut names: Vec<&str> = was.keys().chain(now.keys()).copied().collect();
    names.sort_unstable();
    names.dedup();
    let mut rows: Vec<(i128, &str)> = names
        .into_iter()
        .map(|name| {
            (
                i128::from(now.get(name).copied().unwrap_or(0))
                    - i128::from(was.get(name).copied().unwrap_or(0)),
                name,
            )
        })
        .filter(|(delta, _)| *delta != 0)
        .collect();
    rows.sort_by(|a, b| b.0.abs().cmp(&a.0.abs()).then_with(|| a.1.cmp(b.1)));
    for (delta, name) in rows {
        println!("      {delta:>+14}  {name}");
    }
}

fn print_counts(workloads: &[Workload], measurements: &[Measurement]) {
    println!(
        "{:<20} {:>14} {:>10} {:>12} {:>12} {:>12}",
        "workload", "ir", "blocks", "bytes", "peak", "live"
    );
    for (workload, m) in workloads.iter().zip(measurements) {
        println!(
            "{:<20} {:>14} {:>10} {:>12} {:>12} {:>12}",
            workload.name, m.ir, m.blocks, m.bytes, m.peak, m.live
        );
    }
}

const HISTORY_HEADER: &str = "commit\tworkload\tir\tblocks\tbytes\tpeak_bytes\tlive_bytes";

/// Replaces `commit`'s rows of `perf/history.tsv` and the workloads'
/// `perf/top/<workload>.txt` tables.
fn write_history(
    root: &Path,
    commit: &str,
    workloads: &[Workload],
    measurements: &[Measurement],
) -> Result<(), String> {
    let path = root.join("perf/history.tsv");
    let existing = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("{}: {error}", path.display())),
    };
    let names: Vec<&str> = workloads.iter().map(|w| w.name.as_str()).collect();
    let mut out = format!("{HISTORY_HEADER}\n");
    for line in existing.lines().skip(1) {
        let mut cells = line.split('\t');
        let (row_commit, row_workload) = (
            cells.next().unwrap_or_default(),
            cells.next().unwrap_or_default(),
        );
        if row_commit != commit || !names.contains(&row_workload) {
            out.push_str(line);
            out.push('\n');
        }
    }
    for (workload, m) in workloads.iter().zip(measurements) {
        let _ = writeln!(
            out,
            "{commit}\t{}\t{}\t{}\t{}\t{}\t{}",
            workload.name, m.ir, m.blocks, m.bytes, m.peak, m.live
        );
    }
    files::write_atomically(&path, out.as_bytes())?;
    let top = root.join("perf/top");
    fs::create_dir_all(&top).map_err(|error| format!("{}: {error}", top.display()))?;
    for (workload, m) in workloads.iter().zip(measurements) {
        let mut text = format!(
            "# {} at {commit}\n\n## self Ir by function\n\n",
            workload.name
        );
        for (name, cost) in &m.functions {
            let _ = writeln!(text, "{cost:>14}  {name}");
        }
        text.push_str("\n## bytes by allocation site\n\n");
        for (site, bytes) in &m.sites {
            let _ = writeln!(text, "{bytes:>14}  {site}");
        }
        files::write_atomically(&top.join(format!("{}.txt", workload.name)), text.as_bytes())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rise_must_pass_both_bounds() {
        assert!(!rose(1_000_000, 1_009_000, IR_RISE, IR_FLOOR));
        assert!(rose(10_000_000, 10_200_000, IR_RISE, IR_FLOOR));
        assert!(
            !rose(100_000, 105_000, IR_RISE, IR_FLOOR),
            "under the absolute floor"
        );
        assert!(
            !rose(10_000_000, 9_000_000, IR_RISE, IR_FLOOR),
            "a fall is no rise"
        );
    }
}
