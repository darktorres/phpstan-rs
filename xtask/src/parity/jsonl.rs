//! JSON lines, as the extractors and the `debug` subcommands write them.

use std::collections::BTreeMap;

use serde_json::Value;

/// One value per non-empty line.
pub(crate) fn parse_lines(bytes: &[u8]) -> Result<Vec<Value>, String> {
    String::from_utf8_lossy(bytes)
        .lines()
        .filter(|line| !line.is_empty())
        .map(|line| serde_json::from_str(line).map_err(|error| format!("{error}: {line}")))
        .collect()
}

/// The lines keyed by their `id` field, which must be unique.
pub(crate) fn by_id(bytes: &[u8]) -> Result<BTreeMap<String, Value>, String> {
    by_id_values(parse_lines(bytes)?)
}

pub(crate) fn by_id_values(lines: Vec<Value>) -> Result<BTreeMap<String, Value>, String> {
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

/// A binary that lacks a suite's code answers with one `{"unported": ...}`
/// line; this is the reason every case of the suite then fails with.
pub(crate) fn unported(suite: &str, lines: &[Value]) -> Option<String> {
    let [line] = lines else { return None };
    line.get("unported")
        .map(|_| format!("the {suite} suite's code is not ported"))
}
