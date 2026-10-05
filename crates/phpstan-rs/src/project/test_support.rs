//! Projects of in-memory files, loaded the way a session loads them.

use std::path::{Path, PathBuf};

use super::session::{Overrides, Policy, Session};
use super::{Project, ProjectSettings};

/// The files of a project, with the role each has.
#[derive(Default)]
pub(crate) struct Files {
    analysed: Vec<(PathBuf, String)>,
    context: Vec<(PathBuf, String)>,
    settings: Option<ProjectSettings>,
}

impl Files {
    pub(crate) fn new() -> Files {
        Files::default()
    }

    /// A file that is checked.
    pub(crate) fn analysed(mut self, path: impl AsRef<Path>, source: &str) -> Files {
        self.analysed
            .push((path.as_ref().to_path_buf(), source.to_string()));
        self
    }

    /// A file that is only read for the declarations it gives.
    pub(crate) fn context(mut self, path: impl AsRef<Path>, source: &str) -> Files {
        self.context
            .push((path.as_ref().to_path_buf(), source.to_string()));
        self
    }

    /// What the project is configured with, in place of the session's own.
    pub(crate) fn settings(mut self, settings: ProjectSettings) -> Files {
        self.settings = Some(settings);
        self
    }

    /// The session that loads the files, kept for edits as the language
    /// server keeps its project.
    pub(crate) fn session(self) -> Session {
        let schema = crate::app::config_schema();
        let config = config::ConfigFile::parse("", &schema).expect("an empty configuration parses");
        let paths = |files: &[(PathBuf, String)]| -> Vec<PathBuf> {
            files.iter().map(|(path, _)| path.clone()).collect()
        };
        let overrides = Overrides {
            paths: paths(&self.analysed),
            scan_paths: paths(&self.context),
            no_composer: true,
            ..Overrides::default()
        };
        let session = Session::resolve(
            &config,
            None,
            PathBuf::from("/in-memory"),
            overrides,
            Policy::Degrade,
            super::worker_pool(),
        )
        .expect("the files resolve")
        .for_editing();
        let session = match self.settings {
            Some(settings) => session.with_settings(settings),
            None => session,
        };
        session.with_overlay(
            self.analysed
                .into_iter()
                .chain(self.context)
                .map(|(path, source)| (path, source.into_bytes())),
        )
    }

    /// The project the files load into, with loading finished.
    pub(crate) fn load(self) -> Project {
        let paths = |files: &[(PathBuf, String)]| -> Vec<PathBuf> {
            files.iter().map(|(path, _)| path.clone()).collect()
        };
        let (analysed, context) = (paths(&self.analysed), paths(&self.context));
        let session = self.session();
        session.load_listed(false, &context, analysed).project
    }
}
