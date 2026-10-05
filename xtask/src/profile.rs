//! Profiles of the runs the corpus pass makes (`cargo xtask profile <kind>
//! [<project>]`): the frame-pointer `dhat` binaries of HEAD, a stub cache of
//! their own warmed by a first run, a warm run timed with its peak memory,
//! and callgrind of the same run with one output per thread, annotated, with
//! a `README` that says how every file was made. A profile is a directory of
//! `~/.cache/prs-profile` (or of `PHPSTAN_RS_PROFILE_DIR`), named by the run
//! and the commit, that later measurements take their runs from, the same
//! way.
//!
//! - **`vendor`**: the gate runner over one vendor project of the corpus,
//!   every rule set on, with no Composer context and the per-file cache off,
//!   as the pass runs it.
//! - **`replay`**: `check-project --edit-replay` over a fresh copy of the
//!   same project, with the same sets, as the pass runs it.

use std::ffi::OsStr;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::{
    ALL_ON_SETS, capture, corpus_dir, git, head_hash, require_clean_tree, root, run,
    vendor_projects, write,
};

/// The project a kind profiles when none is named: 26 files.
const DEFAULT_PROJECT: &str = "vendor/amphp/amp";

/// The kinds of run a profile can take.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Vendor,
    Replay,
}

impl Kind {
    const ALL: [Kind; 2] = [Kind::Vendor, Kind::Replay];

    fn name(self) -> &'static str {
        match self {
            Kind::Vendor => "vendor",
            Kind::Replay => "replay",
        }
    }

    fn parse(name: &str) -> Result<Kind, String> {
        Kind::ALL
            .into_iter()
            .find(|kind| kind.name() == name)
            .ok_or_else(|| {
                let kinds: Vec<&str> = Kind::ALL.iter().map(|kind| kind.name()).collect();
                format!("unknown kind {name:?}; the kinds are {}", kinds.join(", "))
            })
    }

    /// The run's binary: the gate runner takes a vendor project, the
    /// project's own binary takes a replay.
    fn binary(self) -> &'static str {
        match self {
            Kind::Vendor => "gate-runner",
            Kind::Replay => "phpstan-rs",
        }
    }
}

/// One run of a kind, over `project`, in the profile's directory.
struct Workload<'a> {
    kind: Kind,
    project: &'a str,
    dir: &'a Path,
}

impl Workload<'_> {
    /// The command a run of this workload is, behind `wrapper`, writing its
    /// output under `out`; the tree it runs in is new.
    fn prepare(&self, wrapper: &[&OsStr], out: &Path) -> Result<Command, String> {
        remove(out)?;
        let program = self.dir.join(self.kind.binary());
        let mut cmd = match wrapper.split_first() {
            Some((first, rest)) => {
                let mut cmd = Command::new(first);
                cmd.args(rest).arg(&program);
                cmd
            }
            None => Command::new(&program),
        };
        cmd.env("PHPSTAN_RS_CACHE_DIR", self.dir.join("cache"))
            .env("PHPSTAN_RS_NO_FILE_CACHE", "1")
            .env_remove("PHPSTAN_RS_THREADS")
            .env_remove("PHPSTAN_RS_NO_STUB_CACHE")
            .stdout(Stdio::null());
        match self.kind {
            Kind::Vendor => {
                cmd.current_dir(corpus_dir()).arg("--no-composer");
                for set in ALL_ON_SETS {
                    cmd.arg("--set").arg(set);
                }
                cmd.arg("--out").arg(out).arg(self.project);
            }
            Kind::Replay => {
                let copy = self.copy();
                remove(&copy)?;
                run(Command::new("cp")
                    .arg("-a")
                    .arg(corpus_dir().join(self.project))
                    .arg(&copy))?;
                write(&copy.join(".phpstan-rs-replay"), b"")?;
                cmd.current_dir(&copy)
                    .args(["check-project", "--no-composer"]);
                for set in ALL_ON_SETS {
                    cmd.arg("--set").arg(set);
                }
                cmd.arg("--edit-replay").arg(out).arg(".");
            }
        }
        Ok(cmd)
    }

    /// Removes what a run left in the profile's directory beside its output.
    fn finish(&self) -> Result<(), String> {
        remove(&self.copy())
    }

    /// The tree a replay edits.
    fn copy(&self) -> PathBuf {
        self.dir.join("copy")
    }
}

