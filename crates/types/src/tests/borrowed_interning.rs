//! A form looked for by the parts it borrows is the node the owned form
//! is, with the same hash, and nothing is made when it exists.
use super::super::*;
use super::shortcuts::any_type;
use crate::intern::ReprRef;
use proptest::prelude::*;
use std::sync::Arc;

proptest! {
    #[test]
    fn the_borrowed_and_the_owned_form_are_one_node(ty in any_type()) {
        let repr = ty.repr().clone();
        // The owned form of the repr is the node the type already is.
        prop_assert!(Arc::ptr_eq(&Type::new(repr.clone()).0, &ty.0));
        // The borrowed form is too, and the hash the node holds is the one
        // either form gives.
        let borrowed = Type::from_ref(repr.as_ref());
        prop_assert!(Arc::ptr_eq(&borrowed.0, &ty.0));
        let owned = Type::from_ref(ReprRef::Owned(&repr));
        prop_assert!(Arc::ptr_eq(&owned.0, &ty.0));
    }

    #[test]
    fn the_constructors_that_borrow_give_what_the_owned_ones_give(
        class in "[A-C][a-z]{0,3}",
        text in "[a-z]{0,5}",
        args in prop::collection::vec(any_type(), 0..3),
        inner in any_type(),
    ) {
        let class: Name = class.into();
        let literal = PhpString::from(text.as_str());
        prop_assert!(Arc::ptr_eq(
            &Type::object_ref(&class, &args).0,
            &Type::object(class.clone(), args.clone()).0
        ));
        prop_assert!(Arc::ptr_eq(
            &Type::string_literal_ref(&literal).0,
            &Type::string_literal(literal.clone()).0
        ));
        prop_assert!(Arc::ptr_eq(
            &Type::enum_case_ref(&class, &class).0,
            &Type::enum_case(class.clone(), class.clone()).0
        ));
        prop_assert!(Arc::ptr_eq(
            &Type::class_string_ref(&inner).0,
            &Type::class_string(inner.clone()).0
        ));
    }
}

#[test]
fn a_form_that_does_not_exist_yet_is_made_from_a_copy_of_its_parts() {
    let class: Name = "BorrowedInterningTestClass".into();
    let args = [Type::int(), Type::string()];
    let made = Type::object_ref(&class, &args);
    assert_eq!(
        made.as_object().map(|(n, a)| (n.clone(), a.len())),
        Some((class.clone(), 2))
    );
    // Asked for again, it is the same node, not a second one.
    assert!(Arc::ptr_eq(&made.0, &Type::object(class, args.to_vec()).0));
}
