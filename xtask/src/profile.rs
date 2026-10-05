//! Profiles of the runs the corpus pass makes (`cargo xtask profile <kind>
//! [<subject>] [--threads <n>] [--file-cache]`): the frame-pointer `dhat`
//! build of HEAD, a stub cache of their own warmed by a cold run, a warm run
//! timed with its peak memory, and callgrind of the same run with one output
//! per thread, annotated, with a `README` that says how every file was made.
//! A profile is a directory of `~/.cache/prs-profile` (or of
//! `PHPSTAN_RS_PROFILE_DIR`), named by the run and the commit, that later
//! measurements take their runs from, the same way.
//!
//! - **`vendor [<project>]`**: the gate runner over one vendor project of
//!   the corpus, every rule set on, with no Composer context, as the pass
//!   runs it.
//! - **`replay [<project>]`**: `check-project --edit-replay` over a fresh
//!   copy of the same project, with the same sets, as the pass runs it.
//! - **`config [<name>]`**: the gate runner over a configuration of the
//!   corpus (`scoped`, `all-on`, `unscoped`, `vendor-tree` or `arch`) with
//!   every artifact the full pass asks of it: the views, every level's
//!   report from the one analysis, the dumps and the stub bases. The cold
//!   run is the pass's first state and writes whole; the warm and the
//!   callgrind runs are a later state, written against the cold run's
//!   dumps, so they cost what a state after a snapshot costs.
//! - **`session [<project>]`**: `record-session` over the corpus (`.`) or a
//!   vendor project, the warm and the callgrind runs written against the
//!   cold run's record.
//! - **`fixtures [<filter>]`**: the fixture suite's binary, with each
//!   fixture's records on, or the fixtures a filter names.
//!
//! `--threads <n>` sizes the worker pool, as the pass's one-thread state
//! does, and `--file-cache` leaves the per-file cache on, so the cold run
//! writes it and the warm and callgrind runs read it.

use std::ffi::{OsStr, OsString};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use super::{
    ALL_ON, ALL_ON_SETS, ARCH, Config, SCOPED, UNSCOPED, VENDOR_TREE, baseline_file, capture,
    corpus_dir, git, head_hash, require_clean_tree, root, run, vendor_projects, write,
};

/// The project a kind profiles when none is named: 26 files.
const DEFAULT_PROJECT: &str = "vendor/amphp/amp";

/// The project a session profiles when none is named.
const DEFAULT_SESSION_PROJECT: &str = "vendor/symfony/console";

/// The configurations a profile can take.
const CONFIGS: [Config; 5] = [SCOPED, ALL_ON, UNSCOPED, VENDOR_TREE, ARCH];

/// The kinds of run a profile can take.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Vendor,
    Replay,
    Config,
    Session,
    Fixtures,
}

impl Kind {
    const ALL: [Kind; 5] = [
        Kind::Vendor,
        Kind::Replay,
        Kind::Config,
        Kind::Session,
        Kind::Fixtures,
    ];

    fn name(self) -> &'static str {
        match self {
            Kind::Vendor => "vendor",
            Kind::Replay => "replay",
            Kind::Config => "config",
            Kind::Session => "session",
            Kind::Fixtures => "fixtures",
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

    /// What the kind profiles when its argument is left out.
    fn default_subject(self) -> Option<&'static str> {
        match self {
            Kind::Vendor | Kind::Replay => Some(DEFAULT_PROJECT),
            Kind::Config => Some(SCOPED.name),
            Kind::Session => Some(DEFAULT_SESSION_PROJECT),
            Kind::Fixtures => None,
        }
    }

    /// The binary of the profile's directory the run is.
    fn binary(self) -> &'static str {
        match self {
            Kind::Vendor | Kind::Config => "gate-runner",
            Kind::Replay | Kind::Session => "phpstan-rs",
            Kind::Fixtures => "fixtures",
        }
    }

    /// Whether the later runs are written against the cold run's output.
    fn writes_against_cold(self) -> bool {
        matches!(self, Kind::Config | Kind::Session)
    }

    /// Whether the callgrind run's output stays in the profile: small ones
    /// do, a corpus's dumps do not.
    fn keeps_output(self) -> bool {
        matches!(self, Kind::Vendor | Kind::Replay)
    }
}

/// What a profile's runs vary.
struct Options {
    threads: Option<String>,
    file_cache: bool,
}

/// The runs of a profile, in order.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Cold,
    Warm,
    Callgrind,
}

/// The runs of a kind over its subject, in the profile's directory.
struct Workload<'a> {
    kind: Kind,
    subject: Option<&'a str>,
    options: &'a Options,
    dir: &'a Path,
}

