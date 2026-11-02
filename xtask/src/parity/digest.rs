//! The suites whose cases compare a digest of each input's whole output,
//! plus a count and the errors: the lexer (nikic's token stream, also with
//! the line breaks rewritten), the parser (nikic's AST after name
//! resolution, as PHPStan parses a file) and phpdoc (phpdoc-parser's tree of
//! each distinct doc comment, as PHPStan parses one).
//!
//! Both read the same inputs, one `<case id>\t<file>` line each: phpstan-src's
//! own PHP files, nikic's parser fixtures, php-src's `.phpt` tests and the
//! generated operator pairs, the last three materialized into files by
//! `inputs.php`.

use std::path::Path;
use std::process::Command;

use serde_json::Value;

use super::jsonl::{by_id, by_id_values, parse_lines, unported};
use super::oracle::{Oracle, piped};
use super::pins;
use super::score::{Case, Verdict};
use crate::files;

/// One digest suite.
pub(crate) struct Suite {
    pub(crate) name: &'static str,
    /// The extractor under `parity/extract/`.
    extractor: &'static str,
    /// The fields of a case that must be equal, in the order they are checked.
    fields: &'static [&'static str],
    /// Whether the cases are the doc comments the extractor's `--list` finds
    /// in the inputs, rather than the inputs themselves.
    docs: bool,
}

pub(crate) const LEXER: Suite = Suite {
    name: "lexer",
    extractor: "lexer.php",
    fields: &["tokens", "sha256", "errors"],
    docs: false,
};

pub(crate) const PARSER: Suite = Suite {
    name: "parser",
    extractor: "parser.php",
    fields: &["errors", "nodes", "sha256"],
    docs: false,
};

pub(crate) const PHPDOC: Suite = Suite {
    name: "phpdoc",
    extractor: "phpdoc.php",
    fields: &["error", "nodes", "sha256", "printed"],
    docs: true,
};

/// The inputs, one `<case id>\t<file>` line each, in byte order:
/// - every `.php` file under phpstan-src's `tests` and `vendor`, its id its
///   `/`-spelled path there;
/// - the code sections of nikic's fixtures, the FILE sections of php-src's
///   tests and the operator pairs, from `inputs.php`'s manifest.
fn inputs(oracle: &Oracle) -> Result<Vec<u8>, String> {
    let mut lines = Vec::new();
    for dir in ["tests", "vendor"] {
        for id in files::php_files(&oracle.src, &oracle.src.join(dir))? {
            let file = oracle.src.join(&id);
            let file = file
                .to_str()
                .ok_or_else(|| format!("{} is not UTF-8", file.display()))?;
            lines.push(format!("{id}\t{file}\n").into_bytes());
        }
    }
    let manifest = materialized(oracle)?;
    if let Some(line) = std::str::from_utf8(&manifest)
        .map_err(|error| format!("the inputs manifest is not UTF-8: {error}"))?
        .lines()
        .find(|line| !line.contains('\t'))
    {
        return Err(format!(
            "the inputs manifest has a line without a file: {line}"
        ));
    }
    lines.extend(
        manifest
            .split_inclusive(|&byte| byte == b'\n')
            .map(<[u8]>::to_vec),
    );
    lines.sort_unstable();
    Ok(lines.concat())
}

/// `inputs.php`'s manifest, produced again if a file it names is gone.
fn materialized(oracle: &Oracle) -> Result<Vec<u8>, String> {
    let nikic = pins::nikic(oracle)?;
    let php_src = pins::php_src(oracle)?;
    let store = oracle.cache.join("inputs/store");
    let path = |p: &Path| {
        p.to_str()
            .map(str::to_owned)
            .ok_or_else(|| format!("{} is not UTF-8", p.display()))
    };
    let args = [path(&nikic)?, path(&php_src)?, path(&store)?];
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    oracle.extract_valid("inputs", &["inputs.php"], &args, b"", |manifest| {
        manifest_files(manifest).all(|file| Path::new(file).is_file())
    })
}

fn manifest_files(manifest: &[u8]) -> impl Iterator<Item = &str> {
    std::str::from_utf8(manifest)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.split('\t').nth(1))
}

