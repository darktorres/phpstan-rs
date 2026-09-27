//! A `check-project` run: its configuration resolved into a session,
//! and the baseline it's reported against.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::baseline::{self, Baseline};
use crate::config::Config;
use crate::project::session::{Finalize, Overrides, Session};

/// What a `check-project` run asks for, after its command line is parsed.
#[derive(Default, Clone)]
pub struct CheckRequest {
    /// The TOML config file; its directory is what the config's relative
    /// paths resolve against.
    pub config_file: Option<PathBuf>,
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

/// `request`'s config file loaded, or the default config without one.
pub fn load_config(request: &CheckRequest) -> Result<Config, String> {
    match &request.config_file {
        Some(path) => Config::load(path),
        None => Ok(Config::default()),
    }
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

/// Resolves `request` with `config` (already overlaid with whatever the
/// command line sets) into a run on `pool`, rooted at the working
/// directory.
pub fn prepare(
    request: &CheckRequest,
    config: Config,
    pool: Arc<rayon::ThreadPool>,
) -> Result<CheckRun, String> {
    let config_dir = request
        .config_file
        .as_deref()
        .and_then(Path::parent)
        .map(Path::to_path_buf);
    let baseline_path = request
        .baseline
        .clone()
        .or(config.baseline.clone().map(|p| match &config_dir {
            Some(dir) => dir.join(p),
            None => p,
        }));
    let baseline_mode =
        if request.baseline_match_identifier || config.baseline_match_identifier.unwrap_or(false) {
            baseline::MatchMode::Identifier
        } else {
            baseline::MatchMode::Message
        };
    let baseline_per_identifier =
        request.baseline_per_identifier || config.baseline_per_identifier.unwrap_or(false);
    let overrides = Overrides {
        paths: request.paths.clone(),
        scan_paths: request.scan_paths.clone(),
        stub_paths: request.stub_paths.clone(),
        level: request.level,
        report_level: None,
        php_version: request.php_version.clone(),
        no_composer: request.no_composer,
        emission_order: request.emission_order,
    };
    let session = Session::resolve(config, config_dir, PathBuf::from("."), overrides, pool)?;
    Ok(CheckRun {
        session,
        baseline_path,
        baseline_mode,
        baseline_per_identifier,
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
