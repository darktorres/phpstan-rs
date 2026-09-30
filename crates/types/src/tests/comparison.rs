//! The comparison model against PHP: every pair of a fixed set of values,
//! evaluated by `tools/php-comparison-matrix/matrix.php` on each PHP
//! version, must never be decided otherwise than PHP decided it.

use rustc_hash::FxHashMap;
use syntax::name::Name;

use crate::{
    CallableSig, ClassFacts, ClassHierarchy, ClassKind, OrderingOp, Outcomes, PhpVersion, Relation,
    Shape, ShapeField, ShapeKey, TrinaryLogic, Type, Variance,
};

/// The matrix's classes: `P` and `Q` unrelated, `S` with `__toString`.
struct MatrixClasses;

impl ClassHierarchy for MatrixClasses {
    fn is_subclass_of(&self, class: &str, of: &str) -> bool {
        class.eq_ignore_ascii_case(of)
    }

    fn find_class(&self, name: &str) -> Option<ClassFacts<'_>> {
        matches!(name, "P" | "Q" | "S").then_some(ClassFacts {
            kind: ClassKind::Class,
            is_final: false,
            templates: &[],
        })
    }

    fn ancestor_names(&self, _: &str) -> Vec<Name> {
        Vec::new()
    }

    fn has_method(&self, class: &str, method: &str) -> bool {
        class == "S" && method.eq_ignore_ascii_case("__toString")
    }

    fn is_static_method(&self, _: &str, _: &str) -> bool {
        false
    }

    fn method_signature(&self, _: &str, _: &str) -> Option<CallableSig> {
        None
    }

    fn function_exists(&self, _: &str) -> TrinaryLogic {
        TrinaryLogic::Maybe
    }

    fn method_return_type(&self, _: &str, _: &str) -> Option<Type> {
        None
    }

    fn property_type(&self, _: &str, _: &str) -> Option<Type> {
        None
    }

    fn slot_variance(&self, _: &str, _: usize) -> Option<Variance> {
        None
    }

    fn ancestor_template_bindings(&self, _: &str, _: &[Type], _: &str) -> FxHashMap<String, Type> {
        FxHashMap::default()
    }
}

fn list(values: Vec<Type>) -> Type {
    Type::array_shape(Shape::sealed(
        values
            .into_iter()
            .enumerate()
            .map(|(i, v)| ShapeField::new(ShapeKey::Int(i as i64), v, false))
            .collect(),
    ))
}

fn string(s: &str) -> Type {
    Type::string_literal(s.into())
}

/// The type of the value the matrix labels `label`.
fn value(label: &str) -> Type {
    match label {
        "null" => Type::null(),
        "true" => Type::bool_literal(true),
        "false" => Type::bool_literal(false),
        "PHP_INT_MAX" => Type::int_literal(i64::MAX),
        "0.0" => Type::float_literal(0.0),
        "-0.0" => Type::float_literal(-0.0),
        "1.5" => Type::float_literal(1.5),
        "INF" => Type::float_literal(f64::INFINITY),
        "NAN" => Type::float_literal(f64::NAN),
        "array{}" => list(vec![]),
        "array{1}" => list(vec![Type::int_literal(1)]),
        "array{'1'}" => list(vec![string("1")]),
        "array{1, 2}" => list(vec![Type::int_literal(1), Type::int_literal(2)]),
        "array{a: 1}" => Type::array_shape(Shape::from_named([("a", Type::int_literal(1), false)])),
        "array{b: 1}" => Type::array_shape(Shape::from_named([("b", Type::int_literal(1), false)])),
        "P" | "Q" | "S" => Type::object(label.into(), Vec::new()),
        _ if label.starts_with('\'') => string(&label[1..label.len() - 1]),
        _ => Type::int_literal(
            label
                .parse()
                .unwrap_or_else(|_| panic!("unknown value {label}")),
        ),
    }
}

fn label(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn check(data: &str, version: PhpVersion) {
    let relation = Relation {
        hierarchy: &MatrixClasses,
        ..Relation::structural()
    };
    let json: serde_json::Value = serde_json::from_str(data).unwrap();
    let mut wrong = Vec::new();
    let (mut decided, mut total) = (0, 0);
    for row in json["results"].as_array().unwrap() {
        let row = row.as_array().unwrap();
        let (a, b) = (label(&row[0]), label(&row[1]));
        let (ta, tb) = (value(&a), value(&b));
        let checks: [(&str, TrinaryLogic); 5] = [
            ("==", relation.loose_equal(&ta, &tb, Some(version))),
            (
                "<",
                relation.ordering(&ta, OrderingOp::Lt, &tb, Some(version)),
            ),
            (
                "<=",
                relation.ordering(&ta, OrderingOp::LtEq, &tb, Some(version)),
            ),
            (
                ">",
                relation.ordering(&ta, OrderingOp::Gt, &tb, Some(version)),
            ),
            (
                ">=",
                relation.ordering(&ta, OrderingOp::GtEq, &tb, Some(version)),
            ),
        ];
        for (i, (op, verdict)) in checks.iter().enumerate() {
            let php = row[2 + i].as_bool().unwrap();
            total += 1;
            if !verdict.maybe() {
                decided += 1;
            }
            if (verdict.yes() && !php) || (verdict.no() && php) {
                wrong.push(format!("{a} {op} {b}: PHP {php}, model {verdict:?}"));
            }
        }
        let spaceship = row[7].as_i64().unwrap();
        let outcomes = relation.outcomes(&ta, &tb, Some(version));
        let allowed = match spaceship {
            -1 => Outcomes::LESS,
            0 => Outcomes::EQUAL,
            _ => Outcomes::GREATER,
        };
        // `<=>` gives 1 for an uncomparable pair too.
        let found = outcomes.contains(allowed)
            || (spaceship == 1 && outcomes.contains(Outcomes::UNCOMPARABLE));
        if !found {
            wrong.push(format!("{a} <=> {b}: PHP {spaceship}, model {outcomes:?}"));
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
fn comparisons_on_php_8_5_hold_as_php_evaluates_them() {
    check(
        include_str!("data/comparison-8.5.json"),
        PhpVersion::new(8, 5),
    );
}

#[test]
fn comparisons_on_php_8_0_hold_as_php_evaluates_them() {
    check(
        include_str!("data/comparison-8.0.json"),
        PhpVersion::new(8, 0),
    );
}

#[test]
fn comparisons_on_php_7_4_hold_as_php_evaluates_them() {
    check(
        include_str!("data/comparison-7.4.json"),
        PhpVersion::new(7, 4),
    );
}