/// Runs `cmd` with its standard error in `log`.
fn run_logged(cmd: &mut Command, log: &Path) -> Result<(), String> {
    let file = std::fs::File::create(log).map_err(|e| format!("{}: {e}", log.display()))?;
    run(cmd.stderr(file))
}

fn remove(path: &Path) -> Result<(), String> {
    let result = match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(_) => return Ok(()),
    };
    result.map_err(|e| format!("removing {}: {e}", path.display()))
}

/// Where profiles are kept.
fn profiles_dir() -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("PHPSTAN_RS_PROFILE_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
    Ok(PathBuf::from(home).join(".cache/prs-profile"))
}

/// The directory the frame-pointer build of this checkout is made in: one
/// of its own, so the normal build is not disturbed and each worktree keeps
/// its own.
fn build_dir(profiles: &Path) -> PathBuf {
    let checkout = root()
        .file_name()
        .and_then(OsStr::to_str)
        .and_then(|name| name.strip_prefix("phpstan-rs-"))
        .unwrap_or("main")
        .to_string();
    profiles.join(format!("fp-{checkout}"))
}

/// `nice -n 19` before `program`, as every heavy job runs.
fn niced(program: impl AsRef<OsStr>) -> Command {
    let mut cmd = Command::new("nice");
    cmd.args(["-n", "19"]).arg(program);
    cmd
}

/// A profile's directory name: the run, then the commit.
fn profile_name(kind: Kind, project: &str, hash: &str) -> String {
    format!(
        "{}-{}-{}",
        kind.name(),
        project.replace('/', "-"),
        &hash[..8]
    )
}

pub(crate) fn profile_command(args: &[String]) -> Result<(), String> {
    let mut words = args.iter();
    let kind = Kind::parse(words.next().ok_or("profile takes a kind")?)?;
    let project = words.next().map_or(DEFAULT_PROJECT, String::as_str);
    if let Some(extra) = words.next() {
        return Err(format!("unexpected argument {extra:?}"));
    }
    if !vendor_projects()?.iter().any(|known| known == project) {
        return Err(format!(
            "{project} is not a vendor project of {}",
            corpus_dir().display()
        ));
    }
    require_clean_tree()?;
    let hash = head_hash()?;
    let profiles = profiles_dir()?;
    let dir = profiles.join(profile_name(kind, project, &hash));
    if dir.exists() {
        return Err(format!(
            "{} exists; remove it to profile this commit again",
            dir.display()
        ));
    }

    let target = build_dir(&profiles);
    let build = [
        "build",
        "--profile",
        "dhat",
        "-p",
        "phpstan-rs",
        "--bin",
        "phpstan-rs",
        "-p",
        "phpstan-rs-gate-runner",
        "--bin",
        "gate-runner",
    ];
    eprintln!("[profile] building {} in {}", &hash[..8], target.display());
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    run(niced(cargo)
        .current_dir(root())
        .args(build)
        .env("RUSTFLAGS", "-C force-frame-pointers=yes")
        .env("CARGO_TARGET_DIR", &target))?;

    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for binary in ["phpstan-rs", "gate-runner"] {
        let from = target.join("dhat").join(binary);
        std::fs::copy(&from, dir.join(binary))
            .map_err(|e| format!("copying {}: {e}", from.display()))?;
    }

    let work = Workload {
        kind,
        project,
        dir: &dir,
    };
    let cache = dir.join("cache");

    eprintln!("[profile] cold run, to build the stub cache");
    let out = dir.join("out");
    let mut cmd = work.prepare(
        &[OsStr::new("nice"), OsStr::new("-n"), OsStr::new("19")],
        &out,
    )?;
    run_logged(&mut cmd, &dir.join("cold.err"))?;
    work.finish()?;
    remove(&out)?;
    let stored = cache_files(&cache)?;
    if stored.is_empty() {
        return Err(format!(
            "the cold run wrote nothing to {}; the stub cache is off",
            cache.display()
        ));
    }

    eprintln!("[profile] warm run");
    let timed = dir.join("warm.time");
    let load_warm = loadavg();
    let mut cmd = work.prepare(
        &[
            OsStr::new("nice"),
            OsStr::new("-n"),
            OsStr::new("19"),
            OsStr::new("/usr/bin/time"),
            OsStr::new("-f"),
            OsStr::new("%e s %M KB"),
            OsStr::new("-o"),
            timed.as_os_str(),
        ],
        &out,
    )?;
    run_logged(&mut cmd, &dir.join("warm.err"))?;
    work.finish()?;
    remove(&out)?;
    if cache_files(&cache)? != stored {
        return Err(format!(
            "the warm run rewrote {}; the profile would measure a cold cache",
            cache.display()
        ));
    }
    let warm = read_time(&timed)?;

    eprintln!("[profile] callgrind");
    let callgrind_out = dir.join("cg.out");
    let load_callgrind = loadavg();
    let mut cmd = work.prepare(
        &[
            OsStr::new("nice"),
            OsStr::new("-n"),
            OsStr::new("19"),
            OsStr::new("valgrind"),
            OsStr::new("--tool=callgrind"),
            &out_arg("--callgrind-out-file=", &callgrind_out),
            OsStr::new("--separate-threads=yes"),
        ],
        &out,
    )?;
    run_logged(&mut cmd, &dir.join("callgrind.log"))?;
    work.finish()?;

    let threads = thread_outputs(&dir)?;
    let mut totals = Vec::new();
    for (name, path) in &threads {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        totals.push((name.clone(), callgrind_total(&text)?));
    }
    let mut tsv = String::new();
    for (name, total) in &totals {
        writeln!(tsv, "{name}\t{total}").unwrap();
    }
    write(&dir.join("totals.tsv"), tsv.as_bytes())?;
    for (name, path) in &threads {
        let annotated = capture(Command::new("callgrind_annotate").args([
            OsStr::new("--inclusive=yes"),
            OsStr::new("--tree=none"),
            path.as_os_str(),
        ]))?;
        write(&dir.join(format!("ann-{name}.txt")), annotated.as_bytes())?;
    }

    let readme = readme(&Report {
        kind,
        project,
        hash: &hash,
        target: &target,
        subject: git(&["log", "-1", "--format=%s", "HEAD"])?,
        warm,
        load_warm,
        load_callgrind,
        totals: &totals,
    });
    write(&dir.join("README"), readme.as_bytes())?;
    eprintln!("[profile] {}", dir.display());
    Ok(())
}

