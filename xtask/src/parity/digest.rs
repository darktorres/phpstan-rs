//! The suites whose cases compare a digest of each input's whole output,
//! plus a count and the errors: the lexer (nikic's token stream, also with
//! the line breaks rewritten) and the parser (nikic's AST after name
//! resolution, as PHPStan parses a file).

use std::path::Path;
use std::process::Command;

use serde_json::Value;

use super::jsonl::{by_id, by_id_values, parse_lines, unported};
use super::oracle::{Oracle, piped};
use super::score::{Case, Verdict};
use crate::files;

/// One digest suite.
pub(crate) struct Suite {
    pub(crate) name: &'static str,
    /// The extractor under `parity/extract/`.
    extractor: &'static str,
    /// The fields of a case that must be equal, in the order they are checked.
    fields: &'static [&'static str],
}

pub(crate) const LEXER: Suite = Suite {
    name: "lexer",
    extractor: "lexer.php",
    fields: &["tokens", "sha256", "errors"],
};

pub(crate) const PARSER: Suite = Suite {
    name: "parser",
    extractor: "parser.php",
    fields: &["errors", "nodes", "sha256"],
};

/// The inputs: every `.php` file under phpstan-src's `tests` and `vendor`,
/// one `/`-spelled path per line, in byte order.
fn inputs(oracle: &Oracle) -> Result<Vec<u8>, String> {
    let mut paths = files::php_files(&oracle.src, &oracle.src.join("tests"))?;
    paths.extend(files::php_files(&oracle.src, &oracle.src.join("vendor"))?);
    paths.sort_unstable();
    let mut out = Vec::new();
    for path in paths {
        out.extend_from_slice(path.as_bytes());
        out.push(b'\n');
    }
    Ok(out)
}

pub(crate) fn run(suite: &Suite, oracle: &Oracle, binary: &Path) -> Result<Vec<Case>, String> {
    let inputs = inputs(oracle)?;
    let expected = by_id(&oracle.extract(suite.name, &[suite.extractor], &[], &inputs)?)?;
    let output = piped(
        Command::new(binary)
            .args(["debug", suite.name])
            .arg(&oracle.src),
        &inputs,
    )?;
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

/// The first path component, `tests` or `vendor`.
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
pub(crate) fn show(suite: &Suite, oracle: &Oracle, binary: &Path, id: &str) -> Result<(), String> {
    let work = oracle.cache.join(suite.name).join("work");
    let want = oracle.php(suite.extractor, &["--show", id], &[], &work)?;
    let got = piped(
        Command::new(binary)
            .args(["debug", suite.name])
            .arg(&oracle.src)
            .args(["--show", id]),
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
