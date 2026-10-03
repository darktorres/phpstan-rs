//! The LSP-mode equivalent of `main/check_project.rs::run_check_project_command`'s
//! own CLI+config merge, minus anything CLI-only (the LSP has no flags
//! of its own - a workspace's `phpstan-rs.toml`, auto-discovered at its
//! root, is the only source for everything here).

use std::path::{Path, PathBuf};

use composer::vendor::VendorContext;
use config::{ConfigFile, Schema};

use crate::checker::settings::EngineSettings;
use crate::ext::disallowed_calls::DisallowedCallsSettings;
use crate::ext::sql::SqlSettings;
use crate::project::ProjectSettings;
use crate::project::session::SessionSettings;
use crate::registry::settings::ReflectionSettings;

/// Everything a workspace's `phpstan-rs.toml` (or its absence) resolves
/// into, applied once per `Server::configure_from_workspace` call.
/// `PartialEq` backs `Server::reload_configuration`'s no-op fast path -
/// comparing the real resolved value (not a `Debug`-formatted fingerprint)
/// so a change to a field a hand-written `Debug` impl deliberately
/// redacts (`SqlReflectionConfig`'s `password`) is never silently missed.
#[derive(Clone, PartialEq)]
pub(crate) struct ResolvedWorkspaceConfig {
    /// What the workspace's project is configured with.
    pub(crate) settings: ProjectSettings,
    pub(crate) vendor_context: VendorContext,
    pub(crate) extra_context_dirs: Vec<PathBuf>,
    pub(crate) baseline: Option<crate::baseline::Baseline>,
    pub(crate) ignore_error_entries: Vec<crate::ignore_errors::IgnoreErrorEntry>,
    pub(crate) analyze_paths: Vec<PathBuf>,
}

/// The two kinds of paths `Server::load_workspace` needs beyond `root`
/// itself - loaded first, as context (`analyzed: false`), so `root`'s
/// own walk can dedupe against them by canonicalized path. See
/// `Server::load_workspace`'s own doc comment for why load order matters
/// beyond just the dedup. `analyze_paths` is the *analyzed* (`analyzed:
/// true`) scope's own restriction - empty means "no `paths` key
/// configured, walk all of `root`" (`Server::load_workspace_files`'s own
/// fallback), matching `run_check_project`'s identical "config.paths
/// empty -> analyze everything under the given dirs" default, not "empty
/// -> analyze nothing".
pub(crate) struct WorkspaceContextPaths {
    pub(crate) vendor: VendorContext,
    pub(crate) extra_dirs: Vec<PathBuf>,
    pub(crate) analyze_paths: Vec<PathBuf>,
}

