//! The lexer suite: nikic/php-parser's token stream for every input, as it
//! is and with its line breaks rewritten (`parity/extract/lexer.php`).

use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

use super::oracle::{Oracle, piped};
use super::score::{Case, Verdict};
use crate::files;

pub(crate) const NAME: &str = "lexer";
const EXTRACTOR: &str = "lexer.php";

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

pub(crate) fn run(oracle: &Oracle, binary: &Path) -> Result<Vec<Case>, String> {
    let inputs = inputs(oracle)?;
    let expected = by_id(&oracle.extract(NAME, EXTRACTOR, &[], &inputs)?)?;
    let output = piped(
        Command::new(binary).args(["debug", NAME]).arg(&oracle.src),
        &inputs,
    )?;
    let actual = parse_lines(&output)?;
    if let Some(reason) = unported(&actual) {
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
                Some(got) => compare(&want, &got),
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

fn compare(want: &Value, got: &Value) -> Verdict {
    for field in ["tokens", "sha256", "errors"] {
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

/// `--show <id>`: prints the first token where the two sides differ.
pub(crate) fn show(oracle: &Oracle, binary: &Path, id: &str) -> Result<(), String> {
    let want = oracle.php(EXTRACTOR, &["--show", id], &[])?;
    let got = piped(
        Command::new(binary)
            .args(["debug", NAME])
            .arg(&oracle.src)
            .args(["--show", id]),
        &[],
    )?;
    let want = String::from_utf8_lossy(&want);
    let got = String::from_utf8_lossy(&got);
    if let Some(reason) = got
        .lines()
        .next()
        .and_then(|line| unported(&[serde_json::from_str(line).ok()?]))
    {
        println!("{reason}; the expected stream starts:");
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
        None => println!("{id}: the token streams are equal"),
        Some((index, (w, g))) => println!(
            "{id}: token {index} differs\n  expected: {}\n  got:      {}",
            w.unwrap_or("(end)"),
            g.unwrap_or("(end)")
        ),
    }
    Ok(())
}

/// A binary that has no lexer yet answers with one `{"unported": ...}` line.
fn unported(lines: &[Value]) -> Option<String> {
    let [line] = lines else { return None };
    line.get("unported")
        .map(|_| "the lexer is not ported".to_owned())
}

fn parse_lines(bytes: &[u8]) -> Result<Vec<Value>, String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).map_err(|error| format!("{error}: {line}")))
        .collect()
}

fn by_id(bytes: &[u8]) -> Result<BTreeMap<String, Value>, String> {
    by_id_values(parse_lines(bytes)?)
}

fn by_id_values(lines: Vec<Value>) -> Result<BTreeMap<String, Value>, String> {
    let mut map = BTreeMap::new();
    for line in lines {
        let id = line
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("a case has no id: {line}"))?
            .to_owned();
        if map.insert(id.clone(), line).is_some() {
            return Err(format!("case {id} appears twice"));
        }
    }
    Ok(map)
}
