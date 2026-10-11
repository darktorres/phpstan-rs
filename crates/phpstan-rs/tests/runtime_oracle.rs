//! The runtime oracle: every fixture under `tests/fixtures` is run under
//! PHP, and each `assertType()` that runs must allow the value PHP
//! produced there. The shim (`runtime_oracle/shim.php`) records each call
//! and each warning PHP emits.
//!
//! A fixture whose `X.toml` sets `runtime_warnings = "exact"` must also
//! expect a diagnostic on every line PHP warned on. `runtime_packages`
//! names libraries the fixture needs at runtime, installed from
//! `runtime_oracle/composer.lock`. A fixture analysed for another PHP
//! version runs in that version's `php:<v>-cli` Docker image when it is
//! present.
//!
//! `PHPSTAN_RS_ORACLE_IMAGE` (an image reference, digest-pinned) and
//! `PHPSTAN_RS_ORACLE_PHP` (the exact `PHP_VERSION` it must report) pin the
//! run: every fixture runs in that image, a fixture whose versions don't
//! include the pinned one is not executed, and anything the run needs that is
//! missing fails it instead of skipping. CI sets both.
//!
//! A fixture that reaches no `assertType()` is `not executed`, and one
//! that can't be run here is `skipped`; neither fails. The counts, and
//! each fixture in the two lists, are printed at the end.
//!
//! `cargo test -p phpstan-rs --profile oracle --test runtime_oracle`: the
//! `oracle` profile is the `gate` profile with the debug assertions and
//! overflow checks that the checker's own assertions need.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use libtest_mimic::{Arguments, Failed, Trial};
use phpstan_rs::gate::{self, FixtureConfig, TemplateScopes};
use phpstan_rs::project::session::SessionSettings;
use phpstan_rs::project::worker_pool;
use serde_json::Value;
use types::{PhpString, Shape, ShapeField, ShapeKey, Type};

/// How long one fixture may run.
const TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Default)]
struct Outcomes {
    executed: Vec<String>,
    not_executed: Vec<(String, String)>,
    skipped: Vec<(String, String)>,
}

fn main() {
    let args = Arguments::from_args();
    let pin = Pin::from_env().map(Arc::new);
    let local = match &pin {
        Some(pin) => {
            if let Err(why) = pin.verify() {
                eprintln!("runtime oracle: {why}");
                std::process::exit(1);
            }
            String::new()
        }
        None => {
            let Some(local) = local_php_version() else {
                println!("runtime oracle: no `php` on PATH, nothing run");
                return;
            };
            local
        }
    };
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let pool = worker_pool();
    let outcomes = Arc::new(Mutex::new(Outcomes::default()));
    let mut trials = Vec::new();
    collect(&root, &root, &pool, &local, &pin, &outcomes, &mut trials);
    trials.sort_by(|a, b| a.name().cmp(b.name()));
    let conclusion = libtest_mimic::run(&args, trials);
    let outcomes = outcomes.lock().unwrap();
    for (name, why) in &outcomes.not_executed {
        println!("not executed: {name}: {why}");
    }
    for (name, why) in &outcomes.skipped {
        println!("skipped: {name}: {why}");
    }
    println!(
        "runtime oracle: {} executed, {} not executed, {} skipped",
        outcomes.executed.len(),
        outcomes.not_executed.len(),
        outcomes.skipped.len()
    );
    conclusion.exit();
}

