//! The suites taken from PHPStan's own tests, run under the recording
//! bootstrap (`parity/extract/tests.php`): types, one case per assert of a
//! type-inference test, and rules, one case per `analyse()` or `fix()` call
//! of a rule test.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;
use std::process::Command;

use serde_json::Value;

use super::jsonl::{by_id_values, parse_lines, unported};
use super::oracle::{Oracle, piped};
use super::score::{Case, Verdict};

pub(crate) const TYPES: &str = "types";
pub(crate) const RULES: &str = "rules";

/// The tests' records, shared by both suites.
fn records(oracle: &Oracle) -> Result<Vec<u8>, String> {
    oracle.extract("tests", &["tests.php", "bootstrap.php"], &[], &[])
}

pub(crate) fn run_types(oracle: &Oracle, binary: &Path) -> Result<Vec<Case>, String> {
    let records = records(oracle)?;
    let asserts = oracle.extract(TYPES, &["asserts.php"], &[], &records)?;
    let mut expected = BTreeMap::new();
    let mut input = Vec::new();
    for mut assert in parse_lines(&asserts)? {
        let field = |name: &str| assert.get(name).cloned().unwrap_or(Value::Null);
        let id = format!(
            "{}:{}:{}:{}",
            field("file").as_str().unwrap_or_default(),
            field("line"),
            field("column"),
            field("kind").as_str().unwrap_or_default()
        );
        let group = field("kind").as_str().unwrap_or_default().to_owned();
        let want = field("expected");
        assert["id"] = Value::String(id.clone());
        input.extend_from_slice(assert.to_string().as_bytes());
        input.push(b'\n');
        if expected.insert(id.clone(), (group, want)).is_some() {
            return Err(format!("assert {id} appears twice"));
        }
    }
    let output = piped(
        Command::new(binary).args(["debug", TYPES]).arg(&oracle.src),
        &input,
    )?;
    judge(TYPES, expected, &output, |want, got| {
        let actual = got.get("actual").unwrap_or(&Value::Null);
        if actual == want {
            Verdict::Pass
        } else {
            Verdict::Fail(format!("expected {want}, got {actual}"))
        }
    })
}

/// The expected and actual values of a types case's failure detail,
/// `expected <json>, got <json>`; `None` for a part that does not read.
pub(crate) fn expected_and_actual(detail: &str) -> (Option<Value>, Option<Value>) {
    let Some(rest) = detail.strip_prefix("expected ") else {
        return (None, None);
    };
    let mut values = serde_json::Deserializer::from_str(rest).into_iter::<Value>();
    let Some(Ok(expected)) = values.next() else {
        return (None, None);
    };
    let actual = rest[values.byte_offset()..]
        .strip_prefix(", got ")
        .and_then(|got| serde_json::from_str(got).ok());
    (Some(expected), actual)
}