/// The suite's input lines: the inputs, or for phpdoc the doc comments in
/// them, one `<case id>\t<file>\t<offset>\t<length>` line each.
fn suite_inputs(suite: &Suite, oracle: &Oracle) -> Result<Vec<u8>, String> {
    let inputs = inputs(oracle)?;
    if !suite.docs {
        return Ok(inputs);
    }
    oracle.extract_valid(
        "phpdoc-docs",
        &[suite.extractor],
        &["--list"],
        &inputs,
        |docs| manifest_files(docs).all(|file| Path::new(file).is_file()),
    )
}

pub(crate) fn run(suite: &Suite, oracle: &Oracle, binary: &Path) -> Result<Vec<Case>, String> {
    let inputs = suite_inputs(suite, oracle)?;
    let expected = by_id(&oracle.extract(suite.name, &[suite.extractor], &[], &inputs)?)?;
    let output = piped(Command::new(binary).args(["debug", suite.name]), &inputs)?;
    let actual = parse_lines(&output)?;
    if let Some(reason) = unported(suite.name, &actual) {
        return Ok(expected
            .into_keys()
            .map(|id| Case {
                group: group(&id),
                id,
                verdict: Verdict::Fail(reason.clone()),
            })
            .collect());
    }
    let mut actual = by_id_values(actual)?;
    Ok(expected
        .into_iter()
        .map(|(id, want)| {
            let verdict = match actual.remove(&id) {
                None => Verdict::Error("phpstan-rs printed nothing for this case".to_owned()),
                Some(got) => compare(suite, &want, &got),
            };
            Case {
                group: group(&id),
                id,
                verdict,
            }
        })
        .collect())
}

/// The first component of the id: `tests`, `vendor`, `nikic`, `php-src` or
/// `operators`.
fn group(id: &str) -> String {
    id.split('/').next().unwrap_or_default().to_owned()
}

fn compare(suite: &Suite, want: &Value, got: &Value) -> Verdict {
    for &field in suite.fields {
        if want.get(field) != got.get(field) {
            return Verdict::Fail(format!(
                "{field}: expected {}, got {}",
                want.get(field).unwrap_or(&Value::Null),
                got.get(field).unwrap_or(&Value::Null)
            ));
        }
    }
    Verdict::Pass
}

/// `--show <id>`: prints the first line where the two sides' dumps differ.
/// Both sides get the id and the rest of its input line as arguments. A
/// lexer id may end in `#crlf` or `#cr`, naming its rewritten variant.
pub(crate) fn show(suite: &Suite, oracle: &Oracle, binary: &Path, id: &str) -> Result<(), String> {
    let input = id
        .strip_suffix("#crlf")
        .or_else(|| id.strip_suffix("#cr"))
        .unwrap_or(id);
    let inputs = suite_inputs(suite, oracle)?;
    let rest = std::str::from_utf8(&inputs)
        .map_err(|error| format!("the inputs are not UTF-8: {error}"))?
        .lines()
        .find_map(|line| line.strip_prefix(input)?.strip_prefix('\t'))
        .ok_or_else(|| format!("no input has the id {input}"))?;
    let mut args = vec!["--show", id];
    args.extend(rest.split('\t'));
    let work = oracle.cache.join(suite.name).join("work");
    let want = oracle.php(suite.extractor, &args, &[], &work)?;
    let got = piped(
        Command::new(binary).args(["debug", suite.name]).args(&args),
        &[],
    )?;
    let want = String::from_utf8_lossy(&want);
    let got = String::from_utf8_lossy(&got);
    if let Some(reason) = got
        .lines()
        .next()
        .and_then(|line| unported(suite.name, &[serde_json::from_str(line).ok()?]))
    {
        println!("{reason}; the expected dump starts:");
        for line in want.lines().take(20) {
            println!("  {line}");
        }
        return Ok(());
    }
    let mismatch = want
        .lines()
        .map(Some)
        .chain(std::iter::repeat(None))
        .zip(got.lines().map(Some).chain(std::iter::repeat(None)))
        .enumerate()
        .take_while(|(_, pair)| *pair != (None, None))
        .find(|(_, (w, g))| w != g);
    match mismatch {
        None => println!("{id}: the dumps are equal"),
        Some((index, (w, g))) => println!(
            "{id}: line {} differs\n  expected: {}\n  got:      {}",
            index + 1,
            w.unwrap_or("(end)"),
            g.unwrap_or("(end)")
        ),
    }
    Ok(())
}
