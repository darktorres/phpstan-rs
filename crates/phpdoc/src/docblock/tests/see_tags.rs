//! Tests for `see_reference`: what a `@see` tag's target refers to.
use crate::docblock::{SeeMember, SeeReference, see_reference};

#[test]
fn a_see_target_refers_to_a_class_or_a_member_of_one_when_shaped_like_it() {
    let class = |class| SeeReference {
        class,
        member: None,
    };
    let member = |class, member| SeeReference {
        class,
        member: Some(member),
    };
    for (target, expected) in [
        ("Foo", Some(class("Foo"))),
        ("\\Lib\\Foo", Some(class("\\Lib\\Foo"))),
        ("Foo::bar()", Some(member("Foo", SeeMember::Method("bar")))),
        ("Foo::$baz", Some(member("Foo", SeeMember::Property("baz")))),
        ("Foo::QUX", Some(member("Foo", SeeMember::Constant("QUX")))),
        ("foo::bar()", Some(member("foo", SeeMember::Method("bar")))),
        ("Foo::bar(int $x)", Some(member("Foo", SeeMember::Other))),
        ("foo", None),
        ("foo()", None),
        ("https://example.com/Foo", None),
        ("Foo-Bar", None),
        ("\\", None),
        ("Lib\\\\Foo", None),
        ("::bar()", None),
    ] {
        assert_eq!(see_reference(target), expected, "{target}");
    }
}
