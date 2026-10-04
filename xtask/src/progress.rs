//! A command's progress, for `cargo xtask progress` to show while the
//! command runs: the command writes its state to a status file each time a
//! run starts or ends, with the times things started, and the view renders
//! it against the clock.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

/// The width of a bar, in cells.
const BAR: usize = 30;

/// One line of counts: a label, how many runs it has, how many are done,
/// and how far it is indented under the line before it.
pub(crate) struct Group {
    pub(crate) label: String,
    pub(crate) total: usize,
    pub(crate) indent: usize,
}

/// The state of a command that runs many things, written to `path` as it
/// changes.
pub(crate) struct Progress {
    path: PathBuf,
    command: String,
    started: u64,
    groups: Vec<(Group, usize)>,
    running: Vec<(String, u64)>,
    failures: Vec<String>,
    outcome: Option<(u64, Option<String>)>,
}

/// Seconds since the epoch.
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Progress {
    /// The progress of `command`, with its groups of runs, written to `path`.
    pub(crate) fn new(path: PathBuf, command: &str, groups: Vec<Group>) -> Progress {
        let progress = Progress {
            path,
            command: command.to_string(),
            started: now(),
            groups: groups.into_iter().map(|g| (g, 0)).collect(),
            running: Vec::new(),
            failures: Vec::new(),
            outcome: None,
        };
        progress.write();
        progress
    }

    /// `run` started.
    pub(crate) fn start(&mut self, run: &str) {
        self.running.push((run.to_string(), now()));
        self.write();
    }

    /// `run` ended, counting in each of `groups` (indexes into the groups
    /// given to `new`); `failure` says how, when it failed.
    pub(crate) fn finish(&mut self, run: &str, groups: &[usize], failure: Option<String>) {
        self.running.retain(|(r, _)| r != run);
        for &g in groups {
            self.groups[g].1 += 1;
        }
        self.failures.extend(failure);
        self.write();
    }

    /// A failure that belongs to no one run.
    pub(crate) fn fail(&mut self, failure: String) {
        self.failures.push(failure);
        self.write();
    }

    /// The command ended, with `result`.
    pub(crate) fn end(&mut self, result: &Result<(), String>) {
        self.outcome = Some((now(), result.as_ref().err().cloned()));
        self.write();
    }

    fn write(&self) {
        let state = json!({
            "command": self.command,
            "pid": std::process::id(),
            "started": self.started,
            "groups": self.groups.iter().map(|(g, done)| json!({
                "label": g.label,
                "total": g.total,
                "done": done,
                "indent": g.indent,
            })).collect::<Vec<_>>(),
            "running": self.running.iter().map(|(run, since)| json!({
                "run": run,
                "since": since,
            })).collect::<Vec<_>>(),
            "failures": self.failures,
            "ended": self.outcome.as_ref().map(|(at, error)| json!({
                "at": at,
                "error": error,
            })),
        });
        // Written whole and renamed into place, so a view never reads half.
        let temporary = self.path.with_extension("json.tmp");
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if std::fs::write(&temporary, state.to_string()).is_ok() {
            let _ = std::fs::rename(&temporary, &self.path);
        }
    }
}

/// `seconds` as minutes and seconds.
fn duration(seconds: u64) -> String {
    format!("{}m {:02}s", seconds / 60, seconds % 60)
}

/// A bar of `done` out of `total`, with the counts; full when there is
/// nothing to do.
fn bar(done: u64, total: u64) -> String {
    let filled = (BAR as u64 * done)
        .checked_div(total)
        .map_or(BAR, |cells| (cells as usize).min(BAR));
    format!(
        "{}{} {done:>3}/{total}",
        "█".repeat(filled),
        "░".repeat(BAR - filled)
    )
}

/// Whether the process `pid` is alive.
fn alive(pid: u64) -> bool {
    Path::new("/proc").join(pid.to_string()).exists()
}

