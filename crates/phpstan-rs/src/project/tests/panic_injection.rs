//! A test-only hook that makes checking panic on
//! chosen files, so a test can check what a checker panic costs.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

static INJECTED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// Makes checking panic when it reaches `path`. Tests run in
/// parallel, so each test injects a path no other test loads.
pub(in crate::project) fn inject_panic(path: &Path) {
    INJECTED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(path.to_path_buf());
}

pub(in crate::project) fn panic_if_injected(path: &Path) {
    let injected = INJECTED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .any(|p| p == path);
    if injected {
        panic!("injected panic for {}", path.display());
    }
}
