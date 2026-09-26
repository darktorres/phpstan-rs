//! Building an intersection type: nested intersections flattened, every
//! array member folded into one array, repeated members dropped.

use crate::{Relation, Type};

/// The intersection of `members`. `list<int>&non-empty-array` is one
/// array (`int` values, `int` keys) carrying both refinements, i.e.
/// `non-empty-list<int>`.
pub fn intersect_types(members: Vec<Type>) -> Type {
    let mut flat: Vec<Type> = Vec::new();
    for member in members {
        match member {
            Type::Intersection(inner) => flat.extend(inner),
            other => flat.push(other),
        }
    }
    let mut array: Option<(Type, Type)> = None;
    let mut rest: Vec<Type> = Vec::new();
    for member in flat {
        match member {
            Type::Array(key, value) => {
                array = Some(match array {
                    None => (*key, *value),
                    Some((k, v)) => (narrower(k, *key), narrower(v, *value)),
                });
            }
            other if !rest.contains(&other) => rest.push(other),
            _ => {}
        }
    }
    let mut out: Vec<Type> = Vec::new();
    if let Some((key, value)) = array {
        out.push(Type::Array(Box::new(key), Box::new(value)));
    }
    out.extend(rest);
    match out.len() {
        1 => out.pop().expect("one member"),
        _ => Type::Intersection(out),
    }
}

/// The narrower of two types when one contains the other, both otherwise.
fn narrower(a: Type, b: Type) -> Type {
    let relation = Relation::structural();
    if matches!(b, Type::Mixed(_)) || relation.subtype(&a, &b) {
        a
    } else if matches!(a, Type::Mixed(_)) || relation.subtype(&b, &a) {
        b
    } else {
        intersect_types(vec![a, b])
    }
}
