//! `Type::mentions_any` is `mentions` for each name.
use super::super::*;

#[test]
fn mentions_any_is_mentions_for_each_name() {
    let names = ["callable", "Closure", "Traversable", "iterable", "int"];
    let object = |name: &str| Type::object(name.into(), Vec::new());
    let types = [
        Type::int(),
        Type::string(),
        Type::callable(None),
        object("Closure"),
        object("\\closure"),
        object("Foo"),
        Type::array(Type::int(), object("Traversable")),
        Type::union_of(vec![Type::null(), Type::list_of(Type::callable(None))]),
        Type::object("Box".into(), vec![object("Iterator")]),
        Type::array(Type::string(), Type::string()),
    ];
    for ty in &types {
        assert_eq!(
            ty.mentions_any(&names),
            names.iter().any(|name| ty.mentions(name)),
            "{ty}"
        );
    }
    assert!(!Type::string().mentions_any(&[]));
}

#[test]
fn the_first_and_single_object_class_are_those_of_the_list() {
    /// `object_class_names` as it was made: a list for each member, joined.
    fn by_lists(ty: &Type) -> Vec<Name> {
        match ty.repr() {
            Repr::Object(class, _) | Repr::EnumCase(class, _) => vec![class.clone()],
            Repr::Union(members) | Repr::BenevolentUnion(members) | Repr::Intersection(members) => {
                members.iter().flat_map(by_lists).collect()
            }
            _ => Vec::new(),
        }
    }
    for text in [
        "int",
        "Foo",
        "Foo|Bar",
        "Foo|null",
        "(Foo|Bar)|Baz",
        "Foo&Bar",
        "Foo&Bar&Baz",
        "array<Foo>",
        "list<Foo>|Bar",
        "int|string",
        "never",
        "object",
        "Foo|int|Bar|string|Baz",
    ] {
        let ty = parse_type_string(text);
        let expected = by_lists(&ty);
        let names: Vec<Name> = ty.object_class_names().into_iter().cloned().collect();
        assert_eq!(names, expected, "{text}");
        assert_eq!(ty.first_object_class(), expected.first(), "{text}");
        assert_eq!(
            ty.single_object_class(),
            (expected.len() == 1).then(|| &expected[0]),
            "{text}"
        );
    }
}
