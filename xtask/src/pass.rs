//! The corpus pass (`cargo xtask corpus`): every run the gates and item 0's
//! dumps take of the corpus, each configuration analysed once per state
//! with every artifact written by that one analysis, on a worker per core
//! from one queue.
//!
//! - **Configurations.** The runner analyses a configuration once per state
//!   and writes the report, its views and, in the full pass, the dumps. A
//!   state's artifacts must equal the first state's, all of them. The
//!   scoped, all-on and unscoped configurations run in the equalities'
//!   states (`EQUALITY_STATES`), the vendor tree and the architecture
//!   configuration at the default thread count and at one
//!   (`THREAD_STATES`). The front end's dump is taken in the states that
//!   change the thread count, and the stub bases in the scoped
//!   configuration's first two, cold and then read back.
//! - **Levels.** Every finding carries the levels it is made at, so each
//!   level's report is a cut of one analysis: the scoped and all-on
//!   configurations' runs in the full pass write every level's report
//!   beside their own, compared across states like their other artifacts.
//! - **The server's sessions and the edit replays** run at the default
//!   thread count and at one, a replay's two runs one after the other in a
//!   fresh copy of its tree, which the dumps print.
//! - **The vendor projects** run alone, every rule set on, and the fixture
//!   suite with its records.
//!
//! The first state's artifacts are recorded by commit in the store
//! (`store`), each added to the pass's tree once its runs are done, and the
//! tree becomes the commit's snapshot once every run has passed; a pass
//! with another build records nothing.
//!
//! A dump is written only where it differs from another run's: a
//! configuration's first state, and a session's default run, from the
//! nearest snapshot that has the dump, every later state, and the one-thread
//! run, from the first's. Two runs are compared by their dumps' manifests,
//! and an entity's texts are read only where its ids differ.

use std::collections::VecDeque;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Condvar, Mutex, PoisonError};
use std::time::Instant;

use dumps::{Dump, manifest_path};

use super::progress::{Group, Progress};
use super::store::{Content, Snapshots, Store, dump_kind, pass_ref, snapshot_ref, text_content};
use super::{
    ALL_ON, ALL_ON_SETS, ARCH, Config, LEVELS, SCOPED, SESSION_PROJECTS, Timings, UNSCOPED,
    VENDOR_TREE, artifact_name, baseline_file, cargo, corpus_dir, fixture_entries, gate_dir, git,
    normalize, root, run, shown, unified_diff, vendor_projects, write,
};

/// How many runs over the corpus take place at once: each holds the
/// project and its dumps, a few gigabytes, so more would exhaust memory.
const LARGE_AT_ONCE: usize = 3;

/// What a pass runs.
pub(crate) struct PassSetup {
    /// The full pass, where the quick one runs only the scoped and all-on
    /// reports.
    pub(crate) full: bool,
    /// Whether the fixture suite runs in the pass.
    pub(crate) fixtures: bool,
    /// The cargo profile of `phpstan-rs` and the runner: `gate`, or
    /// `oracle`, which keeps the per-file cache off.
    pub(crate) profile: &'static str,
    /// The commit the first states' artifacts are recorded under, when
    /// they are.
    pub(crate) hash: Option<String>,
}

/// A state a configuration is analysed in: a cache directory of its own
/// (`None` for the user's), the per-file cache on or off, and a thread
/// count (`None` for the default).
#[derive(Clone, Copy)]
struct State {
    name: &'static str,
    cache: Option<&'static str>,
    file_cache: bool,
    threads: Option<&'static str>,
}

/// The equalities' states, in order: two share a cache directory so that
/// the second reads what the first wrote.
const EQUALITY_STATES: [State; 5] = [
    State {
        name: "stub cache cold, file cache off",
        cache: Some("a"),
        file_cache: false,
        threads: None,
    },
    State {
        name: "stub cache warm, file cache off",
        cache: Some("a"),
        file_cache: false,
        threads: None,
    },
    State {
        name: "file cache written",
        cache: Some("b"),
        file_cache: true,
        threads: None,
    },
    State {
        name: "file cache read",
        cache: Some("b"),
        file_cache: true,
        threads: None,
    },
    State {
        name: "one thread",
        cache: Some("b"),
        file_cache: true,
        threads: Some("1"),
    },
];

/// The default thread count and one, the stub cache cold and then warm.
const THREAD_STATES: [State; 2] = [
    State {
        name: "default threads",
        cache: Some("a"),
        file_cache: false,
        threads: None,
    },
    State {
        name: "one thread",
        cache: Some("a"),
        file_cache: false,
        threads: Some("1"),
    },
];

/// The quick tier's one state: the user's caches, as a gate run finds them.
const QUICK_STATE: [State; 1] = [State {
    name: "default caches",
    cache: None,
    file_cache: true,
    threads: None,
}];

