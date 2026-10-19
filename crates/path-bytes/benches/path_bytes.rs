//! What reading a path from its bytes costs against asking `Path`, over paths
//! shaped as a load's: absolute, below a project's root, a few directories
//! deep, ending in a PHP file. A benchmark runs one function over every path
//! of the mix, so one call takes its time over `PATHS`. `unusual` is a mix the
//! bytes leave to `Path`, which costs what looking at them first does.

use std::ffi::OsStr;
use std::hint::black_box;
use std::path::{Component, Path, PathBuf};

use criterion::{Criterion, Throughput, criterion_group, criterion_main};

const PATHS: usize = 1000;

#[cfg(windows)]
const SEPARATOR: &str = "\\";
#[cfg(not(windows))]
const SEPARATOR: &str = "/";

#[cfg(windows)]
const ROOT: &str = "C:\\work\\project";
#[cfg(not(windows))]
const ROOT: &str = "/home/user/project";

/// The next number below `bound` of a repeatable sequence.
fn next(state: &mut u64, bound: usize) -> usize {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    ((*state >> 33) as usize) % bound
}

fn joined(parts: &[&str]) -> PathBuf {
    PathBuf::from(parts.join(SEPARATOR))
}

/// Paths as a project's load carries them: files of a package below
/// `vendor`, and files of the project itself.
fn load_paths() -> Vec<PathBuf> {
    let vendors = ["acme", "symfony", "monolog", "league", "nikic"];
    let packages = ["console", "log", "flysystem", "php-parser", "http-kernel"];
    let directories = [
        "src",
        "Command",
        "Handler",
        "Node",
        "Expr",
        "Resources",
        "Tests",
    ];
    let files = [
        "Application.php",
        "Logger.php",
        "Filesystem.php",
        "Parser.php",
        "Kernel.php",
        "helpers.php",
        "composer.json",
    ];
    let mut state = 0x2545_F491_4F6C_DD1Du64;
    (0..PATHS)
        .map(|_| {
            let mut parts = vec![ROOT];
            if next(&mut state, 3) != 0 {
                parts.extend(["vendor", vendors[next(&mut state, 5)]]);
                parts.push(packages[next(&mut state, 5)]);
            }
            for _ in 0..1 + next(&mut state, 4) {
                parts.push(directories[next(&mut state, 7)]);
            }
            parts.push(files[next(&mut state, 7)]);
            joined(&parts)
        })
        .collect()
}

/// Paths the bytes leave to `Path`: a `.` component, a doubled separator.
fn unusual_paths() -> Vec<PathBuf> {
    load_paths()
        .into_iter()
        .enumerate()
        .map(|(at, path)| {
            let mut text = path.to_string_lossy().into_owned();
            let separator = text.rfind(SEPARATOR).unwrap();
            let extra = if at % 2 == 0 {
                SEPARATOR.to_string()
            } else {
                format!("{SEPARATOR}.")
            };
            text.insert_str(separator, &extra);
            PathBuf::from(text)
        })
        .collect()
}

/// Runs `std` and `bytes` over every path, as two benchmarks of one group.
fn compare<T>(
    c: &mut Criterion,
    group: &str,
    paths: &[PathBuf],
    std: impl Fn(&Path) -> T,
    bytes: impl Fn(&Path) -> T,
) {
    let mut group = c.benchmark_group(group);
    group.throughput(Throughput::Elements(paths.len() as u64));
    group.bench_function("path", |b| {
        b.iter(|| {
            for path in paths {
                black_box(std(black_box(path)));
            }
        })
    });
    group.bench_function("bytes", |b| {
        b.iter(|| {
            for path in paths {
                black_box(bytes(black_box(path)));
            }
        })
    });
    group.finish();
}

/// What a pass over a path's components and over its ancestors costs from
/// `Path`.
fn std_components_and_ancestors(path: &Path) -> usize {
    let components: usize = path.components().map(|c| c.as_os_str().len()).sum();
    let ancestors: usize = path
        .ancestors()
        .map(|a| a.as_os_str().len() + a.file_name().map_or(0, OsStr::len))
        .sum();
    components + ancestors
}

/// The same pass from the bytes, and from `Path` where they do not tell, as
/// the hash of a path's ancestors is made.
fn bytes_components_and_ancestors(path: &Path) -> usize {
    match path_bytes::components(path) {
        Some(components) => components
            .map(|(component, upto)| component.as_os_str().len() + upto.as_os_str().len())
            .sum(),
        None => std_components_and_ancestors(path),
    }
}

fn std_is_normal_absolute(path: &Path) -> bool {
    path.is_absolute()
        && !path
            .components()
            .any(|c| matches!(c, Component::CurDir | Component::ParentDir))
}

fn bench(c: &mut Criterion) {
    let paths = load_paths();
    let root = Path::new(ROOT);
    compare(
        c,
        "file_name",
        &paths,
        |p| p.file_name().map(OsStr::len),
        |p| path_bytes::file_name(p).map(OsStr::len),
    );
    compare(
        c,
        "parent",
        &paths,
        |p| p.parent().map(|d| d.as_os_str().len()),
        |p| path_bytes::parent(p).map(|d| d.as_os_str().len()),
    );
    compare(
        c,
        "split",
        &paths,
        |p| {
            p.parent()
                .zip(p.file_name())
                .map(|(d, n)| d.as_os_str().len() + n.len())
        },
        |p| path_bytes::split(p).map(|(d, n)| d.as_os_str().len() + n.len()),
    );
    compare(
        c,
        "has_file_name",
        &paths,
        |p| p.file_name() == Some(OsStr::new("composer.json")),
        |p| path_bytes::has_file_name(p, "composer.json"),
    );
    compare(
        c,
        "starts_with",
        &paths,
        |p| p.starts_with(root),
        |p| path_bytes::starts_with(p, root),
    );
    compare(
        c,
        "strip_prefix",
        &paths,
        |p| p.strip_prefix(root).ok().map(|r| r.as_os_str().len()),
        |p| path_bytes::strip_prefix(p, root).map(|r| r.as_os_str().len()),
    );
    compare(
        c,
        "is_normal_absolute",
        &paths,
        std_is_normal_absolute,
        path_bytes::is_normal_absolute,
    );
    compare(
        c,
        "components_and_ancestors",
        &paths,
        std_components_and_ancestors,
        bytes_components_and_ancestors,
    );

    let unusual = unusual_paths();
    compare(
        c,
        "unusual/parent",
        &unusual,
        |p| p.parent().map(|d| d.as_os_str().len()),
        |p| path_bytes::parent(p).map(|d| d.as_os_str().len()),
    );
    compare(
        c,
        "unusual/strip_prefix",
        &unusual,
        |p| p.strip_prefix(root).ok().map(|r| r.as_os_str().len()),
        |p| path_bytes::strip_prefix(p, root).map(|r| r.as_os_str().len()),
    );
    compare(
        c,
        "unusual/components_and_ancestors",
        &unusual,
        std_components_and_ancestors,
        bytes_components_and_ancestors,
    );
}

criterion_group!(benches, bench);
criterion_main!(benches);