fn collect(
    root: &Path,
    dir: &Path,
    pool: &Arc<rayon::ThreadPool>,
    local: &str,
    pin: &Option<Arc<Pin>>,
    outcomes: &Arc<Mutex<Outcomes>>,
    trials: &mut Vec<Trial>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if !path.with_extension("expect").is_file() {
                collect(root, &path, pool, local, pin, outcomes, trials);
            }
        } else if path.extension().is_some_and(|e| e == "expect") {
            let name = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .with_extension("")
                .display()
                .to_string();
            let pool = Arc::clone(pool);
            let local = local.to_string();
            let pin = pin.clone();
            let outcomes = Arc::clone(outcomes);
            trials.push(Trial::test(name.clone(), move || {
                let outcome = run(
                    &name,
                    &path.with_extension(""),
                    pool,
                    &local,
                    pin.as_deref(),
                );
                let mut outcomes = outcomes.lock().unwrap();
                match outcome {
                    Ok(Outcome::Executed) => outcomes.executed.push(name),
                    Ok(Outcome::NotExecuted(why)) => outcomes.not_executed.push((name, why)),
                    Ok(Outcome::Skipped(why)) => outcomes.skipped.push((name, why)),
                    Err(failure) => {
                        outcomes.executed.push(name);
                        return Err(failure);
                    }
                }
                Ok(())
            }));
        }
    }
}

enum Outcome {
    Executed,
    NotExecuted(String),
    Skipped(String),
}

/// Where the fixture runs: the local `php`, or a Docker image.
enum Runner {
    Local,
    Docker(String),
}

impl Runner {
    /// A name for the runner, fit for a directory.
    fn label(&self) -> String {
        match self {
            Runner::Local => "local".to_string(),
            Runner::Docker(image) => image.replace([':', '/'], "_"),
        }
    }
}