/// The thread counts a session or a replay is taken at.
const RUN_THREADS: [Option<&str>; 2] = [None, Some("1")];

/// One worker's task.
enum Task {
    /// A configuration in each of `states`, with the type and baseline
    /// views, every level's report, the dumps, the stub bases and the
    /// check of the shipped binary's emission order as asked.
    Config {
        config: Config,
        states: &'static [State],
        views: bool,
        levels: bool,
        dumps: bool,
        stub_bases: bool,
        determinism: bool,
    },
    /// The server's session over `project`, at each thread count.
    Session {
        name: String,
        project: String,
    },
    /// The edit replay in a copy of `from`, at each thread count.
    Replay {
        slug: String,
        from: PathBuf,
        args: Vec<String>,
    },
    VendorProject {
        project: String,
    },
    Fixtures,
}

impl Task {
    /// Whether it runs over the corpus, which takes a few gigabytes.
    fn large(&self) -> bool {
        match self {
            Task::Config { .. } | Task::Fixtures => true,
            Task::Session { project, .. } => project == ".",
            Task::Replay { slug, .. } => slug == "piwigo",
            Task::VendorProject { .. } => false,
        }
    }

    /// How many runs it makes.
    fn runs(&self) -> usize {
        match self {
            Task::Config {
                states,
                determinism,
                ..
            } => states.len() + usize::from(*determinism),
            Task::Session { .. } | Task::Replay { .. } => RUN_THREADS.len(),
            Task::VendorProject { .. } | Task::Fixtures => 1,
        }
    }

    /// The name of its progress group, and for a configuration its own row.
    fn group(&self) -> (&'static str, Option<&'static str>) {
        match self {
            Task::Config { config, .. } => ("configurations", Some(config.name)),
            Task::Session { .. } => ("sessions", None),
            Task::Replay { .. } => ("replays", None),
            Task::VendorProject { .. } => ("vendor projects", None),
            Task::Fixtures => ("fixtures", None),
        }
    }
}

/// The tasks of a pass, the longest first: the replay of the corpus, whose
/// two runs go one after the other, then the corpus's other runs.
fn tasks(setup: &PassSetup) -> Result<Vec<Task>, String> {
    let mut tasks = Vec::new();
    if setup.full {
        tasks.push(Task::Replay {
            slug: "piwigo".into(),
            from: corpus_dir(),
            args: (SCOPED.args)(),
        });
    }
    let configs: Vec<(Config, &'static [State])> = if setup.full {
        vec![
            (SCOPED, &EQUALITY_STATES[..]),
            (ALL_ON, &EQUALITY_STATES[..]),
            (UNSCOPED, &EQUALITY_STATES[..]),
            (VENDOR_TREE, &THREAD_STATES[..]),
            (ARCH, &THREAD_STATES[..]),
        ]
    } else {
        vec![(SCOPED, &QUICK_STATE[..]), (ALL_ON, &QUICK_STATE[..])]
    };
    for (config, states) in configs {
        let views = matches!(config.name, "scoped" | "all-on");
        tasks.push(Task::Config {
            config,
            states,
            views,
            levels: views && setup.full,
            dumps: setup.full,
            stub_bases: setup.full && config.name == "scoped",
            determinism: config.name == "all-on" || (setup.full && config.name == "unscoped"),
        });
    }
    if setup.full {
        tasks.push(Task::Session {
            name: "piwigo".into(),
            project: ".".into(),
        });
    }
    if setup.fixtures {
        tasks.push(Task::Fixtures);
    }
    if setup.full {
        for project in SESSION_PROJECTS {
            tasks.push(Task::Session {
                name: project.replace('/', "-"),
                project: project.to_string(),
            });
        }
        for project in vendor_projects()? {
            let mut args = vec!["--no-composer".to_string()];
            for set in ALL_ON_SETS {
                args.push("--set".into());
                args.push((*set).into());
            }
            args.push(".".into());
            tasks.push(Task::Replay {
                slug: project.replace('/', "-"),
                from: corpus_dir().join(&project),
                args,
            });
            tasks.push(Task::VendorProject { project });
        }
    }
    Ok(tasks)
}

/// What the workers share.
struct Pass<'a> {
    setup: &'a PassSetup,
    binary: PathBuf,
    runner: PathBuf,
    scratch: PathBuf,
    queue: Mutex<Queue>,
    ready: Condvar,
    timings: Mutex<&'a mut Timings>,
    progress: Mutex<Progress>,
    // Each task's progress groups.
    groups: Vec<Vec<usize>>,
    // The equalities kept, by configuration, in the order of the tasks.
    equalities: Mutex<Vec<(usize, String)>>,
    failures: Mutex<Vec<String>>,
    store: Mutex<Store>,
    // Each snapshot's commit, with its entries.
    snapshots: Snapshots,
    // HEAD and its ancestors, nearest first.
    ancestors: Vec<String>,
}