impl Workload<'_> {
    /// The command of `step` behind `wrapper`, its output new.
    fn prepare(&self, wrapper: &[&OsStr], step: Step) -> Result<Command, String> {
        let out = self.out(step);
        remove(&out)?;
        let reference = self.reference(step)?;
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
            .env_remove("PHPSTAN_RS_NO_STUB_CACHE")
            .stdout(Stdio::null());
        if self.options.file_cache {
            cmd.env_remove("PHPSTAN_RS_NO_FILE_CACHE");
        } else {
            cmd.env("PHPSTAN_RS_NO_FILE_CACHE", "1");
        }
        match &self.options.threads {
            Some(threads) => cmd.env("PHPSTAN_RS_THREADS", threads),
            None => cmd.env_remove("PHPSTAN_RS_THREADS"),
        };
        let subject = self.subject.unwrap_or_default();
        match self.kind {
            Kind::Vendor => {
                cmd.current_dir(corpus_dir()).arg("--no-composer");
                set_all_on(&mut cmd);
                cmd.arg("--out").arg(&out).arg(subject);
            }
            Kind::Replay => {
                let copy = self.copy();
                remove(&copy)?;
                run(Command::new("cp")
                    .arg("-a")
                    .arg(corpus_dir().join(subject))
                    .arg(&copy))?;
                write(&copy.join(".phpstan-rs-replay"), b"")?;
                cmd.current_dir(&copy)
                    .args(["check-project", "--no-composer"]);
                set_all_on(&mut cmd);
                cmd.arg("--edit-replay").arg(&out).arg(".");
            }
            Kind::Config => {
                let config = config_named(subject)?;
                cmd.current_dir(corpus_dir())
                    .args((config.args)())
                    .arg("--out")
                    .arg(&out);
                if let Some(reference) = reference {
                    cmd.arg("--reference").arg(reference);
                }
                let views = matches!(config.name, "scoped" | "all-on");
                if views {
                    cmd.arg("--dump-types");
                    let baseline = baseline_file(config.name);
                    if baseline.exists() {
                        cmd.arg("--baseline").arg(baseline);
                    }
                    cmd.arg("--levels");
                }
                cmd.args([
                    "--dump-registry",
                    "--dump-expr-types",
                    "--dump-doc-resolutions",
                ]);
                if step == Step::Cold || self.options.threads.is_some() {
                    cmd.arg("--dump-front-end");
                }
                if config.name == "scoped" {
                    cmd.arg("--dump-stub-bases");
                }
            }
            Kind::Session => {
                cmd.current_dir(corpus_dir())
                    .arg("record-session")
                    .arg(subject)
                    .arg(&out);
                if let Some(reference) = reference {
                    cmd.arg("--reference").arg(dumps::manifest_path(&reference));
                }
            }
            Kind::Fixtures => {
                std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
                cmd.current_dir(root().join("crates/phpstan-rs"))
                    .arg("--quiet")
                    .args(self.subject)
                    .env("PHPSTAN_RS_DUMP_TYPES", &out)
                    .env("PHPSTAN_RS_DUMP_DIAGNOSTICS", &out);
            }
        }
        Ok(cmd)
    }

    /// Removes what a run left in the profile's directory beside its
    /// output: a replay's tree, and for the kinds that write against the
    /// cold run's output, that output once nothing needs it.
    fn finish(&self, step: Step) -> Result<(), String> {
        remove(&self.copy())?;
        match step {
            Step::Cold if !self.kind.writes_against_cold() => remove(&self.out(step)),
            Step::Warm => remove(&self.out(step)),
            Step::Callgrind => {
                if self.kind.writes_against_cold() {
                    remove(&self.dir.join("reference"))?;
                    remove(&self.dir.join("no-reference"))?;
                }
                if self.kind.keeps_output() {
                    Ok(())
                } else {
                    remove(&self.out(step))
                }
            }
            Step::Cold => Ok(()),
        }
    }

    /// Where `step` writes: the cold run of a kind the later runs are
    /// written against writes the reference.
    fn out(&self, step: Step) -> PathBuf {
        if step == Step::Cold && self.kind.writes_against_cold() {
            self.dir.join("reference")
        } else {
            self.dir.join("out")
        }
    }

    /// What `step`'s output is written against: for a configuration the
    /// cold run's, which has nothing before it, so an empty directory.
    fn reference(&self, step: Step) -> Result<Option<PathBuf>, String> {
        match (self.kind, step) {
            (Kind::Config, Step::Cold) => {
                let empty = self.dir.join("no-reference");
                std::fs::create_dir_all(&empty).map_err(|e| format!("{}: {e}", empty.display()))?;
                Ok(Some(empty))
            }
            (Kind::Config | Kind::Session, Step::Warm | Step::Callgrind) => {
                Ok(Some(self.dir.join("reference")))
            }
            _ => Ok(None),
        }
    }

    /// The tree a replay edits.
    fn copy(&self) -> PathBuf {
        self.dir.join("copy")
    }

    /// What the runs are, for the README.
    fn describe(&self) -> String {
        let subject = self.subject.unwrap_or("the whole suite");
        let mut text = match self.kind {
            Kind::Vendor => format!(
                "gate-runner over {subject} in the corpus, as the corpus pass runs a vendor project: every rule set on, no Composer context"
            ),
            Kind::Replay => format!(
                "check-project --edit-replay over a fresh copy of {subject}, as the corpus pass runs a replay: every rule set on, no Composer context"
            ),
            Kind::Config => format!(
                "gate-runner over the {subject} configuration of the corpus with the artifacts the full pass asks of it; the cold run is the pass's first state, the warm and callgrind runs a later one, written against the cold run's dumps"
            ),
            Kind::Session => format!(
                "record-session over {subject} in the corpus; the warm and callgrind runs are written against the cold run's record"
            ),
            Kind::Fixtures => format!(
                "the fixture suite's binary ({subject}) with each fixture's types and diagnostics written"
            ),
        };
        let file_cache = if self.options.file_cache {
            "the per-file cache on, written by the cold run and read by the others"
        } else {
            "PHPSTAN_RS_NO_FILE_CACHE=1"
        };
        write!(text, ", {file_cache}").unwrap();
        if let Some(threads) = &self.options.threads {
            write!(text, ", PHPSTAN_RS_THREADS={threads}").unwrap();
        }
        text
    }
}