fn run(
    name: &str,
    stem: &Path,
    pool: Arc<rayon::ThreadPool>,
    local: &str,
    pin: Option<&Pin>,
) -> Result<Outcome, Failed> {
    let config = gate::fixture_config(&stem.with_extension("toml"))?;
    if config
        .requires_feature
        .iter()
        .any(|f| !(f == "pcre-validate" && cfg!(feature = "pcre-validate")))
    {
        return Ok(Outcome::Skipped("a required feature is off".to_string()));
    }
    // A fixture analysed for a range of versions runs at each end of it.
    let mut runners = Vec::new();
    match (pin, config.php_versions()?) {
        (Some(pin), None) => runners.push(Runner::Docker(pin.image.clone())),
        (Some(pin), Some(versions)) => {
            if [versions.min, versions.max]
                .iter()
                .any(|end| pin.is_minor(end.major, end.minor))
            {
                runners.push(Runner::Docker(pin.image.clone()));
            } else {
                return Ok(Outcome::NotExecuted(format!(
                    "targets PHP {}.{} to {}.{}; this run is pinned to PHP {}",
                    versions.min.major,
                    versions.min.minor,
                    versions.max.major,
                    versions.max.minor,
                    pin.version
                )));
            }
        }
        (None, None) => runners.push(Runner::Local),
        (None, Some(versions)) => {
            let mut ends = vec![versions.min, versions.max];
            ends.dedup();
            for end in ends {
                let version = end.to_string();
                if version == local {
                    runners.push(Runner::Local);
                    continue;
                }
                let image = format!("php:{version}-cli");
                if !docker_image_present(&image) {
                    return Ok(Outcome::Skipped(format!(
                        "needs the `{image}` Docker image"
                    )));
                }
                runners.push(Runner::Docker(image));
            }
        }
    }
    let autoload = if config.runtime_packages.is_empty() {
        None
    } else {
        match runtime_packages() {
            Ok(autoload) => Some(autoload),
            Err(why) if pin.is_some() => return Err(why.into()),
            Err(why) => return Ok(Outcome::Skipped(why)),
        }
    };
    let (base, subjects) = subjects(stem, &config)?;
    let sources: Vec<(String, String)> = subjects
        .iter()
        .map(|subject| {
            let bytes = std::fs::read(base.join(subject)).unwrap_or_default();
            (
                subject.display().to_string(),
                String::from_utf8_lossy(&bytes).into_owned(),
            )
        })
        .collect();
    let asserts_anywhere = sources.iter().any(|(_, source)| source.contains("assert"));
    // Without an assertion in the source there is no record to check, so PHP
    // is not run for it. The exception is a fixture whose warnings must match
    // its diagnostics, which PHP has to run for.
    if !asserts_anywhere && !config.runtime_warnings_exact {
        return Ok(Outcome::NotExecuted(
            "no assertType() in the source".to_string(),
        ));
    }
    // The analysis: what the calls are made of, and what each assertion
    // is judged against.
    let loaded = if asserts_anywhere {
        let (session, _) = gate::load_fixture(stem, &config, pool)?;
        Some(session.load()?)
    } else {
        None
    };
    // Calls of the functions that assert, with values of their parameters'
    // types; a fixture whose warnings must be exactly its diagnostics is
    // run as written.
    let calls = match &loaded {
        Some(loaded) if config.runtime_calls && !config.runtime_warnings_exact => {
            let files: Vec<(&str, &str)> = sources
                .iter()
                .map(|(path, source)| (path.as_str(), source.as_str()))
                .collect();
            gate::samples::runtime_calls(loaded.project.registry(), &files)
        }
        _ => String::new(),
    };
    let mut failures = Vec::new();
    let mut not_executed = Vec::new();
    let mut runs = Vec::new();
    for runner in &runners {
        let work = Path::new(env!("CARGO_TARGET_TMPDIR"))
            .join("runtime_oracle")
            .join(name.replace('/', "__"))
            .join(runner.label());
        let _ = std::fs::remove_dir_all(&work);
        if stem.with_extension("php").is_file() {
            std::fs::create_dir_all(&work)?;
            for subject in &subjects {
                std::fs::copy(base.join(subject), work.join(subject))?;
            }
        } else {
            copy_dir(&base, &work)?;
        }
        let entry = work.join("__oracle_entry.php");
        let mut script = String::from("<?php\n");
        for subject in &subjects {
            script.push_str(&format!(
                "require_once __DIR__ . '/{}';\n",
                subject
                    .display()
                    .to_string()
                    .replace('\\', "\\\\")
                    .replace('\'', "\\'")
            ));
        }
        script.push_str(&calls);
        std::fs::write(&entry, script)?;
        let out = work.join("__oracle.jsonl");
        let finished = execute(runner, &work, autoload.as_deref())?;
        let records = read_records(&out)?;
        if config.runtime_warnings_exact {
            failures.extend(unexpected_warnings(stem, &records, &work, runner));
        }
        let asserts: Vec<Value> = records
            .into_iter()
            .filter(|r| r["kind"] == "assert")
            .collect();
        if asserts.is_empty() {
            not_executed.push(if finished {
                format!("{}: no assertType() ran", runner.label())
            } else {
                format!("{}: timed out after {}s", runner.label(), TIMEOUT.as_secs())
            });
        }
        runs.push((runner, work, asserts));
    }
    if runs.iter().all(|(_, _, asserts)| asserts.is_empty()) {
        if failures.is_empty() {
            return Ok(Outcome::NotExecuted(not_executed.join("; ")));
        }
        return Err(failures.join("\n").into());
    }
    let Some(loaded) = &loaded else {
        return Err("an assertion ran in a fixture with none written".into());
    };
    let registry = loaded.project.registry();
    let relation = registry.relation();
    let mut scopes = TemplateScopes::new(registry);
    for (runner, work, asserts) in &runs {
        for record in asserts {
            let Some(rel) = relative(&record["file"], work, runner) else {
                continue;
            };
            let line = record["line"].as_u64().unwrap_or(0) as u32;
            let original = base.join(&rel);
            let col = assert_type_column(&original, line);
            let scope = scopes.at(&original, line, col);
            let expected_text = record["expected"].as_str().unwrap_or_default();
            let at = format!("{}:{line} ({})", rel.display(), runner.label());
            let Some(expected) =
                types::try_parse_displayed_with(expected_text, &|n| scope.resolve(n))
            else {
                failures.push(format!(
                    "{at}: expected type `{expected_text}` does not parse"
                ));
                continue;
            };
            let Some(value) = value_type(&record["value"]) else {
                failures.push(format!("{at}: unreadable value {}", record["value"]));
                continue;
            };
            if relation.is_super_type_of(&expected, &value).no() {
                failures.push(format!(
                    "{at}: assertType('{expected_text}') got a value of type {value}"
                ));
            }
        }
    }
    if failures.is_empty() {
        Ok(Outcome::Executed)
    } else {
        Err(failures.join("\n").into())
    }
}