pub(crate) fn run_rules(oracle: &Oracle, binary: &Path) -> Result<Vec<Case>, String> {
    let records = records(oracle)?;
    let mut expected = BTreeMap::new();
    let mut input = Vec::new();
    for mut record in parse_lines(&records)? {
        let kind = record
            .get("kind")
            .and_then(Value::as_str)
            .unwrap_or_default();
        if kind != "analyse" && kind != "fix" {
            continue;
        }
        let text = |name: &str| {
            record
                .get(name)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let test = text("test");
        // PHPUnit names a data set by its key: a string, or an int for a
        // list; a test without a data provider has the empty string.
        let data_set = match record.get("data_set") {
            Some(Value::String(name)) => name.clone(),
            Some(Value::Number(index)) => index.to_string(),
            _ => String::new(),
        };
        let call = record.get("call").cloned().unwrap_or(Value::Null);
        let id = if data_set.is_empty() {
            format!("{test}::{}@{call}", text("method"))
        } else {
            format!("{test}::{}#{data_set}@{call}", text("method"))
        };
        // The test's namespace below PHPStan\, such as Rules\Methods.
        let group = test
            .rsplit_once('\\')
            .map_or("", |(namespace, _)| namespace)
            .trim_start_matches("PHPStan\\")
            .to_owned();
        let want = if kind == "fix" {
            let path = oracle.src.join(text("expected"));
            let fixed = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
            serde_json::json!({ "fixed": String::from_utf8_lossy(&fixed) })
        } else {
            serde_json::json!({ "errors": normalized(record.get("expected")) })
        };
        record["id"] = Value::String(id.clone());
        input.extend_from_slice(record.to_string().as_bytes());
        input.push(b'\n');
        if expected.insert(id.clone(), (group, want)).is_some() {
            return Err(format!("rule case {id} appears twice"));
        }
    }
    let output = piped(
        Command::new(binary).args(["debug", RULES]).arg(&oracle.src),
        &input,
    )?;
    judge(RULES, expected, &output, |want, got| {
        let matches = match want.get("errors") {
            Some(errors) => *errors == normalized(got.get("errors")),
            None => want.get("fixed") == got.get("fixed"),
        };
        if matches {
            Verdict::Pass
        } else {
            Verdict::Fail(format!("expected {want}, got {got}"))
        }
    })
}

/// Expected errors as `RuleTestCase::analyse` compares them: a multiset of
/// `[message, line, tip]`, here sorted, with a missing tip as null.
fn normalized(errors: Option<&Value>) -> Value {
    let mut list: Vec<Value> = errors
        .and_then(Value::as_array)
        .map(|errors| {
            errors
                .iter()
                .map(|error| {
                    let item = |i: usize| error.get(i).cloned().unwrap_or(Value::Null);
                    Value::Array(vec![item(0), item(1), item(2)])
                })
                .collect()
        })
        .unwrap_or_default();
    list.sort_by_key(Value::to_string);
    Value::Array(list)
}

/// Compares the binary's output lines with the expected cases.
fn judge(
    suite: &str,
    expected: BTreeMap<String, (String, Value)>,
    output: &[u8],
    compare: impl Fn(&Value, &Value) -> Verdict,
) -> Result<Vec<Case>, String> {
    let lines = parse_lines(output)?;
    if let Some(reason) = unported(suite, &lines) {
        return Ok(expected
            .into_iter()
            .map(|(id, (group, _))| Case {
                id,
                group,
                verdict: Verdict::Fail(reason.clone()),
            })
            .collect());
    }
    let mut actual = by_id_values(lines)?;
    Ok(expected
        .into_iter()
        .map(|(id, (group, want))| {
            let verdict = match actual.remove(&id) {
                None => Verdict::Error("phpstan-rs printed nothing for this case".to_owned()),
                Some(got) => compare(&want, &got),
            };
            Case { id, group, verdict }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_types_detail_is_read_back() {
        let want = Value::String("'a, got b'".to_owned());
        let got = Value::String("string".to_owned());
        let detail = format!("expected {want}, got {got}");
        assert_eq!(
            expected_and_actual(&detail),
            (Some(want.clone()), Some(got))
        );
        let nothing = format!("expected {want}, got {}", Value::Null);
        assert_eq!(
            expected_and_actual(&nothing),
            (Some(want), Some(Value::Null))
        );
        assert_eq!(expected_and_actual("other text"), (None, None));
    }

    #[test]
    fn expected_errors_compare_as_a_multiset() {
        let a = serde_json::json!([["b", 2], ["a", 1, "tip"]]);
        let b = serde_json::json!([["a", 1, "tip"], ["b", 2, null]]);
        assert_eq!(normalized(Some(&a)), normalized(Some(&b)));
        let c = serde_json::json!([["a", 1, "tip"], ["b", 2], ["b", 2]]);
        assert_ne!(normalized(Some(&a)), normalized(Some(&c)));
    }
}
