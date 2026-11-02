//! The pinned inputs fetched from outside the repository: nikic's parser
//! fixtures, phpdoc-parser's tests, php-src's `.phpt` tests, the PHPStan
//! package, composer and the end-to-end projects. Each is fetched once into
//! `<cache>/pins/<name>-<id>`, checked against its pin, and then reused.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

use super::oracle::{Oracle, hex, text};

/// nikic/PHP-Parser's repository at the pinned commit.
pub(crate) fn nikic(oracle: &Oracle) -> Result<PathBuf, String> {
    repository(oracle, "nikic-php-parser")
}

/// phpstan/phpdoc-parser's repository at the pinned commit, which must be
/// the one composer installed in phpstan-src.
pub(crate) fn phpdoc_parser(oracle: &Oracle) -> Result<PathBuf, String> {
    let commit = oracle.pin("phpdoc-parser", "commit")?;
    let installed = oracle.installed_reference("phpstan/phpdoc-parser")?;
    if installed != commit {
        return Err(format!(
            "phpstan-src installed phpstan/phpdoc-parser at {installed}, but the pin is {commit}"
        ));
    }
    repository(oracle, "phpdoc-parser")
}

/// The git repository of the `section` pin at its commit, fetched alone.
fn repository(oracle: &Oracle, section: &str) -> Result<PathBuf, String> {
    let repository = oracle.pin(section, "repository")?;
    let commit = oracle.pin(section, "commit")?;
    fetched(oracle, &format!("{section}-{commit}"), |dir| {
        let git = |args: &[&str]| -> Result<String, String> {
            text(Command::new("git").arg("-C").arg(dir).args(args))
        };
        fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
        git(&["init", "--quiet"])?;
        git(&["fetch", "--quiet", "--depth", "1", &repository, &commit])?;
        git(&["checkout", "--quiet", "FETCH_HEAD"])?;
        let head = git(&["rev-parse", "HEAD"])?;
        if head != commit {
            return Err(format!(
                "{repository} fetched {head}, not the pinned {commit}"
            ));
        }
        Ok(())
    })
}

/// The PHPStan phar in the phpstan/phpstan package at the pinned commit,
/// beside the turbo extension's binaries, checked against the phar's pinned
/// SHA-256.
pub(crate) fn phar(oracle: &Oracle) -> Result<PathBuf, String> {
    let sha256 = oracle.pin("phpstan-phar", "sha256")?;
    let phar = repository(oracle, "phpstan")?.join("phpstan.phar");
    let bytes = fs::read(&phar).map_err(|error| format!("{}: {error}", phar.display()))?;
    let got = hex(&Sha256::digest(&bytes));
    if got != sha256 {
        return Err(format!(
            "{} has SHA-256 {got}, not the pinned {sha256}",
            phar.display()
        ));
    }
    Ok(phar)
}

/// The pinned composer phar.
fn composer(oracle: &Oracle) -> Result<PathBuf, String> {
    let url = oracle.pin("composer", "url")?;
    let sha256 = oracle.pin("composer", "sha256")?;
    let dir = fetched(oracle, &format!("composer-{sha256}"), |dir| {
        fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
        let phar = dir.join("composer.phar");
        text(
            Command::new("curl")
                .args(["-sSfL", "-o"])
                .arg(&phar)
                .arg(&url),
        )?;
        let bytes = fs::read(&phar).map_err(|error| format!("{}: {error}", phar.display()))?;
        let got = hex(&Sha256::digest(&bytes));
        if got != sha256 {
            return Err(format!("{url} has SHA-256 {got}, not the pinned {sha256}"));
        }
        Ok(())
    })?;
    Ok(dir.join("composer.phar"))
}