/// The directory the fixture's paths are relative to, and the files the
/// entry script loads: `X.php`, or every PHP file its config analyses
/// (the whole directory without `paths`), in path order.
fn subjects(stem: &Path, config: &FixtureConfig) -> Result<(PathBuf, Vec<PathBuf>), Failed> {
    let file = stem.with_extension("php");
    if file.is_file() {
        let base = file.parent().unwrap_or(Path::new("")).to_path_buf();
        let name = PathBuf::from(file.file_name().unwrap_or_default());
        return Ok((base, vec![name]));
    }
    let base = stem.to_path_buf();
    let roots = config
        .config
        .section::<SessionSettings>()?
        .paths
        .unwrap_or_else(|| vec![PathBuf::new()]);
    let mut files = BTreeSet::new();
    for root in roots {
        php_files(&base, &base.join(root), &mut files);
    }
    Ok((base, files.into_iter().collect()))
}

fn php_files(base: &Path, path: &Path, out: &mut BTreeSet<PathBuf>) {
    if path.is_file() {
        if path.extension().is_some_and(|e| e == "php")
            && let Ok(rel) = path.strip_prefix(base)
        {
            out.insert(rel.to_path_buf());
        }
        return;
    }
    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    for entry in entries.flatten() {
        php_files(base, &entry.path(), out);
    }
}

fn copy_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// Runs the entry script in `work`; whether it finished before the
/// timeout.
fn execute(runner: &Runner, work: &Path, autoload: Option<&Path>) -> Result<bool, Failed> {
    let shim_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/runtime_oracle");
    let ini = |shim: &str| {
        vec![
            "-d".to_string(),
            "error_reporting=-1".to_string(),
            "-d".to_string(),
            "display_errors=0".to_string(),
            "-d".to_string(),
            "log_errors=0".to_string(),
            "-d".to_string(),
            "zend.assertions=1".to_string(),
            "-d".to_string(),
            "assert.exception=1".to_string(),
            "-d".to_string(),
            "memory_limit=512M".to_string(),
            "-d".to_string(),
            format!("auto_prepend_file={shim}"),
        ]
    };
    let mut command = match runner {
        Runner::Local => {
            let mut command = Command::new("php");
            command
                .args(ini(&shim_dir.join("shim.php").display().to_string()))
                .arg("__oracle_entry.php")
                .current_dir(work)
                .env("PHPSTAN_RS_ORACLE_OUT", work.join("__oracle.jsonl"));
            if let Some(autoload) = autoload {
                command.env("PHPSTAN_RS_ORACLE_AUTOLOAD", autoload);
            }
            command
        }
        Runner::Docker(image) => {
            let mut command = Command::new("docker");
            command
                .args(["run", "--rm", "--network", "none", "-w", "/work"])
                .arg("-v")
                .arg(format!("{}:/work", work.display()))
                .arg("-v")
                .arg(format!("{}:/oracle:ro", shim_dir.display()))
                .args(["-e", "PHPSTAN_RS_ORACLE_OUT=/work/__oracle.jsonl"]);
            if let Some(autoload) = autoload {
                let vendor = autoload.parent().unwrap_or(Path::new("/"));
                command
                    .arg("-v")
                    .arg(format!("{}:/vendor:ro", vendor.display()))
                    .args(["-e", "PHPSTAN_RS_ORACLE_AUTOLOAD=/vendor/autoload.php"]);
            }
            command
                .arg(image)
                .arg("php")
                .args(ini("/oracle/shim.php"))
                .arg("__oracle_entry.php");
            command
        }
    };
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    let started = Instant::now();
    loop {
        if child.try_wait()?.is_some() {
            return Ok(true);
        }
        if started.elapsed() > TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn read_records(out: &Path) -> Result<Vec<Value>, Failed> {
    let Ok(text) = std::fs::read_to_string(out) else {
        return Ok(Vec::new());
    };
    text.lines()
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).map_err(|e| Failed::from(e.to_string())))
        .collect()
}