/// The view of the state at `path`, at `at` (seconds since the epoch), and
/// whether the command is over: ended, or gone without ending.
pub(crate) fn render(path: &Path, at: u64) -> Result<(String, bool), String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let state: Value =
        serde_json::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    let number = |v: &Value, key: &str| v.get(key).and_then(Value::as_u64).unwrap_or(0);
    let started = number(&state, "started");
    let ended = state.get("ended").filter(|e| !e.is_null());
    let gone = ended.is_none() && !alive(number(&state, "pid"));
    let mut out = String::new();
    let command = state.get("command").and_then(Value::as_str).unwrap_or("?");
    let until = ended.map_or(at, |e| number(e, "at"));
    out.push_str(&format!(
        "cargo xtask {command}   elapsed {}\n\n",
        duration(until.saturating_sub(started))
    ));
    let label_width = state["groups"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|g| g["label"].as_str().map_or(0, str::len) + 2 * number(g, "indent") as usize)
        .max()
        .unwrap_or(0);
    for group in state["groups"].as_array().into_iter().flatten() {
        let indent = "  ".repeat(number(group, "indent") as usize);
        let label = format!("{indent}{}", group["label"].as_str().unwrap_or(""));
        out.push_str(&format!(
            "  {label:<label_width$}  {}\n",
            bar(number(group, "done"), number(group, "total"))
        ));
    }
    let mut running: Vec<(&str, u64)> = state["running"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| (r["run"].as_str().unwrap_or(""), number(r, "since")))
        .collect();
    running.sort_by_key(|&(_, since)| since);
    if ended.is_none() && !gone {
        out.push_str(&format!("\n  running now ({})\n", running.len()));
        let width = running.iter().map(|(r, _)| r.len()).max().unwrap_or(0);
        for (run, since) in &running {
            out.push_str(&format!(
                "    {run:<width$}  {:>8}\n",
                duration(at.saturating_sub(*since))
            ));
        }
    }
    let failures: Vec<&str> = state["failures"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    if failures.is_empty() {
        out.push_str("\n  no failure so far\n");
    } else {
        out.push_str(&format!("\n  failures ({})\n", failures.len()));
        for failure in failures {
            out.push_str(&format!("    {failure}\n"));
        }
    }
    if let Some(ended) = ended {
        match ended.get("error").and_then(Value::as_str) {
            None => out.push_str("\n  finished: passed\n"),
            Some(_) => out.push_str("\n  finished: failed\n"),
        }
    } else if gone {
        out.push_str("\n  stopped without finishing: its process is gone\n");
    }
    Ok((out, ended.is_some() || gone))
}

/// `cargo xtask progress [--watch] [<command>]`: shows the progress of the
/// last run of `command` (`dumps` by default) from `dir`; with `--watch`,
/// again every two seconds until it is over, waiting for a run to write
/// its progress when none has.
pub(crate) fn progress_command(dir: &Path, args: &[String]) -> Result<(), String> {
    let watch = args.iter().any(|a| a == "--watch");
    let command = args
        .iter()
        .find(|a| !a.starts_with("--"))
        .map_or("dumps", String::as_str);
    let path = status_path(dir, command);
    loop {
        let (view, over) = if path.exists() {
            render(&path, now())?
        } else {
            (
                format!(
                    "no `cargo xtask {command}` run has written its progress yet: a run \
                     writes\n{} as it goes{}\n",
                    path.display(),
                    if watch { "; waiting for one" } else { "" }
                ),
                false,
            )
        };
        if watch {
            // Home, then clear the screen, so the view stays in place.
            print!("\x1b[H\x1b[2J{view}");
        } else {
            print!("{view}");
        }
        if !watch || over {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}

/// Where the progress of `command` is written.
pub(crate) fn status_path(dir: &Path, command: &str) -> PathBuf {
    dir.join(format!("progress-{command}.json"))
}
