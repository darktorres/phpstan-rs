//! The pinned inputs fetched from outside the repository: nikic's parser
//! fixtures, phpdoc-parser's tests, php-src's `.phpt` tests and the PHPStan
//! package. Each is fetched once into
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