fn config_named(name: &str) -> Result<Config, String> {
    CONFIGS
        .into_iter()
        .find(|config| config.name == name)
        .ok_or_else(|| {
            let names: Vec<&str> = CONFIGS.iter().map(|config| config.name).collect();
            format!(
                "{name:?} is not a configuration; they are {}",
                names.join(", ")
            )
        })
}

fn set_all_on(cmd: &mut Command) {
    for set in ALL_ON_SETS {
        cmd.arg("--set").arg(set);
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

/// A profile's directory name: the run, its subject and options, then the
/// commit.
fn profile_name(kind: Kind, subject: Option<&str>, options: &Options, hash: &str) -> String {
    let mut name = kind.name().to_string();
    if let Some(subject) = subject {
        let slug = match subject {
            "." => "corpus".to_string(),
            other => other.replace('/', "-"),
        };
        write!(name, "-{slug}").unwrap();
    }
    if let Some(threads) = &options.threads {
        write!(name, "-t{threads}").unwrap();
    }
    if options.file_cache {
        name.push_str("-fc");
    }
    write!(name, "-{}", &hash[..8]).unwrap();
    name
}

/// The kind, its subject and the options a command line gives.
fn parse_arguments(args: &[String]) -> Result<(Kind, Option<String>, Options), String> {
    let mut words = Vec::new();
    let mut options = Options {
        threads: None,
        file_cache: false,
    };
    let mut args = args.iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--file-cache" => options.file_cache = true,
            "--threads" => {
                let count = args.next().ok_or("--threads takes a count")?;
                if count.parse::<usize>().is_ok_and(|count| count > 0) {
                    options.threads = Some(count.clone());
                } else {
                    return Err(format!("--threads takes a count, not {count:?}"));
                }
            }
            flag if flag.starts_with("--") => return Err(format!("unknown option {flag}")),
            word => words.push(word.to_string()),
        }
    }
    let mut words = words.into_iter();
    let kind = Kind::parse(&words.next().ok_or("profile takes a kind")?)?;
    let subject = words.next();
    if let Some(extra) = words.next() {
        return Err(format!("unexpected argument {extra:?}"));
    }
    Ok((kind, subject, options))
}

/// Checks that `subject` is one the kind can run over, and fills in its
/// default.
fn resolve_subject(kind: Kind, subject: Option<String>) -> Result<Option<String>, String> {
    let subject = subject.or_else(|| kind.default_subject().map(str::to_string));
    match (kind, &subject) {
        (Kind::Config, Some(name)) => {
            config_named(name)?;
        }
        (Kind::Fixtures, _) => {}
        (Kind::Session, Some(project)) if project == "." => {}
        (_, Some(project)) => {
            if !vendor_projects()?.iter().any(|known| known == project) {
                return Err(format!(
                    "{project} is not a vendor project of {}",
                    corpus_dir().display()
                ));
            }
        }
        (_, None) => unreachable!("every kind but the fixtures has a default"),
    }
    Ok(subject)
}

