//! A context file read back from the per-file cache keeps its text when the
//! project retains its source, as a file read by parsing does.

use super::super::*;
use crate::project::session::load_test_file;

#[test]
fn a_context_file_read_from_the_cache_keeps_its_text_when_the_project_retains_source() {
    let text = "<?php function a_cached_context_file_keeps_its_text(): int { return 1; }\n";
    let path = PathBuf::from("cached_context.php");
    // The second load is read from the cache, which the first one filled.
    for _ in 0..2 {
        let mut project = Project::new();
        // Only a project built once reads the per-file cache.
        project.built_once = true;
        project.retain_source = true;
        load_test_file(&mut project, path.clone(), text, false);
        let id = project.files.id_of(&path).expect("loaded");
        let file = project.files.get_by_id(id).expect("loaded");
        assert_eq!(file.source.as_deref(), Some(text));
    }
}
