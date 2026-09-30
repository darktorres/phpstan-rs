//! The comparator model against `sebastian/comparator`: every pair of a
//! fixed set of values, compared by
//! `tools/phpunit-comparator-matrix/matrix.php` with each comparator
//! major, must never be decided otherwise than the comparator decided it.

use super::comparison::{MatrixClasses, label, value};
use crate::{Relation, TrinaryLogic};

fn check(data: &str) {
    let relation = Relation {
        hierarchy: &MatrixClasses,
        ..Relation::structural()
    };
    let json: serde_json::Value = serde_json::from_str(data).unwrap();
    let mut wrong = Vec::new();
    let (mut decided, mut total) = (0, 0);
    for row in json["results"].as_array().unwrap() {
        let row = row.as_array().unwrap();
        let (e, a) = (label(&row[0]), label(&row[1]));
        let equal = row[2].as_bool().unwrap();
        let verdict = relation.comparator_equal(&value(&e), &value(&a), true);
        total += 1;
        if !verdict.maybe() {
            decided += 1;
        }
        if (verdict.yes() && !equal) || (verdict.no() && equal) {
            wrong.push(format!("{e} vs {a}: comparator {equal}, model {verdict:?}"));
        }
        // Inexact comparisons decide only what no option changes.
        let inexact = relation.comparator_equal(&value(&e), &value(&a), false);
        if inexact != TrinaryLogic::Maybe && inexact != verdict {
            wrong.push(format!(
                "{e} vs {a}: inexact {inexact:?}, exact {verdict:?}"
            ));
        }
    }
    assert!(
        wrong.is_empty(),
        "{} wrong:\n{}",
        wrong.len(),
        wrong.join("\n")
    );
    assert!(
        decided * 10 >= total * 9,
        "only {decided} of {total} answers decided"
    );
}

#[test]
fn comparator_4_equality_holds_as_it_compares() {
    check(include_str!("data/comparator-4.json"));
}

#[test]
fn comparator_5_equality_holds_as_it_compares() {
    check(include_str!("data/comparator-5.json"));
}

#[test]
fn comparator_6_equality_holds_as_it_compares() {
    check(include_str!("data/comparator-6.json"));
}

#[test]
fn comparator_7_equality_holds_as_it_compares() {
    check(include_str!("data/comparator-7.json"));
}