/// The tasks not yet started, and how many over the corpus are under way.
struct Queue {
    tasks: VecDeque<(usize, Task)>,
    large_running: usize,
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Runs the pass `setup` describes: builds `phpstan-rs` and the runner in
/// its profile, runs every task on a worker per core, records what the
/// setup records, and reports every failure once all have run.
pub(crate) fn run_pass(setup: &PassSetup, timings: &mut Timings) -> Result<(), String> {
    let profile = setup.profile;
    timings.time(&format!("{profile} build"), |_| {
        run(cargo().args([
            "build",
            "--profile",
            profile,
            "-p",
            "phpstan-rs",
            "--bin",
            "phpstan-rs",
            "-p",
            "phpstan-rs-gate-runner",
            "--bin",
            "gate-runner",
        ]))
    })?;
    let scratch = gate_dir().join(".pass");
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).map_err(|e| e.to_string())?;
    let store = Store::open()?;
    if let Some(hash) = &setup.hash {
        store.begin_pass(hash)?;
    }
    let snapshots = store.snapshots()?;
    let ancestors = git(&["rev-list", "HEAD"])?
        .lines()
        .map(str::to_string)
        .collect();
    let tasks = tasks(setup)?;
    let (groups, of) = progress_groups(&tasks);
    let pass = Pass {
        setup,
        binary: root().join(format!("target/{profile}/phpstan-rs")),
        runner: root().join(format!("target/{profile}/gate-runner")),
        scratch,
        queue: Mutex::new(Queue {
            tasks: tasks.into_iter().enumerate().collect(),
            large_running: 0,
        }),
        ready: Condvar::new(),
        timings: Mutex::new(timings),
        progress: Mutex::new(Progress::new(
            super::progress::status_path(&gate_dir(), "corpus"),
            "corpus",
            groups,
        )),
        groups: of,
        equalities: Mutex::new(Vec::new()),
        failures: Mutex::new(Vec::new()),
        store: Mutex::new(store),
        snapshots,
        ancestors,
    };
    let workers = std::thread::available_parallelism().map_or(1, usize::from);
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| worker(&pass));
        }
    });
    let mut failures = std::mem::take(&mut *lock(&pass.failures));
    if failures.is_empty()
        && let Some(hash) = &setup.hash
    {
        let equalities = std::mem::take(&mut *lock(&pass.equalities));
        let recorded = if setup.full {
            record_pass_wide(&pass, equalities)
        } else {
            Ok(())
        };
        if let Err(error) = recorded.and_then(|()| lock(&pass.store).finish_pass(hash)) {
            failures.push(error);
        }
    }
    let result = if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("\n"))
    };
    pass.progress
        .into_inner()
        .unwrap_or_else(PoisonError::into_inner)
        .end(&result);
    if result.is_ok() {
        let _ = std::fs::remove_dir_all(&pass.scratch);
    }
    result
}

/// The progress groups of `tasks`: all runs, the configurations' with a
/// row for each, then each other kind's, those with runs only; and the
/// groups each task counts in.
fn progress_groups(tasks: &[Task]) -> (Vec<Group>, Vec<Vec<usize>>) {
    let mut lines: Vec<(&str, usize)> = vec![("all runs", 0), ("configurations", 0)];
    for task in tasks {
        if let (_, Some(row)) = task.group() {
            lines.push((row, 1));
        }
    }
    for kind in ["sessions", "replays", "vendor projects", "fixtures"] {
        lines.push((kind, 0));
    }
    let mut totals = vec![0; lines.len()];
    let find = |label: &str, indent: usize| {
        lines
            .iter()
            .position(|&(l, i)| l == label && i == indent)
            .expect("listed")
    };
    let of: Vec<Vec<usize>> = tasks
        .iter()
        .map(|task| {
            let (kind, row) = task.group();
            let mut mine = vec![0, find(kind, 0)];
            mine.extend(row.map(|row| find(row, 1)));
            mine
        })
        .collect();
    for (task, mine) in tasks.iter().zip(&of) {
        for &g in mine {
            totals[g] += task.runs();
        }
    }
    // Lines with no runs go, and the indexes follow.
    let kept: Vec<usize> = (0..lines.len()).filter(|&i| totals[i] > 0).collect();
    let groups = kept
        .iter()
        .map(|&i| Group {
            label: lines[i].0.to_string(),
            total: totals[i],
            indent: lines[i].1,
        })
        .collect();
    let of = of
        .into_iter()
        .map(|mine| {
            mine.into_iter()
                .map(|g| kept.iter().position(|&k| k == g).expect("a counted group"))
                .collect()
        })
        .collect();
    (groups, of)
}

