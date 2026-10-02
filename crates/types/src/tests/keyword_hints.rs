//! `parse::keyword_type`, the cached type of a keyword type declaration.
use super::super::*;
use syntax::ast::NativeType;

const KINDS: [NativeType; 17] = [
    NativeType::Int,
    NativeType::Float,
    NativeType::String,
    NativeType::Bool,
    NativeType::Array,
    NativeType::Object,
    NativeType::Mixed,
    NativeType::Void,
    NativeType::Never,
    NativeType::Null,
    NativeType::True,
    NativeType::False,
    NativeType::Iterable,
    NativeType::Callable,
    NativeType::SelfType,
    NativeType::Static,
    NativeType::Parent,
];

#[test]
fn a_keyword_hint_is_the_type_the_parser_reads_in_any_spelling() {
    for kind in KINDS {
        let canonical = kind.as_str();
        let spellings = [
            canonical.to_string(),
            canonical.to_ascii_uppercase(),
            format!("{}{}", canonical[..1].to_ascii_uppercase(), &canonical[1..]),
        ];
        for written in spellings {
            assert_eq!(
                crate::parse::keyword_type(kind),
                parse_type_string(&written),
                "{written}"
            );
            // The same node every time.
            assert!(std::ptr::eq(
                &*crate::parse::keyword_type(kind).0,
                &*crate::parse::keyword_type(kind).0
            ));
        }
    }
}
