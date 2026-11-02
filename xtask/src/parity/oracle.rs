//! The oracle side of the scoreboard: the pinned php and phpstan-src, the
//! extractors under `parity/extract/`, and the extraction cache.

use std::fs;
use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;

use sha2::{Digest, Sha256};

use crate::files;

/// The pinned oracle inputs, checked when a run starts.
pub(crate) struct Oracle {
    /// `vendor/phpstan-src`.
    pub(crate) src: PathBuf,
    /// `parity/extract`.
    extract: PathBuf,
    /// `php -v`, which names the build as well as the version.
    php_v: String,
    /// The phpstan-src commit.
    src_commit: String,
    /// SHA-256 of phpstan-src's `vendor/composer/installed.json`: the
    /// composer dependencies as installed.
    installed: String,
    /// `parity/pins.toml`, as bytes for the key and parsed.
    pins: Vec<u8>,
    table: toml::Table,
    /// `<cache>/phpstan-rs/parity`.
    pub(crate) cache: PathBuf,
}

impl Oracle {
    /// Checks the pins against what is installed: the oracle php's version
    /// and the phpstan-src commit.
    pub(crate) fn open(root: &Path) -> Result<Self, String> {
        let pins_path = root.join("parity/pins.toml");
        let pins =
            fs::read(&pins_path).map_err(|error| format!("{}: {error}", pins_path.display()))?;
        let table: toml::Table = std::str::from_utf8(&pins)
            .map_err(|error| format!("{}: {error}", pins_path.display()))?
            .parse()
            .map_err(|error| format!("{}: {error}", pins_path.display()))?;
        let pin = |section: &str, key: &str| pin(&table, section, key);
        let php_version = text(Command::new("php").args(["-r", "echo PHP_VERSION;"]))?;
        let pinned_php = pin("oracle-php", "version")?;
        if php_version != pinned_php {
            return Err(format!(
                "the oracle php is {pinned_php}, but `php` is {php_version}"
            ));
        }
        let src = root.join("vendor/phpstan-src");
        let src_commit = text(
            Command::new("git")
                .arg("-C")
                .arg(&src)
                .args(["rev-parse", "HEAD"]),
        )?;
        let pinned_commit = pin("phpstan-src", "commit")?;
        if src_commit != pinned_commit {
            return Err(format!(
                "vendor/phpstan-src is at {src_commit}, but the pin is {pinned_commit}; run `git submodule update vendor/phpstan-src`"
            ));
        }
        let installed_path = src.join("vendor/composer/installed.json");
        let installed = fs::read(&installed_path).map_err(|error| {
            format!(
                "{}: {error}; run `composer install` in vendor/phpstan-src",
                installed_path.display()
            )
        })?;
        let cache = files::cache_dir()?.join("phpstan-rs/parity");
        Ok(Self {
            extract: root.join("parity/extract"),
            php_v: text(Command::new("php").arg("-v"))?,
            src_commit,
            installed: hex(&Sha256::digest(&installed)),
            pins,
            table,
            src,
            cache,
        })
    }

    /// A value of `parity/pins.toml`.
    pub(crate) fn pin(&self, section: &str, key: &str) -> Result<String, String> {
        pin(&self.table, section, key)
    }

    /// The extraction output of `suite` for `inputs`, from the cache when its
    /// key matches, otherwise by running the first of `scripts` with `args`
    /// and `inputs` on stdin. `scripts` lists every extractor file the run
    /// reads, so each is part of the key.
    pub(crate) fn extract(
        &self,
        suite: &str,
        scripts: &[&str],
        args: &[&str],
        inputs: &[u8],
    ) -> Result<Vec<u8>, String> {
        self.extract_valid(suite, scripts, args, inputs, |_| true)
    }