/// Runs tasks from the queue until none is left.
fn worker(pass: &Pass) {
    loop {
        let (index, task) = {
            let mut queue = lock(&pass.queue);
            loop {
                let room = queue.large_running < LARGE_AT_ONCE;
                if let Some(at) = queue.tasks.iter().position(|(_, t)| room || !t.large()) {
                    let (index, task) = queue.tasks.remove(at).expect("found");
                    if task.large() {
                        queue.large_running += 1;
                    }
                    break (index, task);
                }
                if queue.tasks.is_empty() {
                    return;
                }
                queue = pass
                    .ready
                    .wait(queue)
                    .unwrap_or_else(PoisonError::into_inner);
            }
        };
        let large = task.large();
        if let Err(error) = run_task(pass, index, task) {
            lock(&pass.progress).fail(error.clone());
            lock(&pass.failures).push(error);
        }
        let mut queue = lock(&pass.queue);
        if large {
            queue.large_running -= 1;
        }
        pass.ready.notify_all();
    }
}

impl Pass<'_> {
    /// Runs `step` as the run `label` of task `index`, timed and counted.
    fn step(
        &self,
        index: usize,
        label: &str,
        step: impl FnOnce() -> Result<(), String>,
    ) -> Result<(), String> {
        lock(&self.progress).start(label);
        let started = Instant::now();
        let result = step();
        lock(&self.timings).record(label, started.elapsed().as_secs_f64(), result.is_ok());
        lock(&self.progress).finish(
            label,
            &self.groups[index],
            result.as_ref().err().map(|e| format!("{label}: {e}")),
        );
        result
    }

    /// A command of `program` in the corpus, in `state`.
    fn command(&self, program: &Path, state: &State, caches: &Path) -> Command {
        let mut cmd = Command::new(program);
        cmd.current_dir(corpus_dir()).stdout(Stdio::null());
        if let Some(cache) = state.cache {
            cmd.env("PHPSTAN_RS_CACHE_DIR", caches.join(cache));
        }
        cmd.env_remove("PHPSTAN_RS_NO_STUB_CACHE");
        // The oracle build's assertions read statements a cached entry
        // does not hold.
        if !state.file_cache || self.setup.profile == "oracle" {
            cmd.env("PHPSTAN_RS_NO_FILE_CACHE", "1");
        } else {
            cmd.env_remove("PHPSTAN_RS_NO_FILE_CACHE");
        }
        match state.threads {
            Some(threads) => cmd.env("PHPSTAN_RS_THREADS", threads),
            None => cmd.env_remove("PHPSTAN_RS_THREADS"),
        };
        cmd
    }

    fn hash(&self) -> Option<&str> {
        self.setup.hash.as_deref()
    }

    /// Writes to `to` the manifest of the dump `entry` of the nearest
    /// snapshot that has it, HEAD's own included: what a first run is
    /// written against. False when no snapshot has it.
    fn export_reference(&self, entry: &str, to: &Path) -> Result<bool, String> {
        let Some(hash) = self.ancestors.iter().find(|hash| {
            self.snapshots
                .get(*hash)
                .is_some_and(|entries| entries.contains(entry))
        }) else {
            return Ok(false);
        };
        let kind = dump_kind(entry).ok_or_else(|| format!("{entry} is not a dump"))?;
        lock(&self.store).export(&format!("{}:{entry}", snapshot_ref(hash)), kind, to)?;
        Ok(true)
    }

    /// Adds `entries` to the pass's tree, when the pass records.
    fn record(&self, what: &str, entries: Vec<(String, Content)>) -> Result<(), String> {
        let Some(hash) = self.hash() else {
            return Ok(());
        };
        let label = format!("record {what}");
        let started = Instant::now();
        let result = lock(&self.store).add(&pass_ref(hash), &label, entries);
        lock(&self.timings).record(&label, started.elapsed().as_secs_f64(), result.is_ok());
        result.map_err(|e| format!("{label}: {e}"))
    }

    /// The text of `dump`'s entity `index`: from the run's text, or, where
    /// the run wrote it against a snapshot or its text is recorded and
    /// gone, from the store.
    fn entity_text(&self, dump: &mut Dump, index: usize) -> Result<Vec<u8>, String> {
        dump.text(index)
            .or_else(|_| lock(&self.store).blob(dump.manifest.entries[index].id))
    }

    /// Where the dump of the manifest `b` first differs from that of `a`:
    /// the entity's path, then a diff of its texts.
    fn dump_difference(&self, a: &Path, b: &Path) -> Result<Option<String>, String> {
        let open = |path: &Path| Dump::open(path).map_err(|e| e.to_string());
        let (mut a, mut b) = (open(a)?, open(b)?);
        let Some(at) = dumps::first_difference(&a.manifest, &b.manifest) else {
            return Ok(None);
        };
        let entity = |dump: &mut Dump, index: Option<usize>| -> Result<(String, Vec<u8>), String> {
            match index {
                None => Ok(("the end".to_string(), Vec::new())),
                Some(index) => Ok((
                    dump.manifest.entries[index].path.clone(),
                    self.entity_text(dump, index)?,
                )),
            }
        };
        let (a_path, a_text) = entity(&mut a, at.a)?;
        let (b_path, b_text) = entity(&mut b, at.b)?;
        let place = if a_path == b_path {
            a_path
        } else {
            format!("{a_path}, against {b_path}")
        };
        Ok(Some(format!(
            "{place}\n{}",
            shown(unified_diff(&a_text, &b_text)?.as_bytes())
        )))
    }

    /// Where the run in `b` first differs from the run in `a`: a dump,
    /// written as a text with its manifest beside it, or a directory of
    /// them, of plain files and of such directories, which a file only `a`
    /// has does not make differ: a dump `b`'s state does not take.
    fn run_difference(&self, a: &Path, b: &Path) -> Result<Option<String>, String> {
        if !b.is_dir() {
            return Ok(self
                .dump_difference(&manifest_path(a), &manifest_path(b))?
                .map(|found| format!("{}: {found}", b.display())));
        }
        let mut names: Vec<String> = std::fs::read_dir(b)
            .map_err(|e| format!("{}: {e}", b.display()))?
            .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
            .collect();
        names.sort();
        for name in names {
            let (old, new) = (a.join(&name), b.join(&name));
            if manifest_path(&new).exists() {
                continue;
            }
            if !old.exists() {
                return Ok(Some(format!("{}: only this run has it", new.display())));
            }
            if new.is_dir() {
                if let Some(found) = self.run_difference(&old, &new)? {
                    return Ok(Some(found));
                }
                continue;
            }
            let found = if name.ends_with(".manifest") {
                self.dump_difference(&old, &new)?
            } else {
                let read = |path: &Path| {
                    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
                };
                let (old, new) = (read(&old)?, read(&new)?);
                (old != new)
                    .then(|| unified_diff(&old, &new).map(|diff| shown(diff.as_bytes())))
                    .transpose()?
            };
            if let Some(found) = found {
                return Ok(Some(format!("{}: {found}", new.display())));
            }
        }
        Ok(None)
    }
}