/// A recorded file as a path relative to the fixture's directory; `None`
/// for a file outside it (the shim, a runtime package).
fn relative(file: &Value, work: &Path, runner: &Runner) -> Option<PathBuf> {
    let file = Path::new(file.as_str()?);
    let root = match runner {
        Runner::Local => work.to_path_buf(),
        Runner::Docker(_) => PathBuf::from("/work"),
    };
    file.strip_prefix(root).ok().map(Path::to_path_buf)
}

/// The column of the first `assertType` on `line` of `path`, 1-based; 1
/// when it isn't there.
fn assert_type_column(path: &Path, line: u32) -> u32 {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| {
            let line = text.lines().nth(line.checked_sub(1)? as usize)?;
            line.find("assertType").map(|col| col as u32 + 1)
        })
        .unwrap_or(1)
}

/// Each warning, notice or deprecation PHP emitted on a line the
/// fixture's expectation has no diagnostic on.
fn unexpected_warnings(
    stem: &Path,
    records: &[Value],
    work: &Path,
    runner: &Runner,
) -> Vec<String> {
    let expect = std::fs::read_to_string(stem.with_extension("expect")).unwrap_or_default();
    let single_file = stem.with_extension("php").is_file();
    let mut expected: BTreeSet<(String, u32)> = BTreeSet::new();
    for line in expect.lines().filter(|l| !l.starts_with(' ')) {
        let (file, rest) = if single_file {
            (
                stem.with_extension("php")
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
                line,
            )
        } else {
            match line.split_once(':') {
                Some((file, rest)) => (file.to_string(), rest),
                None => continue,
            }
        };
        if let Some(n) = rest.split(':').next().and_then(|n| n.parse().ok()) {
            expected.insert((file, n));
        }
    }
    records
        .iter()
        .filter(|r| r["kind"] == "error")
        .filter_map(|r| {
            let rel = relative(&r["file"], work, runner)?;
            let line = r["line"].as_u64()? as u32;
            let key = (rel.display().to_string(), line);
            (!expected.contains(&key)).then(|| {
                format!(
                    "{}:{line}: PHP warned with no expected diagnostic: {}",
                    key.0,
                    r["message"].as_str().unwrap_or_default()
                )
            })
        })
        .collect()
}

/// The type of one recorded value: its literal, or a sealed shape of its
/// elements' types.
fn value_type(value: &Value) -> Option<Type> {
    Some(match value["t"].as_str()? {
        "null" => Type::null(),
        "bool" => Type::bool_literal(value["v"].as_bool()?),
        "int" => Type::int_literal(value["v"].as_i64()?),
        "float" => {
            let bytes = hex(value["bits"].as_str()?)?;
            Type::float_literal(f64::from_be_bytes(bytes.try_into().ok()?))
        }
        "string" => Type::string_literal(PhpString::from_bytes(
            hex(value["hex"].as_str()?)?.as_slice(),
        )),
        "array" => {
            let mut fields = Vec::new();
            for item in value["items"].as_array()? {
                let key = match &item[0] {
                    Value::Number(n) => ShapeKey::Int(n.as_i64()?),
                    Value::Object(key) => ShapeKey::Str(PhpString::from_bytes(
                        hex(key.get("hex")?.as_str()?)?.as_slice(),
                    )),
                    _ => return None,
                };
                fields.push(ShapeField::new(key, value_type(&item[1])?, false));
            }
            Type::array_shape(Shape::sealed(fields))
        }
        "enum" => Type::enum_case(
            value["class"].as_str()?.into(),
            value["case"].as_str()?.into(),
        ),
        "object" => match value["class"].as_str() {
            Some(class) => Type::object(class.into(), Vec::new()),
            None => Type::object_without_class(),
        },
        "resource" => Type::resource(value["closed"].as_bool()?),
        "unknown" => Type::mixed(),
        _ => return None,
    })
}

