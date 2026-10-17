//! What a load keeps of a file's text: the buffer a worker reads each file
//! into, the source a file keeps (only a file whose tree is kept, and only in
//! a project that retains source), and the length a file is weighed by.

use super::super::*;
use crate::project::session::{OverlayFile, Session, load_test_file, load_test_files};
use crate::project::source::FileSource;
use crate::registry::stub_set::Stubs;

const CONTEXT: &str = "<?php namespace Lib; function helper(int $n): int { return $n; }";
const APP: &str = "<?php function app(): void {}";

#[test]
fn a_read_fills_the_buffer_with_the_file_alone() {
    let mut source = FileSource::default();
    let (long, short) = (Path::new("/w/long.php"), Path::new("/w/short.php"));
    source.set(
        long,
        b"<?php function a_longer_file_than_the_next_one() { return 1; }",
    );
    source.set(short, b"<?php 1;");
    let mut buf = Vec::new();
    source.read_into(long, &mut buf).expect("reads");
    source.read_into(short, &mut buf).expect("reads");
    assert_eq!(buf, b"<?php 1;");
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    source
        .read_into(&manifest, &mut buf)
        .expect("reads the disk");
    assert_eq!(buf, std::fs::read(&manifest).expect("reads"));
    assert!(
        source
            .read_into(Path::new("/no/such/dir/file.php"), &mut buf)
            .is_err()
    );
}

#[test]
fn a_context_file_keeps_its_source_while_its_tree_is_kept() {
    let mut project = Project::new();
    project.retain_source = true;
    load_test_file(&mut project, PathBuf::from("ctx.php"), CONTEXT, false);
    assert_eq!(project.source(Path::new("ctx.php")), Some(CONTEXT));
}

#[test]
fn a_context_file_whose_tree_is_dropped_keeps_no_source_and_an_analysed_one_keeps_its() {
    let mut project = Project::new();
    project.retain_source = true;
    project.built_once = true;
    load_test_file(&mut project, PathBuf::from("ctx.php"), CONTEXT, false);
    load_test_file(&mut project, PathBuf::from("app.php"), APP, true);
    assert_eq!(project.source(Path::new("ctx.php")), None);
    assert_eq!(project.source(Path::new("app.php")), Some(APP));
}

#[test]
fn a_batch_file_is_weighed_by_its_length_and_keeps_no_source() {
    let long = "<?php function a_long_function() { return 1; } function another() {}";
    let short = "<?php 1;";
    let mut project = Project::new();
    load_test_files(
        &mut project,
        &[
            (PathBuf::from("long.php"), long, false),
            (PathBuf::from("short.php"), short, false),
        ],
    );
    for (path, len) in [("long.php", long.len()), ("short.php", short.len())] {
        let file = project.files.get(Path::new(path)).expect("loaded");
        assert_eq!(file.source_len, len, "{path}");
        assert_eq!(project.source(Path::new(path)), None, "{path}");
    }
}

#[test]
fn a_file_that_is_not_utf8_is_parsed_from_its_bytes_and_weighed_by_its_lossy_length() {
    let bytes = b"<?php function from_bad_bytes() { return '\xff'; }".to_vec();
    let file = OverlayFile {
        path: PathBuf::from("bad.php"),
        bytes: bytes.clone(),
        analysed: false,
        stub: false,
    };
    let loaded = Session::for_overlay(vec![file], Stubs::shared().clone())
        .load()
        .expect("loads");
    let file = loaded
        .project
        .files
        .get(Path::new("bad.php"))
        .expect("loaded");
    assert_eq!(file.source_len, String::from_utf8_lossy(&bytes).len());
    assert_eq!(file.functions.len(), 1);
}
