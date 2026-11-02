//! `cargo xtask port-map`: keeps `parity/port-map.tsv` in step with
//! `vendor/phpstan-src/src`, one row per PHPStan PHP file.
//!
//! Each row has the PHPStan path, the Rust module that ports it, its status
//! (`todo`, `ported` or `wontport`) and a note (the reason, for `wontport`).
//! A run adds rows for new files and drops rows for removed ones, and keeps
//! what the other columns already say. `--check` fails instead of writing
//! when the file is out of date.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const HEADER: &str = "phpstan_file\trust_module\tstatus\tnote";
const STATUSES: [&str; 3] = ["todo", "ported", "wontport"];

pub(crate) fn run(root: &Path, check: bool) -> Result<(), String> {
    let src = root.join("vendor/phpstan-src/src");
    let map_path = root.join("parity/port-map.tsv");
    let mut files = Vec::new();
    collect_php_files(&src, &src, &mut files)?;
    if files.is_empty() {
        return Err(format!(
            "no PHP files under {}; run `git submodule update --init vendor/phpstan-src`",
            src.display()
        ));
    }
    let existing = match fs::read_to_string(&map_path) {
        Ok(text) => parse(&text)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
        Err(error) => return Err(format!("{}: {error}", map_path.display())),
    };
    let rendered = render(&files, &existing);
    let current = fs::read_to_string(&map_path).unwrap_or_default();
    if rendered == current {
        println!("port-map: {} files, up to date", files.len());
        return Ok(());
    }
    if check {
        return Err("parity/port-map.tsv is out of date; run `cargo xtask port-map`".to_owned());
    }
    write_atomically(&map_path, &rendered)?;
    println!("port-map: {} files written", files.len());
    Ok(())
}

/// Collects every `.php` file below `dir` as a `/`-separated path relative
/// to `base`. Directory entries are sorted, so the result is in byte order.
fn collect_php_files(base: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), String> {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|error| format!("{}: {error}", dir.display()))?
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<_, _>>()
        .map_err(|error| format!("{}: {error}", dir.display()))?;
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect_php_files(base, &path, out)?;
        } else if path.extension().is_some_and(|ext| ext == "php") {
            let relative = path.strip_prefix(base).expect("walked below base");
            let parts: Vec<_> = relative
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect();
            out.push(parts.join("/"));
        }
    }
    Ok(())
}

/// The non-path columns of one row.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    rust_module: String,
    status: String,
    note: String,
}

fn parse(text: &str) -> Result<BTreeMap<String, Row>, String> {
    let mut rows = BTreeMap::new();
    for (index, line) in text.lines().enumerate().skip(1) {
        let fields: Vec<&str> = line.split('\t').collect();
        let [file, rust_module, status, note] = fields[..] else {
            return Err(format!(
                "port-map.tsv line {}: expected 4 columns",
                index + 1
            ));
        };
        if !STATUSES.contains(&status) {
            return Err(format!(
                "port-map.tsv line {}: unknown status {status:?}",
                index + 1
            ));
        }
        if status == "wontport" && note.is_empty() {
            return Err(format!(
                "port-map.tsv line {}: `wontport` needs a reason",
                index + 1
            ));
        }
        rows.insert(
            file.to_owned(),
            Row {
                rust_module: rust_module.to_owned(),
                status: status.to_owned(),
                note: note.to_owned(),
            },
        );
    }
    Ok(rows)
}

fn render(files: &[String], existing: &BTreeMap<String, Row>) -> String {
    let mut out = String::with_capacity(files.len() * 64);
    out.push_str(HEADER);
    out.push('\n');
    for file in files {
        let todo = Row {
            rust_module: String::new(),
            status: "todo".to_owned(),
            note: String::new(),
        };
        let row = existing.get(file).unwrap_or(&todo);
        out.push_str(&format!(
            "{file}\t{}\t{}\t{}\n",
            row.rust_module, row.status, row.note
        ));
    }
    out
}

/// Writes `text` to a temporary file beside `path`, then renames it over
/// `path`, so a failed write never leaves a truncated file.
fn write_atomically(path: &Path, text: &str) -> Result<(), String> {
    let temporary = path.with_extension("tsv.tmp");
    fs::write(&temporary, text).map_err(|error| format!("{}: {error}", temporary.display()))?;
    fs::rename(&temporary, path).map_err(|error| format!("{}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_keeps_existing_rows_and_adds_new_files_as_todo() {
        let text = "phpstan_file\trust_module\tstatus\tnote\nA.php\tsyntax::a\tported\t\nGone.php\t\ttodo\t\n";
        let existing = parse(text).unwrap();
        let rendered = render(&["A.php".to_owned(), "B.php".to_owned()], &existing);
        assert_eq!(
            rendered,
            "phpstan_file\trust_module\tstatus\tnote\nA.php\tsyntax::a\tported\t\nB.php\t\ttodo\t\n"
        );
    }

    #[test]
    fn wontport_without_a_reason_is_rejected() {
        let text = "phpstan_file\trust_module\tstatus\tnote\nA.php\t\twontport\t\n";
        assert!(parse(text).is_err());
    }
}