fn out_arg(prefix: &str, path: &Path) -> std::ffi::OsString {
    let mut arg = std::ffi::OsString::from(prefix);
    arg.push(path);
    arg
}

/// The files of a stub cache directory with their sizes and times, in name
/// order.
fn cache_files(dir: &Path) -> Result<Vec<(String, u64, std::time::SystemTime)>, String> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let meta = entry.metadata().map_err(|e| e.to_string())?;
        let modified = meta.modified().map_err(|e| e.to_string())?;
        files.push((
            entry.file_name().to_string_lossy().into_owned(),
            meta.len(),
            modified,
        ));
    }
    files.sort();
    Ok(files)
}

/// `/proc/loadavg`'s first three numbers, or `unknown`.
fn loadavg() -> String {
    std::fs::read_to_string("/proc/loadavg")
        .ok()
        .map(|text| {
            text.split_whitespace()
                .take(3)
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_else(|| "unknown".into())
}

/// A warm run's wall time in seconds and peak resident size in KB, from
/// `/usr/bin/time -f '%e s %M KB'`.
fn parse_time(text: &str) -> Option<(f64, u64)> {
    let line = text.lines().last()?;
    let mut fields = line.split_whitespace();
    let seconds = fields.next()?.parse().ok()?;
    (fields.next()? == "s").then_some(())?;
    let kilobytes = fields.next()?.parse().ok()?;
    (fields.next()? == "KB").then_some(())?;
    Some((seconds, kilobytes))
}

fn read_time(path: &Path) -> Result<(f64, u64), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse_time(&text).ok_or_else(|| format!("{}: not a time: {text:?}", path.display()))
}

/// The instructions a callgrind output file counts.
fn callgrind_total(text: &str) -> Result<u64, String> {
    text.lines()
        .rev()
        .find_map(|line| line.strip_prefix("totals:"))
        .and_then(|total| total.trim().parse().ok())
        .ok_or_else(|| "a callgrind output without its totals".to_string())
}

/// One output per thread, `cg.out-NN`, in thread order, named by `NN`.
fn thread_outputs(dir: &Path) -> Result<Vec<(String, PathBuf)>, String> {
    let mut threads = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        if let Some(number) = entry.file_name().to_string_lossy().strip_prefix("cg.out-") {
            threads.push((number.to_string(), entry.path()));
        }
    }
    threads.sort();
    if threads.is_empty() {
        return Err(format!("callgrind wrote no output in {}", dir.display()));
    }
    Ok(threads)
}