/// An end-to-end project: the `e2e-<name>` pin's repository at its commit,
/// with its dependencies installed from `parity/e2e/<name>/composer.lock`,
/// without dev ones. The project's `composer.json` stays as it is; composer
/// reads a copy, `parity-composer.json`, with `config.lock` on, since a
/// project may turn the lock off.
pub(crate) fn project(oracle: &Oracle, name: &str) -> Result<PathBuf, String> {
    let section = format!("e2e-{name}");
    let lock_path = oracle
        .parity_dir()
        .join("e2e")
        .join(name)
        .join("composer.lock");
    let lock = fs::read(&lock_path).map_err(|error| format!("{}: {error}", lock_path.display()))?;
    let checkout = repository(oracle, &section)?;
    let composer = composer(oracle)?;
    let commit = oracle.pin(&section, "commit")?;
    let lock_sha = hex(&Sha256::digest(&lock));
    fetched(oracle, &format!("{section}-{commit}-{lock_sha}"), |dir| {
        copy_tree(&checkout, dir)?;
        let manifest = dir.join("composer.json");
        let json =
            fs::read(&manifest).map_err(|error| format!("{}: {error}", manifest.display()))?;
        let mut json: serde_json::Value = serde_json::from_slice(&json)
            .map_err(|error| format!("{}: {error}", manifest.display()))?;
        let config = json
            .as_object_mut()
            .ok_or_else(|| format!("{} is not an object", manifest.display()))?
            .entry("config")
            .or_insert_with(|| serde_json::json!({}));
        config
            .as_object_mut()
            .ok_or_else(|| format!("{}: config is not an object", manifest.display()))?
            .insert("lock".to_owned(), serde_json::Value::Bool(true));
        let copy = serde_json::to_vec_pretty(&json).map_err(|error| error.to_string())?;
        let write = |path: PathBuf, bytes: &[u8]| {
            fs::write(&path, bytes).map_err(|error| format!("{}: {error}", path.display()))
        };
        write(dir.join("parity-composer.json"), &copy)?;
        write(dir.join("parity-composer.lock"), &lock)?;
        text(
            Command::new("php")
                .arg(&composer)
                .args([
                    "install",
                    "--no-dev",
                    "--no-scripts",
                    "--no-plugins",
                    "--no-progress",
                    "--no-interaction",
                ])
                .current_dir(dir)
                .env("COMPOSER", "parity-composer.json"),
        )
        .map(drop)
    })
}

/// Copies the files of `from` into a new `to`, leaving out `.git`.
fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|error| format!("{}: {error}", to.display()))?;
    let entries = fs::read_dir(from).map_err(|error| format!("{}: {error}", from.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("{}: {error}", from.display()))?;
        let name = entry.file_name();
        if name == ".git" {
            continue;
        }
        let kind = entry
            .file_type()
            .map_err(|error| format!("{}: {error}", entry.path().display()))?;
        let target = to.join(&name);
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            fs::copy(entry.path(), &target)
                .map_err(|error| format!("{}: {error}", target.display()))?;
        }
    }
    Ok(())
}

/// php-src's release tarball, unpacked.
pub(crate) fn php_src(oracle: &Oracle) -> Result<PathBuf, String> {
    let url = oracle.pin("php-src", "url")?;
    let sha256 = oracle.pin("php-src", "sha256")?;
    fetched(oracle, &format!("php-src-{sha256}"), |dir| {
        let tarball = dir.with_extension("tar.gz");
        text(
            Command::new("curl")
                .args(["-sSfL", "-o"])
                .arg(&tarball)
                .arg(&url),
        )?;
        let bytes =
            fs::read(&tarball).map_err(|error| format!("{}: {error}", tarball.display()))?;
        let got = hex(&Sha256::digest(&bytes));
        if got != sha256 {
            return Err(format!("{url} has SHA-256 {got}, not the pinned {sha256}"));
        }
        fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
        text(
            Command::new("tar")
                .args(["xzf"])
                .arg(&tarball)
                .args(["--strip-components=1", "-C"])
                .arg(dir),
        )?;
        fs::remove_file(&tarball).map_err(|error| format!("{}: {error}", tarball.display()))
    })
}

/// `<cache>/pins/<name>`, made by `fetch` into a temporary directory and
/// renamed once complete, so an interrupted fetch never looks finished.
fn fetched(
    oracle: &Oracle,
    name: &str,
    fetch: impl FnOnce(&Path) -> Result<(), String>,
) -> Result<PathBuf, String> {
    let pins = oracle.cache.join("pins");
    let done = pins.join(name);
    oracle.keep(done.clone());
    if done.is_dir() {
        return Ok(done);
    }
    let partial = pins.join(format!("{name}.partial"));
    if partial.exists() {
        fs::remove_dir_all(&partial).map_err(|error| format!("{}: {error}", partial.display()))?;
    }
    fs::create_dir_all(&pins).map_err(|error| format!("{}: {error}", pins.display()))?;
    fetch(&partial)?;
    fs::rename(&partial, &done).map_err(|error| format!("{}: {error}", done.display()))?;
    Ok(done)
}