/// What the store keeps of the run in `dir`: each dump by its manifest,
/// each other file as a text, by name.
fn dir_content(dir: &Path) -> Result<Content, String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().into_owned()))
        .filter(|name| !name.ends_with(".manifest"))
        .collect();
    names.sort();
    let mut entries = Vec::new();
    for name in names {
        let path = dir.join(&name);
        let manifest = manifest_path(&path);
        let content = if manifest.exists() {
            Content::Dump(manifest)
        } else {
            text_content(&name, read_bytes(&path)?)
        };
        entries.push((name, content));
    }
    Ok(Content::Dir(entries))
}

fn read_bytes(path: &Path) -> Result<Vec<u8>, String> {
    std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))
}

fn run_task(pass: &Pass, index: usize, task: Task) -> Result<(), String> {
    match task {
        Task::Config {
            config,
            states,
            views,
            levels,
            dumps,
            stub_bases,
            determinism,
        } => config_task(
            pass,
            index,
            config,
            states,
            ConfigRuns {
                views,
                levels,
                dumps,
                stub_bases,
                determinism,
            },
        ),
        Task::Session { name, project } => session_task(pass, index, &name, &project),
        Task::Replay { slug, from, args } => replay_task(pass, index, &slug, &from, &args),
        Task::VendorProject { project } => vendor_project_task(pass, index, &project),
        Task::Fixtures => fixtures_task(pass, index),
    }
}

/// The dumps a configuration's runs take: each file the runner writes,
/// with the snapshot's entry for it.
fn config_dumps(config: &str, stub_bases: bool) -> Vec<(&'static str, String)> {
    let mut dumps = vec![
        ("registry.txt", format!("registry-{config}.txt")),
        ("expr-types.txt", format!("expr-types-{config}.txt")),
        (
            "doc-resolutions.txt",
            format!("doc-resolutions-{config}.txt"),
        ),
        ("front-end.txt", format!("front-end-{config}")),
    ];
    if stub_bases {
        dumps.push(("stub-bases.txt", "stub-bases.txt".to_string()));
    }
    dumps
}

/// What a configuration's runs write and check beyond its report.
#[derive(Clone, Copy)]
struct ConfigRuns {
    /// The type and baseline views.
    views: bool,
    /// Every level's report.
    levels: bool,
    dumps: bool,
    stub_bases: bool,
    /// The check of the shipped binary's emission order.
    determinism: bool,
}

