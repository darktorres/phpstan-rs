use crate::checker::settings::CheckSettings;
use crate::project::session::load_test_file;
use diagnostics::Diagnostic;

pub(super) fn all_diagnostics_for(src: &str) -> Vec<Diagnostic> {
    all_diagnostics_for_with(src, |_| {})
}

/// `src` loaded as the one analysed file of a project, finished the way
/// every production path finishes one.
fn project_for(src: &str, configure: impl FnOnce(&mut CheckSettings)) -> crate::project::Project {
    project_in(crate::project::Project::new(), src, configure)
}

/// `project_for`, in `project`.
fn project_in(
    mut project: crate::project::Project,
    src: &str,
    configure: impl FnOnce(&mut CheckSettings),
) -> crate::project::Project {
    load_test_file(
        &mut project,
        std::path::PathBuf::from("test.php"),
        src,
        true,
    );
    project.finish_loading();
    configure(project.check_settings_mut());
    project
}

/// Every statement's recorded variable types, as the type dump writes them.
pub(super) fn type_dump_for(src: &str) -> Vec<String> {
    project_for(src, |_| {}).type_dump(std::path::Path::new("test.php"))
}

pub(super) fn all_diagnostics_for_with(
    src: &str,
    configure: impl FnOnce(&mut CheckSettings),
) -> Vec<Diagnostic> {
    project_for(src, configure).check_file(std::path::Path::new("test.php"))
}

/// `all_diagnostics_for_with`, in a project rooted at `root` whose include
/// path is `include_path`: the includes that name a file below `root` are
/// judged.
pub(super) fn all_diagnostics_in_root_with(
    src: &str,
    root: &std::path::Path,
    include_path: &str,
    configure: impl FnOnce(&mut CheckSettings),
) -> Vec<Diagnostic> {
    let mut project = crate::project::Project::new();
    project.apply_settings(crate::project::ProjectSettings {
        absolute_root: Some(root.to_path_buf()),
        include: crate::checker::include_lookup::IncludeSettings {
            include_path: Some(include_path.to_string()),
            ..Default::default()
        },
        ..crate::project::ProjectSettings::default()
    });
    project_in(project, src, configure).check_file(std::path::Path::new("test.php"))
}