/// Builds the binaries of `kind` with frame pointers into `target` and
/// copies them to `dir`.
fn build(kind: Kind, target: &Path, dir: &Path) -> Result<(), String> {
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut cmd = niced(cargo);
    cmd.current_dir(root())
        .env("RUSTFLAGS", "-C force-frame-pointers=yes")
        .env("CARGO_TARGET_DIR", target);
    if kind == Kind::Fixtures {
        let output = capture(cmd.args([
            "test",
            "-p",
            "phpstan-rs",
            "--profile",
            "dhat",
            "--test",
            "fixtures",
            "--no-run",
            "--message-format=json",
        ]))?;
        let executable = test_executable(&output, "fixtures")
            .ok_or("cargo built no executable for the fixtures")?;
        std::fs::copy(&executable, dir.join("fixtures"))
            .map_err(|e| format!("copying {executable}: {e}"))?;
    } else {
        run(cmd.args([
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
        ]))?;
        for binary in ["phpstan-rs", "gate-runner"] {
            let from = target.join("dhat").join(binary);
            std::fs::copy(&from, dir.join(binary))
                .map_err(|e| format!("copying {}: {e}", from.display()))?;
        }
    }
    Ok(())
}

/// The executable cargo's JSON messages name for the test target `name`.
fn test_executable(messages: &str, name: &str) -> Option<String> {
    messages.lines().find_map(|line| {
        let message: serde_json::Value = serde_json::from_str(line).ok()?;
        (message["reason"] == "compiler-artifact" && message["target"]["name"] == name)
            .then(|| message["executable"].as_str().map(str::to_string))?
    })
}

