//! Reading a callgrind out file: the total Ir and the self cost per
//! function.

use std::collections::BTreeMap;

/// The total Ir and the functions by self cost, largest first, with ties in
/// name order.
pub(crate) struct Profile {
    pub(crate) total: u64,
    pub(crate) functions: Vec<(String, u64)>,
}

/// Parses callgrind's format with one event (Ir) and line positions, the
/// defaults. A function's self cost is the sum of its cost lines, except
/// the line after each `calls=`, which is the inclusive cost of that call.
pub(crate) fn parse(text: &str) -> Result<Profile, String> {
    let mut names: BTreeMap<String, String> = BTreeMap::new();
    let mut costs: BTreeMap<String, u64> = BTreeMap::new();
    let mut total = None;
    let mut current: Option<String> = None;
    let mut skip_next = false;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("summary:") {
            total = Some(number(rest.trim())?);
        } else if let Some(rest) = line.strip_prefix("fn=") {
            current = Some(resolve(&mut names, rest));
        } else if line.starts_with("calls=") {
            skip_next = true;
        } else if line.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '+' | '-' | '*')) {
            if std::mem::take(&mut skip_next) {
                continue;
            }
            let Some(function) = &current else {
                return Err(format!("a cost line before any fn=: {line}"));
            };
            let cost = line.split_whitespace().nth(1).map_or(Ok(0), number)?;
            *costs.entry(function.clone()).or_default() += cost;
        } else if let Some(rest) = line.strip_prefix("cfn=") {
            // Defines a compressed name that a later fn= may refer to.
            resolve(&mut names, rest);
        }
    }
    let mut functions: Vec<(String, u64)> = costs.into_iter().collect();
    functions.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    Ok(Profile {
        total: total.ok_or("no summary: line")?,
        functions,
    })
}

/// `(id) name` defines a compressed name, `(id)` refers to one, and a bare
/// name stands for itself.
fn resolve(names: &mut BTreeMap<String, String>, spec: &str) -> String {
    let spec = spec.trim();
    match spec.strip_prefix('(').and_then(|rest| rest.split_once(')')) {
        Some((id, "")) => names.get(id).cloned().unwrap_or_else(|| format!("({id})")),
        Some((id, name)) => {
            let name = name.trim().to_owned();
            names.insert(id.to_owned(), name.clone());
            name
        }
        None => spec.to_owned(),
    }
}

fn number(text: &str) -> Result<u64, String> {
    text.parse()
        .map_err(|error| format!("not a count: {text}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_cost_leaves_out_call_costs_and_follows_compressed_names() {
        let text = "events: Ir\nsummary: 110\n\nfn=(1) main\n1 5\n+2 3\ncfn=(2) helper\ncalls=1 7\n9 100\n\nfn=(2)\n7 2\nfn=(1)\n3 1\n";
        let profile = parse(text).unwrap();
        assert_eq!(profile.total, 110);
        assert_eq!(
            profile.functions,
            vec![("main".to_owned(), 9), ("helper".to_owned(), 2)]
        );
    }
}
