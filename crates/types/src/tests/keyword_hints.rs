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

#[test]
fn a_plain_word_parses_as_the_syntax_reading_gives() {
    use crate::parse::{is_plain_word, parse_type_atom, parse_type_atom_by_form};
    // Every word of up to four characters over an alphabet with the
    // characters that matter: letters of both cases, digits, `_`, `-`,
    // `\`, and the letters of the words Rust reads as floats.
    let alphabet: Vec<char> = "aZ_9-\\niNfEe".chars().collect();
    let mut words = vec![String::new()];
    let mut frontier = words.clone();
    for _ in 0..4 {
        let mut next = Vec::new();
        for word in &frontier {
            for c in &alphabet {
                next.push(format!("{word}{c}"));
            }
        }
        words.extend(next.iter().cloned());
        frontier = next;
    }
    for word in words.iter().map(String::as_str).chain([
        "inf",
        "Infinity",
        "NaN",
        "INF",
        "nan",
        "closure",
        "Callable",
        "array",
        "list",
        "non-empty-array",
        "object",
        "static",
        "$this",
        "int-mask",
    ]) {
        if is_plain_word(word) {
            assert_eq!(
                parse_type_atom(word),
                parse_type_atom_by_form(word),
                "{word:?}"
            );
        }
    }
}

#[test]
fn the_plain_word_test_is_what_it_was_before_it_used_a_table() {
    use crate::parse::is_plain_word;
    /// The test as first written.
    fn by_iterators(s: &str) -> bool {
        let bytes = s.as_bytes();
        let Some(&first) = bytes.first() else {
            return false;
        };
        (first.is_ascii_alphabetic() || first == b'_' || first == b'\\')
            && bytes
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'\\'))
            && !["inf", "infinity", "nan"]
                .iter()
                .any(|word| s.eq_ignore_ascii_case(word))
    }
    for word in [
        "",
        "a",
        "_",
        "\\",
        "\\Foo",
        "Foo\\Bar",
        "non-empty-string",
        "inf",
        "INF",
        "Inf",
        "nan",
        "NaN",
        "NAN",
        "infinity",
        "Infinity",
        "INFINITY",
        "infinit",
        "infinityy",
        "nana",
        "ina",
        "1a",
        "a1",
        "a b",
        "a.b",
        "a|b",
        "é",
        "aé",
        "a-",
        "-a",
        "a_b",
        "\u{10348}",
        "Foo?",
    ] {
        assert_eq!(is_plain_word(word), by_iterators(word), "{word:?}");
    }
    // Every word of up to three bytes over an alphabet that has the
    // characters the test tells apart.
    let alphabet: Vec<char> = "aiInNfF_-\\0 .é".chars().collect();
    let mut words = vec![String::new()];
    for _ in 0..3 {
        let longer: Vec<String> = words
            .iter()
            .flat_map(|w| alphabet.iter().map(move |c| format!("{w}{c}")))
            .collect();
        for word in &longer {
            assert_eq!(is_plain_word(word), by_iterators(word), "{word:?}");
        }
        words = longer;
    }
}
