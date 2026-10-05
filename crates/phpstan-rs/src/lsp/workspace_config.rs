//! What the language server takes from a workspace's `phpstan-rs.toml` (or
//! its absence): the session `check-project` resolves for the same
//! configuration, and which findings are published.

use std::path::Path;
use std::sync::Arc;

use config::{ConfigFile, Schema};

use crate::project::ProjectSettings;
use crate::project::session::{
    FileSet, Finalize, Overrides, Policy, Session, SessionSettings, configured_cutoff,
};

/// Everything a workspace's configuration resolves into that the server
/// applies, once per `Workspace::configure` call.
/// `PartialEq` backs `Workspace::reload_configuration`'s no-op fast path -
/// comparing the real resolved value (not a `Debug`-formatted fingerprint)
/// so a change to a field a hand-written `Debug` impl deliberately
/// redacts (`SqlReflectionConfig`'s `password`) is never silently missed.
#[derive(Clone, PartialEq)]
pub(crate) struct ResolvedWorkspaceConfig {
    /// What the workspace's project is configured with.
    pub(crate) settings: ProjectSettings,
    /// What decides which files the workspace loads.
    pub(crate) file_set: FileSet,
    /// Which findings are published, and how: every rule's, with no level
    /// configured.
    pub(crate) finalize: Finalize,
    /// What was wrong in the configuration and was taken as unset.
    pub(crate) problems: Vec<String>,
}

/// A workspace's session, and what the server applies of it.
pub(crate) struct OpenedWorkspace {
    pub(crate) session: Session,
    pub(crate) config: ResolvedWorkspaceConfig,
}

/// Resolves `root`'s own `phpstan-rs.toml` (or `config_path_override`),
/// read through `schema`, as `check-project` resolves it. Every failure
/// mode (a malformed config file, an invalid `php_version`, an unreadable
/// `ext.sql.reflection.schema_file`) is taken as unset and listed in
/// `ResolvedWorkspaceConfig::problems`, never `std::process::exit` -
/// there is no process invocation here to fail out of, and a malformed
/// config must not kill the editor session.
pub(crate) fn open(
    root: &Path,
    config_path_override: Option<&Path>,
    schema: &Schema,
    pool: Arc<rayon::ThreadPool>,
) -> OpenedWorkspace {
    let config_path = config_path_override
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.join("phpstan-rs.toml"));
    let mut problems = Vec::new();
    let config = if config_path.is_file() {
        match ConfigFile::load(&config_path, schema).and_then(|c| c.validate(schema).map(|()| c)) {
            Ok(c) => c,
            Err(e) => {
                problems.push(e);
                ConfigFile::empty()
            }
        }
    } else {
        ConfigFile::empty()
    };
    let config_dir = config.dir().unwrap_or(root).to_path_buf();
    let resolve = |config: &ConfigFile| {
        Session::resolve(
            config,
            Some(config_dir.clone()),
            root.to_path_buf(),
            Overrides::default(),
            Policy::Degrade,
            pool.clone(),
        )
    };
    let session = resolve(&config)
        .or_else(|e| {
            problems.push(e);
            resolve(&ConfigFile::empty())
        })
        .unwrap_or_else(|e| unreachable!("a degrading resolve of no configuration fails: {e}"))
        .for_editing();
    problems.extend(session.warnings.iter().cloned());

    let settings: &SessionSettings = session.session_settings();
    let baseline = settings
        .baseline
        .clone()
        .map(|p| config_dir.join(p))
        .and_then(|path| match crate::baseline::Baseline::load(&path) {
            Ok(b) => Some(b.with_match_mode(if settings.baseline_match_identifier {
                crate::baseline::MatchMode::Identifier
            } else {
                crate::baseline::MatchMode::Message
            })),
            Err(e) => {
                problems.push(format!("failed to read baseline {}: {e}", path.display()));
                None
            }
        });
    let config = ResolvedWorkspaceConfig {
        settings: session.settings().clone(),
        file_set: session.file_set(),
        finalize: Finalize {
            cutoff: configured_cutoff(settings.level, u8::MAX),
            baseline,
            ..session.finalize_options()
        },
        problems,
    };
    OpenedWorkspace { session, config }
}
