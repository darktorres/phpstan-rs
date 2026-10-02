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