/// A million instructions as `M`, a billion as `G`.
fn instructions(count: u64) -> String {
    if count >= 1_000_000_000 {
        format!("{:.2} G", count as f64 / 1e9)
    } else {
        format!("{:.0} M", count as f64 / 1e6)
    }
}

struct Report<'a> {
    kind: Kind,
    project: &'a str,
    hash: &'a str,
    target: &'a Path,
    subject: String,
    warm: (f64, u64),
    load_warm: String,
    load_callgrind: String,
    totals: &'a [(String, u64)],
}

fn readme(report: &Report) -> String {
    let (seconds, kilobytes) = report.warm;
    let total: u64 = report.totals.iter().map(|(_, count)| count).sum();
    let mut text = String::new();
    let what = match report.kind {
        Kind::Vendor => format!(
            "gate-runner over {} in the corpus, as the corpus pass runs a vendor project",
            report.project
        ),
        Kind::Replay => format!(
            "check-project --edit-replay over a fresh copy of {}, as the corpus pass runs a replay",
            report.project
        ),
    };
    writeln!(text, "Profile of {what}: every rule set on, no Composer context, PHPSTAN_RS_NO_FILE_CACHE=1, a stub cache of its own warmed by a first run.\n").unwrap();
    writeln!(
        text,
        "commit                   {} ({})",
        report.hash, report.subject
    )
    .unwrap();
    writeln!(text, "phpstan-rs, gate-runner  built from it with RUSTFLAGS=\"-C force-frame-pointers=yes\" CARGO_TARGET_DIR={} cargo build --profile dhat -p phpstan-rs --bin phpstan-rs -p phpstan-rs-gate-runner --bin gate-runner", report.target.display()).unwrap();
    writeln!(text, "cache/                   the stub cache the runs share; the warm run left it as the cold run wrote it").unwrap();
    writeln!(text, "cold.err, warm.err       the runs' standard error").unwrap();
    writeln!(
        text,
        "warm.time                /usr/bin/time -f '%e s %M KB' of the warm run"
    )
    .unwrap();
    writeln!(text, "cg.out-NN                valgrind --tool=callgrind --separate-threads=yes of the same command; NN is the thread, 01 the main one").unwrap();
    writeln!(
        text,
        "ann-NN.txt               callgrind_annotate --inclusive=yes --tree=none of cg.out-NN"
    )
    .unwrap();
    writeln!(text, "totals.tsv               each thread's instructions").unwrap();
    writeln!(
        text,
        "out/                     the callgrind run's output\n"
    )
    .unwrap();
    writeln!(
        text,
        "Warm run: {seconds} s, {kilobytes} KB peak (loadavg {}).",
        report.load_warm
    )
    .unwrap();
    writeln!(
        text,
        "Callgrind (loadavg {}): {} instructions over {} threads.",
        report.load_callgrind,
        instructions(total),
        report.totals.len()
    )
    .unwrap();
    for (name, count) in report.totals {
        writeln!(text, "  thread {name}: {}", instructions(*count)).unwrap();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kind_is_parsed_by_its_name() {
        assert_eq!(Kind::parse("vendor"), Ok(Kind::Vendor));
        assert_eq!(Kind::parse("replay"), Ok(Kind::Replay));
        assert!(Kind::parse("nothing").is_err());
    }

    #[test]
    fn a_profile_is_named_by_its_run_and_commit() {
        assert_eq!(
            profile_name(Kind::Vendor, "vendor/amphp/amp", "dc4da9a2f3b1"),
            "vendor-vendor-amphp-amp-dc4da9a2"
        );
    }

    #[test]
    fn a_time_line_gives_seconds_and_peak_size() {
        assert_eq!(parse_time("0.53 s 335812 KB\n"), Some((0.53, 335_812)));
        assert_eq!(
            parse_time("Command exited with non-zero status 1\n0.53 s 335812 KB\n"),
            Some((0.53, 335_812))
        );
        assert_eq!(parse_time("0.53 s"), None);
    }

    #[test]
    fn a_callgrind_output_ends_with_its_totals() {
        assert_eq!(
            callgrind_total("summary: 299917626\nfn=1\n2 5\ntotals: 299917626\n"),
            Ok(299_917_626)
        );
        assert!(callgrind_total("summary: 1\n").is_err());
    }

    #[test]
    fn instructions_are_shown_in_millions_and_billions() {
        assert_eq!(instructions(392_149_900), "392 M");
        assert_eq!(instructions(1_981_881_393), "1.98 G");
    }
}
