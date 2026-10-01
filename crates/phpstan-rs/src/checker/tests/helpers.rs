use crate::checker::settings::CheckSettings;
use diagnostics::Diagnostic;

pub(super) fn all_diagnostics_for(src: &str) -> Vec<Diagnostic> {
    all_diagnostics_for_with(src, |_| {})
}

/// `src` loaded as the one analysed file of a project, finished the way
/// every production path finishes one.
fn project_for(src: &str, configure: impl FnOnce(&mut CheckSettings)) -> crate::project::Project {
    let mut project = crate::project::Project::new();
    project
        .load_analyzed_file(std::path::PathBuf::from("test.php"), src)
        .unwrap();
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