    /// `extract`, where a cached output that `valid` rejects is produced
    /// again: for an output that names files outside the cache entry.
    pub(crate) fn extract_valid(
        &self,
        suite: &str,
        scripts: &[&str],
        args: &[&str],
        inputs: &[u8],
        valid: impl Fn(&[u8]) -> bool,
    ) -> Result<Vec<u8>, String> {
        let mut extractor = Vec::new();
        for script in scripts {
            let path = self.extract.join(script);
            let bytes = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
            extractor.extend_from_slice(&(script.len() as u64).to_le_bytes());
            extractor.extend_from_slice(script.as_bytes());
            extractor.extend_from_slice(&(bytes.len() as u64).to_le_bytes());
            extractor.extend_from_slice(&bytes);
        }
        let key = cache_key(&KeyInputs {
            extractor: &extractor,
            php_flags: &PHP_FLAGS,
            args,
            php_v: &self.php_v,
            src_commit: &self.src_commit,
            installed: &self.installed,
            pins: &self.pins,
            inputs,
        });
        let [script, ..] = scripts else {
            return Err(format!("suite {suite} names no extractor"));
        };
        let dir = self.cache.join(suite);
        cached(
            &dir.join(&key),
            || self.php(script, args, inputs, &dir.join("work")),
            valid,
        )
    }

    /// Runs the `script` extractor with `args` after the phpstan-src path,
    /// feeding it `stdin`, and returns its stdout. `PARITY_WORK` names
    /// `work`, a directory the extractor may write scratch files to.
    pub(crate) fn php(
        &self,
        script: &str,
        args: &[&str],
        stdin: &[u8],
        work: &Path,
    ) -> Result<Vec<u8>, String> {
        let mut command = Command::new("php");
        command
            .args(PHP_FLAGS)
            .arg(self.extract.join(script))
            .arg(&self.src)
            .args(args)
            .env("PARITY_WORK", work);
        piped(&mut command, stdin)
    }
}

fn pin(table: &toml::Table, section: &str, key: &str) -> Result<String, String> {
    table
        .get(section)
        .and_then(|s| s.get(key))
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("parity/pins.toml has no {section}.{key}"))
}

/// The php settings every extractor runs with, so that the local php.ini and
/// the image's (which has none) give the same run:
/// - `short_open_tag` changes how `<?` lexes; the compiled default is on,
///   and a distribution's php.ini turns it off;
/// - every diagnostic is reported, as the compiled default does, and on
///   stderr: the CLI's compiled default prints it to stdout, into the
///   extraction's output.
const PHP_FLAGS: [&str; 8] = [
    "-d",
    "memory_limit=-1",
    "-d",
    "short_open_tag=1",
    "-d",
    "error_reporting=-1",
    "-d",
    "display_errors=stderr",
];

/// `<dir>/cases.jsonl` if it exists and is `valid`; otherwise `produce`'s output, stored
/// there first.
fn cached(
    dir: &Path,
    produce: impl FnOnce() -> Result<Vec<u8>, String>,
    valid: impl Fn(&[u8]) -> bool,
) -> Result<Vec<u8>, String> {
    let cases = dir.join("cases.jsonl");
    match fs::read(&cases) {
        Ok(bytes) if valid(&bytes) => return Ok(bytes),
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(format!("{}: {error}", cases.display())),
    }
    let output = produce()?;
    fs::create_dir_all(dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    files::write_atomically(&cases, &output)?;
    Ok(output)
}

/// Every input an extraction's output depends on. The cache key is built
/// from all of its fields, so a new input can't be left out of the key
/// without changing this struct.
struct KeyInputs<'a> {
    extractor: &'a [u8],
    php_flags: &'a [&'a str],
    args: &'a [&'a str],
    php_v: &'a str,
    src_commit: &'a str,
    installed: &'a str,
    pins: &'a [u8],
    inputs: &'a [u8],
}

fn cache_key(inputs: &KeyInputs<'_>) -> String {
    let KeyInputs {
        extractor,
        php_flags,
        args,
        php_v,
        src_commit,
        installed,
        pins,
        inputs,
    } = inputs;
    let mut hasher = Sha256::new();
    let mut field = |bytes: &[u8]| {
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    };
    field(extractor);
    for list in [*php_flags, *args] {
        field(&(list.len() as u64).to_le_bytes());
        for item in list {
            field(item.as_bytes());
        }
    }
    field(php_v.as_bytes());
    field(src_commit.as_bytes());
    field(installed.as_bytes());
    field(pins);
    field(inputs);
    hex(&hasher.finalize())
}