pub(crate) fn profile_command(args: &[String]) -> Result<(), String> {
    let (kind, subject, options) = parse_arguments(args)?;
    let subject = resolve_subject(kind, subject)?;
    require_clean_tree()?;
    let hash = head_hash()?;
    let profiles = profiles_dir()?;
    let dir = profiles.join(profile_name(kind, subject.as_deref(), &options, &hash));
    if dir.exists() {
        return Err(format!(
            "{} exists; remove it to profile this commit again",
            dir.display()
        ));
    }

    let target = build_dir(&profiles);
    eprintln!("[profile] building {} in {}", &hash[..8], target.display());
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    build(kind, &target, &dir)?;

    let work = Workload {
        kind,
        subject: subject.as_deref(),
        options: &options,
        dir: &dir,
    };
    let cache = dir.join("cache");
    let nice = [OsStr::new("nice"), OsStr::new("-n"), OsStr::new("19")];

    eprintln!("[profile] cold run, to build the stub cache");
    let mut cmd = work.prepare(&nice, Step::Cold)?;
    run_logged(&mut cmd, &dir.join("cold.err"))?;
    work.finish(Step::Cold)?;
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
    let mut wrapper = nice.to_vec();
    wrapper.extend([
        OsStr::new("/usr/bin/time"),
        OsStr::new("-f"),
        OsStr::new("%e s %M KB"),
        OsStr::new("-o"),
        timed.as_os_str(),
    ]);
    let mut cmd = work.prepare(&wrapper, Step::Warm)?;
    run_logged(&mut cmd, &dir.join("warm.err"))?;
    work.finish(Step::Warm)?;
    if cache_files(&cache)? != stored {
        return Err(format!(
            "the warm run rewrote {}; the profile would measure a cold cache",
            cache.display()
        ));
    }
    let warm = read_time(&timed)?;

    eprintln!("[profile] callgrind");
    let load_callgrind = loadavg();
    let callgrind_out = out_arg("--callgrind-out-file=", &dir.join("cg.out"));
    let mut wrapper = nice.to_vec();
    wrapper.extend([
        OsStr::new("valgrind"),
        OsStr::new("--tool=callgrind"),
        callgrind_out.as_os_str(),
        OsStr::new("--separate-threads=yes"),
    ]);
    let mut cmd = work.prepare(&wrapper, Step::Callgrind)?;
    run_logged(&mut cmd, &dir.join("callgrind.log"))?;
    work.finish(Step::Callgrind)?;

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
        what: work.describe(),
        keeps_output: kind.keeps_output(),
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

fn out_arg(prefix: &str, path: &Path) -> OsString {
    let mut arg = OsString::from(prefix);
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
pub(crate) fn loadavg() -> String {
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
    what: String,
    keeps_output: bool,
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
    writeln!(
        text,
        "Profile of {}, with a stub cache of its own warmed by a first run.\n",
        report.what
    )
    .unwrap();
    writeln!(
        text,
        "commit                   {} ({})",
        report.hash, report.subject
    )
    .unwrap();
    writeln!(text, "phpstan-rs, gate-runner, fixtures").unwrap();
    writeln!(text, "                         built from it with RUSTFLAGS=\"-C force-frame-pointers=yes\" CARGO_TARGET_DIR={} cargo build --profile dhat (cargo test --no-run for the fixtures); the one the run needs is here", report.target.display()).unwrap();
    writeln!(text, "cache/                   the stub cache the runs share; the warm run left it as the cold run wrote it").unwrap();
    writeln!(text, "cold.err, warm.err       the runs' standard error").unwrap();
    writeln!(
        text,
        "warm.time                /usr/bin/time -f '%e s %M KB' of the warm run"
    )
    .unwrap();
    writeln!(text, "cg.out-NN                valgrind --tool=callgrind --separate-threads=yes of the same command as the warm run; NN is the thread, 01 the main one").unwrap();
    writeln!(
        text,
        "ann-NN.txt               callgrind_annotate --inclusive=yes --tree=none of cg.out-NN"
    )
    .unwrap();
    writeln!(text, "totals.tsv               each thread's instructions").unwrap();
    if report.keeps_output {
        writeln!(text, "out/                     the callgrind run's output").unwrap();
    } else {
        writeln!(
            text,
            "                         the runs' output is removed: it holds a corpus's dumps"
        )
        .unwrap();
    }
    writeln!(text).unwrap();
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

    fn arguments(words: &[&str]) -> Result<(Kind, Option<String>, Options), String> {
        parse_arguments(&words.iter().map(|w| w.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn a_kind_is_parsed_by_its_name() {
        for kind in Kind::ALL {
            assert_eq!(Kind::parse(kind.name()), Ok(kind));
        }
        assert!(Kind::parse("nothing").is_err());
    }

    #[test]
    fn a_command_line_gives_the_kind_subject_and_options_in_any_order() {
        let (kind, subject, options) =
            arguments(&["--threads", "1", "config", "all-on", "--file-cache"]).unwrap();
        assert_eq!(kind, Kind::Config);
        assert_eq!(subject.as_deref(), Some("all-on"));
        assert_eq!(options.threads.as_deref(), Some("1"));
        assert!(options.file_cache);
        assert!(arguments(&["vendor", "a", "b"]).is_err());
        assert!(arguments(&["vendor", "--threads", "0"]).is_err());
        assert!(arguments(&["vendor", "--nothing"]).is_err());
        assert!(arguments(&[]).is_err());
    }

    #[test]
    fn a_profile_is_named_by_its_run_subject_options_and_commit() {
        let none = Options {
            threads: None,
            file_cache: false,
        };
        assert_eq!(
            profile_name(
                Kind::Vendor,
                Some("vendor/amphp/amp"),
                &none,
                "dc4da9a2f3b1"
            ),
            "vendor-vendor-amphp-amp-dc4da9a2"
        );
        assert_eq!(
            profile_name(Kind::Session, Some("."), &none, "dc4da9a2f3b1"),
            "session-corpus-dc4da9a2"
        );
        let options = Options {
            threads: Some("1".into()),
            file_cache: true,
        };
        assert_eq!(
            profile_name(Kind::Fixtures, None, &options, "dc4da9a2f3b1"),
            "fixtures-t1-fc-dc4da9a2"
        );
    }

    #[test]
    fn a_configuration_is_found_by_its_name() {
        assert!(config_named("all-on").is_ok());
        assert!(config_named("vendor").is_err());
    }

    #[test]
    fn the_fixtures_executable_is_the_one_cargo_names() {
        let messages = concat!(
            "{\"reason\":\"compiler-artifact\",\"target\":{\"name\":\"other\"},\"executable\":\"/t/other\"}\n",
            "{\"reason\":\"compiler-artifact\",\"target\":{\"name\":\"fixtures\"},\"executable\":\"/t/fixtures-1a\"}\n",
            "{\"reason\":\"build-finished\",\"success\":true}\n",
        );
        assert_eq!(
            test_executable(messages, "fixtures").as_deref(),
            Some("/t/fixtures-1a")
        );
        assert_eq!(test_executable(messages, "missing"), None);
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
