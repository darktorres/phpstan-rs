//! A `check-project` run: its configuration resolved into a session,
//! and the baseline it's reported against.

use std::path::{Path, PathBuf};

use config::ConfigFile;

use crate::baseline::{self, Baseline};
use crate::project::session::{ErrorPolicy, Finalize, Overrides, Session};
use crate::registry::stub_set::Stubs;

/// What a `check-project` run asks for, after its command line is parsed.
#[derive(Default, Clone)]
pub struct CheckRequest {
    /// The TOML config file; its directory is what the config's relative
    /// paths resolve against.
    pub config_file: Option<PathBuf>,
    /// `key=value` assignments over the config's keys (`--set`).
    pub sets: Vec<String>,
    pub paths: Vec<PathBuf>,
    pub scan_paths: Vec<PathBuf>,
    pub stub_paths: Vec<PathBuf>,
    pub level: Option<u8>,
    pub php_version: Option<String>,
    pub no_composer: bool,
    pub emission_order: bool,
    /// The baseline to report against, over the config's.
    pub baseline: Option<PathBuf>,
    pub baseline_match_identifier: bool,
    pub baseline_per_identifier: bool,
}

/// `request`'s config file (or none) with its `--set` assignments
/// applied, checked against every owner's section.
pub fn load_config(request: &CheckRequest) -> Result<ConfigFile, String> {
    let schema = super::config_schema();
    let mut config = match &request.config_file {
        Some(path) => ConfigFile::load(path, &schema)?,
        None => ConfigFile::empty(),
    };
    for assignment in &request.sets {
        config.set(&schema, assignment)?;
    }
    config.validate(&schema)?;
    Ok(config)
}

/// A resolved `check-project` run.
pub struct CheckRun {
    pub session: Session,
    /// The baseline to report against, resolved against the config's
    /// directory when the config names it.
    pub baseline_path: Option<PathBuf>,
    pub baseline_mode: baseline::MatchMode,
    pub baseline_per_identifier: bool,
}

/// Resolves `request` with `config` (`load_config`) into a run on the pool
/// of `stubs`, rooted at `root`. `policy` is what a configuration that cannot
/// be used does (`ErrorPolicy`).
pub fn prepare(
    request: &CheckRequest,
    config: &ConfigFile,
    stubs: Stubs,
    root: PathBuf,
    policy: ErrorPolicy,
) -> Result<CheckRun, String> {
    let overrides = Overrides {
        paths: request.paths.clone(),
        scan_paths: request.scan_paths.clone(),
        stub_paths: request.stub_paths.clone(),
        level: request.level,
        report_level: None,
        php_version: request.php_version.clone(),
        no_composer: request.no_composer,
        emission_order: request.emission_order,
        baseline: request.baseline.clone(),
        baseline_match_identifier: request.baseline_match_identifier,
        baseline_per_identifier: request.baseline_per_identifier,
    };
    let session = Session::resolve(
        config,
        config.dir().map(Path::to_path_buf),
        root,
        overrides,
        stubs,
        policy,
    )?;
    Ok(CheckRun {
        baseline_path: session.baseline_path().map(Path::to_path_buf),
        baseline_mode: session.baseline_mode(),
        baseline_per_identifier: session.baseline_per_identifier(),
        session,
    })
}

impl CheckRun {
    /// The baseline to report against, loaded; the error is the line to
    /// report.
    pub fn load_baseline(&self) -> Option<Result<Baseline, String>> {
        let path = self.baseline_path.as_ref()?;
        Some(
            Baseline::load(path)
                .map(|b| b.with_match_mode(self.baseline_mode))
                .map_err(|e| format!("failed to load baseline {}: {e}", path.display())),
        )
    }

    /// What resolving the run said beyond its diagnostics, one line each.
    pub fn notices(&self) -> Vec<String> {
        self.session.warnings.clone()
    }

    /// The session's post-processing, reported against `baseline`.
    pub fn finalize_options(&self, baseline: Option<Baseline>) -> Finalize {
        Finalize {
            baseline,
            ..self.session.finalize_options()
        }
    }
}