/// Analyses `config` in each of `states`, each state's artifacts compared
/// with the first's, and records the first's as soon as it has run, its
/// dumps' texts then gone from the scratch.
fn config_task(
    pass: &Pass,
    index: usize,
    config: Config,
    states: &'static [State],
    runs: ConfigRuns,
) -> Result<(), String> {
    let ConfigRuns {
        views,
        levels,
        dumps,
        stub_bases,
        determinism,
    } = runs;
    let dir = pass.scratch.join(config.name);
    let caches = dir.join("caches");
    let baseline = baseline_file(config.name);
    let references = dir.join("references");
    std::fs::create_dir_all(&references).map_err(|e| e.to_string())?;
    if dumps {
        for (file, entry) in config_dumps(config.name, stub_bases) {
            pass.export_reference(&entry, &manifest_path(&references.join(file)))?;
        }
    }
    let mut first: Option<(PathBuf, &str)> = None;
    for (i, state) in states.iter().enumerate() {
        let out = dir.join(format!("state-{i}"));
        let mut cmd = pass.command(&pass.runner, state, &caches);
        cmd.args((config.args)()).arg("--out").arg(&out);
        cmd.arg("--reference")
            .arg(first.as_ref().map_or(&references, |(first, _)| first));
        if views {
            cmd.arg("--dump-types");
            if baseline.exists() {
                cmd.arg("--baseline").arg(&baseline);
            }
        }
        if levels {
            cmd.arg("--levels");
        }
        if dumps {
            cmd.args([
                "--dump-registry",
                "--dump-expr-types",
                "--dump-doc-resolutions",
            ]);
            if i == 0 || state.threads.is_some() {
                cmd.arg("--dump-front-end");
            }
        }
        if stub_bases && i < 2 {
            cmd.arg("--dump-stub-bases");
        }
        pass.step(index, &format!("{} {}", config.name, state.name), || {
            run(&mut cmd)
        })?;
        match &first {
            None => {
                if pass.hash().is_some() {
                    record_config(pass, config, &out, runs)?;
                    for (file, _) in config_dumps(config.name, stub_bases) {
                        let _ = std::fs::remove_file(out.join(file));
                    }
                }
                first = Some((out, state.name));
            }
            Some((reference, reference_name)) => {
                if let Some(difference) = pass.run_difference(reference, &out)? {
                    return Err(format!(
                        "{}: the run with {} differs from the run with {reference_name} at {difference}",
                        config.name, state.name
                    ));
                }
                lock(&pass.equalities).push((
                    index,
                    format!("{}: {} = {reference_name}", config.name, state.name),
                ));
                let _ = std::fs::remove_dir_all(&out);
            }
        }
    }
    let (reference, _) = first.expect("a configuration runs in a state");
    if determinism {
        let state = &states[0];
        let label = format!("{} emission order of the binary", config.name);
        pass.step(index, &label, || {
            emission_order_check(pass, config, state, &caches, &reference)
        })?;
    }
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

/// Requires the shipped binary's `--emission-order` output of `config` in
/// `state` to be the runner's emission view of the run in `reference`:
/// the engine reproduces its order, and the runner runs what the binary
/// runs.
fn emission_order_check(
    pass: &Pass,
    config: Config,
    state: &State,
    caches: &Path,
    reference: &Path,
) -> Result<(), String> {
    let mut cmd = pass.command(&pass.binary, state, caches);
    cmd.arg("check-project")
        .args((config.args)())
        .args(["--format", "json", "--emission-order"])
        .stdout(Stdio::piped());
    let out = cmd
        .output()
        .map_err(|e| format!("running phpstan-rs on {}: {e}", config.name))?;
    let read = |name: &str| {
        std::fs::read(reference.join(name)).map_err(|e| format!("{}: {name}: {e}", config.name))
    };
    let (stdout, stderr) = (read("emission.json")?, read("emission.err")?);
    if out.stdout != stdout || out.stderr != stderr {
        let diff = if out.stdout != stdout {
            unified_diff(&stdout, &out.stdout)?
        } else {
            unified_diff(&stderr, &out.stderr)?
        };
        let shown: String = diff.lines().take(40).collect::<Vec<_>>().join("\n");
        return Err(format!(
            "{}: the binary's --emission-order output differs from the runner's: either the engine isn't reproducible, or the runner doesn't run what the binary runs\n{shown}",
            config.name
        ));
    }
    Ok(())
}

/// Records the artifacts of `config`'s run in `out`, each level's report
/// as its own artifact.
fn record_config(pass: &Pass, config: Config, out: &Path, runs: ConfigRuns) -> Result<(), String> {
    let artifact = config.name;
    let mut entries = report_entries(artifact, out)?;
    if runs.levels {
        for level in &LEVELS[1..] {
            let name = artifact_name(&config, level);
            entries.extend(report_entries(&name, &out.join("levels").join(level))?);
        }
    }
    if runs.views {
        let name = format!("types-{artifact}.txt");
        entries.push((
            name.clone(),
            text_content(&name, read_bytes(&out.join("types.txt"))?),
        ));
        let baseline = out.join("baseline.json");
        if baseline.exists() {
            let normalized = normalize(&read_text(&baseline)?)?;
            let stderr = read_text(&out.join("baseline.err"))?;
            // The Stage A snapshot is empty; `compare` holds every later one
            // to its parent's plus the commit's approved delta.
            let remaining = normalized.lines().filter(|l| !l.starts_with('#')).count();
            let stale = stderr
                .lines()
                .filter(|l| l.contains("baseline entry"))
                .count();
            if remaining != 0 || stale != 0 {
                eprintln!(
                    "baseline compatibility ({artifact}): {remaining} diagnostic(s) and {stale} stale entr(ies) with the reference baseline"
                );
            }
            let name = format!("baseline-{artifact}.txt");
            entries.push((name.clone(), text_content(&name, normalized.into_bytes())));
            entries.push((
                format!("baseline-{artifact}.err"),
                Content::Text(stderr.into_bytes()),
            ));
        } else {
            eprintln!("no reference baseline for {artifact} yet; run `cargo xtask baseline`");
        }
    }
    if runs.dumps {
        for (file, entry) in config_dumps(artifact, runs.stub_bases) {
            entries.push((entry, Content::Dump(manifest_path(&out.join(file)))));
        }
    }
    pass.record(artifact, entries)
}

/// The report of the run in `out` as `artifact`: the normalized report, its
/// stderr, and every field of every diagnostic.
fn report_entries(artifact: &str, out: &Path) -> Result<Vec<(String, Content)>, String> {
    let report = format!("{artifact}.txt");
    let diagnostics = format!("diagnostics-{artifact}.txt");
    Ok(vec![
        (
            report.clone(),
            text_content(
                &report,
                normalize(&read_text(&out.join("report.json"))?)?.into_bytes(),
            ),
        ),
        (
            format!("{artifact}.err"),
            Content::Text(read_bytes(&out.join("report.err"))?),
        ),
        (
            diagnostics.clone(),
            text_content(&diagnostics, read_bytes(&out.join("diagnostics.txt"))?),
        ),
    ])
}

fn read_text(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

/// Takes the server's session over `project` at each thread count, and
/// requires them to be the same: the default's record written against the
/// nearest snapshot's, the one thread's against the default's.
fn session_task(pass: &Pass, index: usize, name: &str, project: &str) -> Result<(), String> {
    let artifact = format!("session-{name}");
    let entry = format!("{artifact}.txt");
    let snapshot = manifest_path(&pass.scratch.join(format!("{artifact}-snapshot.txt")));
    let mut reference = pass
        .export_reference(&entry, &snapshot)?
        .then_some(snapshot);
    let mut taken = Vec::new();
    for threads in RUN_THREADS {
        let out = pass
            .scratch
            .join(format!("{artifact}-{}.txt", threads.unwrap_or("default")));
        let state = State {
            name: "session",
            cache: Some("sessions"),
            file_cache: false,
            threads,
        };
        let mut cmd = pass.command(&pass.binary, &state, &pass.scratch);
        cmd.arg("record-session").arg(project).arg(&out);
        if let Some(reference) = &reference {
            cmd.arg("--reference").arg(reference);
        }
        let label = format!("{artifact} at {} threads", threads.unwrap_or("default"));
        pass.step(index, &label, || run(&mut cmd))?;
        reference = Some(manifest_path(&out));
        taken.push(out);
    }
    record_pair(pass, &entry, &taken[0], &taken[1])
}

/// Runs the edit replay in a fresh copy of `from` at each thread count, in
/// the same place each time, and requires the two to be the same: the one
/// thread's dumps written against the default's. The default's are written
/// whole, since the replay reads its steps' texts back.
fn replay_task(
    pass: &Pass,
    index: usize,
    slug: &str,
    from: &Path,
    args: &[String],
) -> Result<(), String> {
    let artifact = format!("replay-{slug}");
    let copy = pass.scratch.join("replays").join(slug);
    let mut taken: Vec<PathBuf> = Vec::new();
    for threads in RUN_THREADS {
        let out = pass
            .scratch
            .join(format!("{artifact}-{}", threads.unwrap_or("default")));
        let _ = std::fs::remove_dir_all(&copy);
        std::fs::create_dir_all(copy.parent().expect("under the scratch"))
            .map_err(|e| e.to_string())?;
        run(Command::new("cp").arg("-a").arg(from).arg(&copy))?;
        write(&copy.join(".phpstan-rs-replay"), b"")?;
        let state = State {
            name: "replay",
            cache: Some("replays-cache"),
            file_cache: false,
            threads,
        };
        let mut cmd = pass.command(&pass.binary, &state, &pass.scratch);
        cmd.current_dir(&copy)
            .arg("check-project")
            .args(args)
            .arg("--edit-replay")
            .arg(&out);
        if let Some(default) = taken.first() {
            cmd.arg("--dump-reference").arg(default);
        }
        let label = format!("{artifact} at {} threads", threads.unwrap_or("default"));
        pass.step(index, &label, || run(&mut cmd))?;
        taken.push(out);
    }
    let _ = std::fs::remove_dir_all(&copy);
    record_pair(pass, &artifact, &taken[0], &taken[1])
}

/// Requires the runs at the default and at one thread to be the same and,
/// when recording, records the default's as the snapshot's `entry`: a dump,
/// or a directory of them.
fn record_pair(pass: &Pass, entry: &str, default: &Path, one: &Path) -> Result<(), String> {
    if let Some(difference) = pass.run_difference(default, one)? {
        return Err(format!(
            "{entry}: one thread differs from the default at {difference}"
        ));
    }
    if pass.hash().is_some() {
        let content = if default.is_dir() {
            dir_content(default)?
        } else {
            Content::Dump(manifest_path(default))
        };
        pass.record(entry, vec![(entry.to_string(), content)])?;
    }
    for path in [one, default] {
        let _ = std::fs::remove_dir_all(path);
        let _ = std::fs::remove_file(path);
        let _ = std::fs::remove_file(manifest_path(path));
    }
    Ok(())
}

/// Analyses the vendor project alone, every rule set on, with no
/// configuration file and no Composer context; the reports are joined into
/// `vendor-projects` once all have run.
fn vendor_project_task(pass: &Pass, index: usize, project: &str) -> Result<(), String> {
    let out = vendor_out(&pass.scratch, project);
    let state = State {
        name: "vendor project",
        cache: Some("vendor-cache"),
        file_cache: false,
        threads: None,
    };
    let mut cmd = pass.command(&pass.runner, &state, &pass.scratch);
    cmd.arg("--no-composer");
    for set in ALL_ON_SETS {
        cmd.arg("--set").arg(set);
    }
    cmd.arg("--out").arg(&out).arg(project);
    pass.step(index, &format!("vendor project {project}"), || {
        run(&mut cmd)
    })
}

/// Where a vendor project's run writes.
fn vendor_out(scratch: &Path, project: &str) -> PathBuf {
    scratch
        .join("vendor-projects")
        .join(project.replace('/', "-"))
}

/// Runs the fixture suite with each fixture's records on and, when
/// recording, records them.
fn fixtures_task(pass: &Pass, index: usize) -> Result<(), String> {
    let dir = pass.scratch.join("fixtures");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let profile = pass.setup.profile;
    pass.step(index, "fixtures", || {
        run(cargo()
            .args([
                "test",
                "--profile",
                profile,
                "--test",
                "fixtures",
                "--quiet",
            ])
            .env("PHPSTAN_RS_DUMP_TYPES", &dir)
            .env("PHPSTAN_RS_DUMP_DIAGNOSTICS", &dir))
    })?;
    if pass.hash().is_some() {
        pass.record("the fixtures", fixture_entries(&dir)?)?;
    }
    Ok(())
}

/// What a full pass records once every task has run: the vendor projects'
/// reports joined in path order, and the equalities kept.
fn record_pass_wide(pass: &Pass, mut equalities: Vec<(usize, String)>) -> Result<(), String> {
    let (mut report, mut err, mut diagnostics) = (String::new(), String::new(), String::new());
    for project in vendor_projects()? {
        let out = vendor_out(&pass.scratch, &project);
        let read = |name: &str| read_text(&out.join(name));
        for (text, name) in [
            (&mut err, "report.err"),
            (&mut diagnostics, "diagnostics.txt"),
        ] {
            writeln!(text, "# project {project}").unwrap();
            text.push_str(&read(name)?);
        }
        writeln!(report, "# project {project}").unwrap();
        report.push_str(&normalize(&read("report.json")?)?);
    }
    equalities.sort_by_key(|(task, _)| *task);
    let mut lines = String::new();
    for (_, line) in equalities {
        writeln!(lines, "{line}").unwrap();
    }
    let entries = [
        ("vendor-projects.txt", report),
        ("vendor-projects.err", err),
        ("diagnostics-vendor-projects.txt", diagnostics),
        ("equalities.txt", lines),
    ]
    .into_iter()
    .map(|(name, text)| (name.to_string(), text_content(name, text.into_bytes())))
    .collect();
    pass.record("the vendor projects and the equalities", entries)
}