/// Lower-case hexadecimal.
pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

/// Runs `command` with `stdin` written from another thread, so neither side
/// blocks on a full pipe, and returns its stdout. A non-zero exit is an
/// error carrying stderr.
pub(crate) fn piped(command: &mut Command, stdin: &[u8]) -> Result<Vec<u8>, String> {
    let name = format!("{command:?}");
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("{name}: {error}"))?;
    let mut pipe = child.stdin.take().expect("stdin is piped");
    let input = stdin.to_vec();
    // A child that stops reading early closes the pipe; that is not an error.
    let writer = thread::spawn(move || match pipe.write_all(&input) {
        Err(error) if error.kind() != ErrorKind::BrokenPipe => Err(error),
        _ => Ok(()),
    });
    let output = child
        .wait_with_output()
        .map_err(|error| format!("{name}: {error}"))?;
    writer
        .join()
        .expect("the stdin writer does not panic")
        .map_err(|error| format!("{name}: writing stdin: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "{name} failed ({}):\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(output.stdout)
}

/// The trimmed stdout of a command that must succeed.
pub(crate) fn text(command: &mut Command) -> Result<String, String> {
    let stdout = piped(command, &[])?;
    Ok(String::from_utf8_lossy(&stdout).trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_with(change: usize) -> String {
        let base = |i: usize, value: &'static str| if i == change { "changed" } else { value };
        let args = [base(1, "arg")];
        let flags = [base(7, "-d")];
        cache_key(&KeyInputs {
            extractor: base(0, "script").as_bytes(),
            php_flags: &flags,
            args: &args,
            php_v: base(2, "PHP 8.5.4"),
            src_commit: base(3, "commit"),
            installed: base(4, "installed"),
            pins: base(5, "pins").as_bytes(),
            inputs: base(6, "inputs").as_bytes(),
        })
    }

    #[test]
    fn every_key_input_changes_the_key() {
        let unchanged = key_with(usize::MAX);
        for field in 0..8 {
            assert_ne!(
                key_with(field),
                unchanged,
                "field {field} is not in the key"
            );
        }
    }

    #[test]
    fn fields_are_length_prefixed() {
        let one = cache_key(&KeyInputs {
            extractor: b"ab",
            php_flags: &[],
            args: &[],
            php_v: "c",
            src_commit: "",
            installed: "",
            pins: b"",
            inputs: b"",
        });
        let two = cache_key(&KeyInputs {
            extractor: b"a",
            php_flags: &[],
            args: &[],
            php_v: "bc",
            src_commit: "",
            installed: "",
            pins: b"",
            inputs: b"",
        });
        assert_ne!(one, two);
    }

    #[test]
    fn a_warm_extraction_reads_the_cache_and_writes_nothing() {
        let scratch = files::cache_dir()
            .unwrap()
            .join("phpstan-rs/test-scratch")
            .join(format!("cached-{}", std::process::id()));
        let dir = scratch.join("key");
        let cold = cached(&dir, || Ok(b"cases".to_vec()), |_| true).unwrap();
        assert_eq!(cold, b"cases");
        let written = fs::metadata(dir.join("cases.jsonl"))
            .unwrap()
            .modified()
            .unwrap();
        let listing = |dir: &Path| {
            let mut names: Vec<_> = fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            names.sort();
            names
        };
        let before = listing(&dir);
        let warm = cached(&dir, || panic!("a warm run must not extract"), |_| true).unwrap();
        assert_eq!(warm, b"cases");
        assert_eq!(listing(&dir), before);
        assert_eq!(
            fs::metadata(dir.join("cases.jsonl"))
                .unwrap()
                .modified()
                .unwrap(),
            written
        );
        let redone = cached(&dir, || Ok(b"again".to_vec()), |bytes| bytes != b"cases").unwrap();
        assert_eq!(redone, b"again");
        assert_eq!(fs::read(dir.join("cases.jsonl")).unwrap(), b"again");
        fs::remove_dir_all(&scratch).unwrap();
    }

    #[test]
    fn hex_is_lower_case_and_padded() {
        assert_eq!(hex(&[0x00, 0x0f, 0xab]), "000fab");
    }
}
