//! The canonical print of a type: every part shown, nothing read from the
//! interning.
use super::super::*;
use crate::Repr;

#[test]
fn a_unions_members_print_sorted_whatever_their_order() {
    let a = Type::union_of(vec![Type::null(), Type::int(), Type::string()]);
    let b = Type::union_of(vec![Type::string(), Type::null(), Type::int()]);
    assert_eq!(a.canonical_text(), "(int | null | string)");
    assert_eq!(b.canonical_text(), a.canonical_text());
    let intersection = Type::intersection_of(vec![
        Type::accessory(AccessoryKind::NonEmptyString),
        Type::string(),
    ]);
    assert_eq!(intersection.canonical_text(), "(non-empty-string & string)");
}

#[test]
fn mixed_prints_whether_it_was_written_and_what_it_subtracts() {
    assert_eq!(Type::mixed().canonical_text(), "mixed(implicit)");
    assert_eq!(Type::explicit_mixed().canonical_text(), "mixed(explicit)");
    let without_null = Type::new(Repr::Mixed(true, Some(Type::null())));
    assert_eq!(without_null.canonical_text(), "mixed(explicit, minus null)");
}

#[test]
fn a_literal_string_prints_its_bytes() {
    let text = Type::string_literal(PhpString::from_bytes(b"a'b\\c\x00\xc3\xa9\xff".to_vec()));
    assert_eq!(text.canonical_text(), r"'a\'b\\c\x00é\xff'");
}

#[test]
fn a_literal_float_prints_its_bits() {
    assert_eq!(Type::float_literal(1.5).canonical_text(), "float(1.5)");
    assert_eq!(Type::float_literal(0.0).canonical_text(), "float(0.0)");
    assert_eq!(Type::float_literal(-0.0).canonical_text(), "float(-0.0)");
    assert_eq!(
        Type::float_literal(f64::NAN).canonical_text(),
        format!("float(NaN 0x{:016x})", f64::NAN.to_bits())
    );
}

#[test]
fn a_class_prints_after_a_backslash() {
    let resource = Type::object("resource".into(), vec![Type::int()]);
    assert_eq!(resource.canonical_text(), r"\resource<int>");
    assert_eq!(Type::resource(false).canonical_text(), "resource");
    assert_eq!(Type::resource(true).canonical_text(), "closed-resource");
}

#[test]
fn a_template_prints_its_scope_bound_variance_and_default() {
    let template = Type::template_param(TemplateParam {
        name: "T".into(),
        bound: Box::new(Type::int()),
        variance: Variance::Covariant,
        scope: Some(r"Foo\Bar".into()),
        default: Some(Box::new(Type::int_literal(1))),
    });
    assert_eq!(
        template.canonical_text(),
        r"template(T, scope: Foo\Bar, bound: int, variance: covariant, default: 1)"
    );
    let unbounded = Type::template_param(TemplateParam::unbounded("U".into()));
    assert_eq!(
        unbounded.canonical_text(),
        "template(U, scope: -, bound: mixed(explicit), variance: invariant, default: -)"
    );
}

#[test]
fn a_shape_prints_each_fields_optionality_and_whether_it_is_sealed() {
    let fields = vec![
        ShapeField::new(ShapeKey::Int(0), Type::int(), false),
        ShapeField::new(ShapeKey::from_name("name"), Type::string(), true),
    ];
    let sealed = Type::array_shape(Shape::sealed(fields.clone()));
    assert_eq!(sealed.canonical_text(), "array{0: int, 'name'?: string}");
    let unsealed = Type::array_shape(Shape::from_parts(
        fields,
        Some(Box::new((Type::int(), Type::bool()))),
        None,
    ));
    assert_eq!(
        unsealed.canonical_text(),
        "array{0: int, 'name'?: string, ...<int, bool>}"
    );
}

#[test]
fn a_callable_prints_each_parameters_passing_and_its_templates() {
    let sig = CallableSig {
        params: vec![
            CallableParam {
                ty: Type::int(),
                variadic: false,
                optional: true,
                by_ref: true,
            },
            CallableParam {
                ty: Type::string(),
                variadic: true,
                optional: false,
                by_ref: false,
            },
        ],
        return_type: Box::new(Type::void()),
        templates: vec![],
    };
    assert_eq!(
        Type::callable(Some(std::sync::Arc::new(sig))).canonical_text(),
        "callable(&int=, string...): void"
    );
}

#[test]
fn a_conditional_prints_its_subjects_kind() {
    let conditional = Type::conditional(
        ConditionalSubject::Parameter("value".into()),
        Type::null(),
        Type::int(),
        Type::string(),
        true,
    );
    assert_eq!(
        conditional.canonical_text(),
        "(parameter $value is not null ? int : string)"
    );
}