/// Resolves `root`'s own `phpstan-rs.toml` (or `config_path_override`,
/// for a future explicit-path setting), read through `schema`, into
/// everything `Server::configure_from_workspace` needs to apply. Every
/// failure mode (a malformed config file, an invalid `php_version`, an
/// unreadable `ext.sql.reflection.schema_file`) degrades to "as if unset"
/// plus one `eprintln!`, never `std::process::exit` - unlike
/// `check-project`, there is no process invocation here to fail out of,
/// and a malformed config must not kill the editor session.
pub(crate) fn resolve(
    root: &Path,
    config_path_override: Option<&Path>,
    schema: &Schema,
) -> ResolvedWorkspaceConfig {
    let config_path = config_path_override
        .map(Path::to_path_buf)
        .unwrap_or_else(|| root.join("phpstan-rs.toml"));
    let config = if config_path.is_file() {
        match ConfigFile::load(&config_path, schema).and_then(|c| c.validate(schema).map(|()| c)) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("phpstan-rs lsp: {e}");
                ConfigFile::empty()
            }
        }
    } else {
        ConfigFile::empty()
    };
    for warning in config.warnings() {
        eprintln!("phpstan-rs lsp: {warning}");
    }
    fn section<S: config::Section>(config: &ConfigFile) -> S {
        config.section::<S>().unwrap_or_else(|e| {
            eprintln!("phpstan-rs lsp: {e}");
            S::default()
        })
    }
    let session = section::<SessionSettings>(&config);
    let reflection = section::<ReflectionSettings>(&config);
    let disallowed_calls = section::<DisallowedCallsSettings>(&config);

    let resolve_against_root = |p: PathBuf| root.join(p);

    let php_version = match reflection.php_version {
        Some(s) => match types::php_version::parse_php_version(&s) {
            Ok(v) => Some(v),
            Err(e) => {
                eprintln!("phpstan-rs lsp: {e}");
                None
            }
        },
        None => None,
    };

    let sql_reflection = section::<SqlSettings>(&config).reflection;
    let sql_schema = match sql_reflection
        .as_ref()
        .and_then(|c| c.schema_file.as_ref().map(|f| (f.clone(), c.driver)))
    {
        Some((path, driver)) => {
            let path = resolve_against_root(path);
            match sql::schema::parse_ddl_file(&path, driver) {
                Ok(catalog) => Some(catalog),
                Err(e) => {
                    eprintln!("phpstan-rs lsp: {e}");
                    None
                }
            }
        }
        None => None,
    };

    let mut restricted_symbols = disallowed_calls.restricted_symbols.clone();
    for preset in &disallowed_calls.restricted_symbol_presets {
        match crate::banned::restricted_symbol_preset(preset) {
            Ok(entries) => restricted_symbols.extend(entries),
            Err(e) => eprintln!("phpstan-rs lsp: {e}"),
        }
    }

    let baseline = session
        .baseline
        .clone()
        .map(resolve_against_root)
        .and_then(|path| match crate::baseline::Baseline::load(&path) {
            Ok(b) => Some(b.with_match_mode(if session.baseline_match_identifier {
                crate::baseline::MatchMode::Identifier
            } else {
                crate::baseline::MatchMode::Message
            })),
            Err(e) => {
                eprintln!(
                    "phpstan-rs lsp: failed to read baseline {}: {e}",
                    path.display()
                );
                None
            }
        });

    let no_composer = reflection.no_composer.unwrap_or(false);
    let absolute_root = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let package = if no_composer {
        composer::PackageContext::default()
    } else {
        composer::PackageContext::load(&absolute_root, root)
    };
    let phpstan_api_check_enabled = package.requires_phpstan();
    let extensions = crate::ext::ActiveExtensions::resolve(&config, &|p| {
        crate::ext::installed_in(package.manifest.as_ref(), p)
    })
    .unwrap_or_else(|e| {
        eprintln!("phpstan-rs lsp: {e}");
        crate::ext::ActiveExtensions::default()
    });
    let php_versions = php_version
        .map(types::PhpVersions::exactly)
        .or_else(|| crate::project::session::declared_php_versions(&package));
    let packages = composer::PackageIndex::new(package.manifest.into_iter().collect());
    let vendor_context = package.vendor.get().clone();

    let analyze_paths: Vec<PathBuf> = session
        .paths
        .clone()
        .unwrap_or_default()
        .into_iter()
        .map(resolve_against_root)
        .collect();

    let mut extra_context_dirs: Vec<PathBuf> = reflection
        .scan_paths
        .iter()
        .cloned()
        .map(resolve_against_root)
        .collect();
    extra_context_dirs.extend(
        reflection
            .stub_paths
            .iter()
            .cloned()
            .map(resolve_against_root),
    );

    let settings = ProjectSettings {
        working_dir: Some(root.to_path_buf()),
        absolute_root: Some(absolute_root),
        packages,
        php_versions,
        configured_level: session.level,
        phpstan_api_check_enabled,
        defer_inline_ignores: false,
        retain_source: true,
        built_once: false,
        rules: section(&config),
        extensions,
        extras: section(&config),
        strict_rules: section(&config),
        shipmonk: section(&config),
        voku: section(&config),
        slam: section(&config),
        psr3: section(&config),
        sidz: section(&config),
        phpunit: section(&config),
        restricted_symbols,
        doctrine: section(&config),
        dynamic_constant_names: section::<EngineSettings>(&config).dynamic_constant_entries(),
        overlays: section(&config),
        sql_reflection,
        sql_schema,
    };

    ResolvedWorkspaceConfig {
        settings,
        vendor_context,
        extra_context_dirs,
        baseline,
        ignore_error_entries: session.ignore_errors,
        analyze_paths,
    }
}