fn hex(text: &str) -> Option<Vec<u8>> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

/// The PHP a run is pinned to: an image whose digest fixes its build, and the
/// exact `PHP_VERSION` that image must report.
struct Pin {
    image: String,
    version: String,
    major: u8,
    minor: u8,
}

impl Pin {
    fn from_env() -> Option<Pin> {
        let image = std::env::var("PHPSTAN_RS_ORACLE_IMAGE").ok()?;
        let version = std::env::var("PHPSTAN_RS_ORACLE_PHP")
            .expect("PHPSTAN_RS_ORACLE_PHP names the PHP_VERSION the pinned image reports");
        let mut parts = version.split('.').map(str::parse::<u8>);
        let (Some(Ok(major)), Some(Ok(minor))) = (parts.next(), parts.next()) else {
            panic!("PHPSTAN_RS_ORACLE_PHP `{version}` is not a PHP version");
        };
        Some(Pin {
            image,
            version,
            major,
            minor,
        })
    }

    fn is_minor(&self, major: u8, minor: u8) -> bool {
        self.major == major && self.minor == minor
    }

    /// Fails unless the pinned image runs and reports exactly the pinned version.
    fn verify(&self) -> Result<(), String> {
        let output = Command::new("docker")
            .args(["run", "--rm", "--network", "none", &self.image])
            .args(["php", "-r", "echo PHP_VERSION;"])
            .output()
            .map_err(|e| format!("docker: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "`{}` did not run: {}",
                self.image,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        let reported = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if reported != self.version {
            return Err(format!(
                "`{}` reports PHP {reported}, expected {}",
                self.image, self.version
            ));
        }
        Ok(())
    }
}

/// `8.0` of `8.0`, `8.0.30` or `8.0 - 8.3`'s lower end.
fn local_php_version() -> Option<String> {
    let output = Command::new("php")
        .args(["-r", "echo PHP_MAJOR_VERSION, '.', PHP_MINOR_VERSION;"])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn docker_image_present(image: &str) -> bool {
    static PRESENT: OnceLock<Mutex<rustc_hash::FxHashMap<String, bool>>> = OnceLock::new();
    let cache = PRESENT.get_or_init(Default::default);
    if let Some(present) = cache.lock().unwrap().get(image) {
        return *present;
    }
    let present = Command::new("docker")
        .args(["image", "inspect", image])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    cache.lock().unwrap().insert(image.to_string(), present);
    present
}

/// The runtime packages' autoloader, installed from the committed lock
/// file into the target directory on first use.
fn runtime_packages() -> Result<PathBuf, String> {
    static INSTALLED: OnceLock<Result<PathBuf, String>> = OnceLock::new();
    INSTALLED
        .get_or_init(|| {
            let vendor = Path::new(env!("CARGO_TARGET_TMPDIR"))
                .join("runtime_oracle")
                .join("vendor");
            let autoload = vendor.join("autoload.php");
            if autoload.is_file() {
                return Ok(autoload);
            }
            let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/runtime_oracle");
            let status = Command::new("composer")
                .args(["install", "--no-interaction", "--no-progress", "--quiet"])
                .arg("--working-dir")
                .arg(&dir)
                .env("COMPOSER_VENDOR_DIR", &vendor)
                .status()
                .map_err(|e| format!("composer: {e}"))?;
            if status.success() && autoload.is_file() {
                Ok(autoload)
            } else {
                Err("the runtime packages could not be installed".to_string())
            }
        })
        .clone()
}
